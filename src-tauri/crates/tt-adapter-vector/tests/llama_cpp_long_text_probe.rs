//! A manual probe: long-text retrieval with a local llama.cpp embedding server,
//! simulating STMBE's auto-chunking behavior across models with different
//! context lengths.
//!
//! Premise: STMBE must adapt to any embedding backend. Short-context models
//! (bge / yuan at 512 tokens) cannot ingest a long chat message whole — the
//! llama.cpp server rejects it with HTTP 500 (`input too large to process`),
//! it does not silently truncate. So the system has to chunk client-side.
//! Long-context models (Qwen3 at 32k) ingest the full message.
//!
//! This probe models that adaptive chunking:
//!
//! - For each doc (~1300 chars), pick a chunk size: full doc if the model has
//!   ≥ 4096 tokens of context, else 500 chars (~500 tokens for bge's char-level
//!   tokenizer; safely under its 512-token ceiling).
//! - Embed each chunk as its own vector. Multiple chunks per doc for the
//!   short-ctx models, one per doc for the long-ctx model.
//! - Run 30 start-name queries (anchor at char ~57, inside every chunk) and
//!   30 end-name queries (anchor at char ~1280, past the first 500-char chunk
//!   for short-ctx models). For each query, take the top-1 chunk by cosine,
//!   map it to its source doc, and check recall.
//!
//! If chunking closes the gap, all backends land at similar recall — meaning
//! the choice between bge / yuan / Qwen3 is about index size and simplicity,
//! not retrieval quality. If chunking loses information (e.g., bge end-name
//! recall drops), the chat-text channel needs either a long-ctx model or
//! per-chunk aggregation more clever than a top-1 chunk lookup.
//!
//! Same conventions as the state-history probe: `#[ignore]`d, needs a local
//! llama.cpp embedding server. Run with:
//!
//! ```text
//! TAURITAVERN_RECALL_EVAL_URL=http://127.0.0.1:8091 \
//!   cargo test -p tt-adapter-vector --test llama_cpp_long_text_probe --release -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const NAMES_START: [&str; 30] = [
    "林远", "苏晚", "顾深", "沈默", "陈默", "周深", "白远", "何夕", "江澄", "魏婴",
    "蓝湛", "金子", "叶修", "周泽", "陈深", "王也", "诸葛", "欧阳", "司马", "上官",
    "慕容", "夏侯", "尉迟", "皇甫", "宇文", "长孙", "公孙", "南门", "东方", "令狐",
];

const NAMES_END: [&str; 30] = [
    "陆离", "楚云", "宋辞", "裴珩", "谢衍", "霍然", "顾昀", "费渡", "骆闻舟", "陶然",
    "盛望", "江添", "林无涯", "苏铭", "陈酒", "白瑾", "何晏", "温客", "顾剑", "秦玄",
    "夜深", "陆沉", "楚寒", "宋砚", "裴洛", "谢临", "霍城", "顾言", "费尧", "骆青",
];

const DOCS: usize = 30;
const BOILER_REPEATS: usize = 13;

/// Char-level chunk size for short-context models. bge-large-zh-v1.5's
/// tokenizer is roughly char-level (~1 token per char for Chinese), and its
/// server rejects inputs > 512 tokens. 500 chars leaves headroom for the BOS
/// token the server may prepend.
const SHORT_CTX_CHUNK_CHARS: usize = 500;

/// Generic narration boilerplate, identical across docs so the only
/// discriminative signal between doc_i and doc_j is the start_name and
/// end_name regions.
const BOILER: &str = "夜色渐深，街灯在雾气里散成一片暖黄。她靠着廊柱，听见远处传来钟声。\
风把窗帘吹得微微鼓起，又落回去。桌上的茶已经凉透了，没有人动它。\
他沿着走廊一直走到尽头，推开那扇木门时，门轴发出一声很轻的响。";

fn start_opening(i: usize) -> String {
    format!(
        "在所有故事开始的地方，第一个出现的人是{}。{}站在门口，背对着光，没有人看清他的脸。",
        NAMES_START[i], NAMES_START[i]
    )
}

fn end_closing(i: usize) -> String {
    format!(
        "直到最后，她才在记忆深处看清了那个名字：{}。{}是一切的答案，也是她一直在找的人。",
        NAMES_END[i], NAMES_END[i]
    )
}

fn build_doc(i: usize) -> String {
    let mut s = String::new();
    s.push_str(&start_opening(i));
    for _ in 0..BOILER_REPEATS {
        s.push_str(BOILER);
    }
    s.push_str(&end_closing(i));
    s
}

/// Char (not byte) offset of the first occurrence of `needle` in `haystack`.
/// `String::find` returns byte offset, which is wrong for non-ASCII content
/// in printouts and for slicing.
fn char_position(haystack: &str, needle: &str) -> usize {
    haystack
        .char_indices()
        .zip(needle.char_indices())
        .find(|((_, hb), (_, nb))| hb == nb)
        .map(|((hc, _), _)| hc)
        .unwrap_or_else(|| {
            // Fallback: use a char-by-char search to find the substring.
            let hchars: Vec<char> = haystack.chars().collect();
            let nchars: Vec<char> = needle.chars().collect();
            for start in 0..hchars.len().saturating_sub(nchars.len()) {
                if hchars[start..start + nchars.len()] == nchars[..] {
                    return start;
                }
            }
            0
        })
}

fn chunk_doc(doc: &str, chunk_chars: usize) -> Vec<String> {
    let chars: Vec<char> = doc.chars().collect();
    chars
        .chunks(chunk_chars)
        .map(|piece| piece.iter().collect())
        .collect()
}

fn query_for(name: &str) -> String {
    name.to_string()
}

fn embed(base_url: &str, inputs: &[String]) -> Result<(Vec<Vec<f32>>, u64), String> {
    let address = base_url
        .trim()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let (host, port) = address
        .split_once(':')
        .ok_or_else(|| format!("expected host:port in `{base_url}`"))?;
    let port: u16 = port.parse().map_err(|error| format!("bad port: {error}"))?;

    let body = serde_json::json!({ "input": inputs, "model": "local" }).to_string();
    let request = format!(
        "POST /v1/embeddings HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let mut stream = TcpStream::connect((host, port))
        .map_err(|error| format!("connect {host}:{port}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(600)))
        .map_err(|error| format!("set timeout: {error}"))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write request: {error}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("read response: {error}"))?;
    let response = String::from_utf8_lossy(&response);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response".to_string())?;
    let status = head.lines().next().unwrap_or_default().to_string();
    if !status.contains(" 200") {
        return Err(format!(
            "{status}: {}",
            body.chars().take(400).collect::<String>()
        ));
    }
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked(body)?
    } else {
        body.to_string()
    };

    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("bad JSON: {error}"))?;
    let data = parsed["data"]
        .as_array()
        .ok_or_else(|| "response has no data array".to_string())?;
    let mut vectors = vec![Vec::new(); inputs.len()];
    for entry in data {
        let index = entry["index"].as_u64().unwrap_or(0) as usize;
        let embedding: Vec<f32> = entry["embedding"]
            .as_array()
            .ok_or_else(|| "an entry has no embedding".to_string())?
            .iter()
            .map(|value| value.as_f64().unwrap_or_default() as f32)
            .collect();
        if index < vectors.len() {
            vectors[index] = embedding;
        }
    }
    if vectors.iter().any(Vec::is_empty) {
        return Err("the server returned fewer vectors than inputs".to_string());
    }
    let tokens = parsed["usage"]["prompt_tokens"].as_u64().unwrap_or_default();
    Ok((vectors, tokens))
}

fn decode_chunked(body: &str) -> Result<String, String> {
    let mut decoded = String::new();
    let mut rest = body;
    loop {
        let (size, remainder) = rest
            .split_once("\r\n")
            .ok_or_else(|| "malformed chunk size".to_string())?;
        let size = usize::from_str_radix(size.trim(), 16)
            .map_err(|error| format!("bad chunk size: {error}"))?;
        if size == 0 {
            return Ok(decoded);
        }
        let chunk = remainder
            .get(..size)
            .ok_or_else(|| "chunk shorter than its size".to_string())?;
        decoded.push_str(chunk);
        rest = remainder
            .get(size + 2..)
            .ok_or_else(|| "chunk missing its terminator".to_string())?;
    }
}

fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm > f64::EPSILON {
        for value in &mut vector {
            *value = (f64::from(*value) / norm) as f32;
        }
    }
    vector
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

/// Per-query top-1 chunk → source doc, then Recall@1 and Recall@k (any of the
/// top-k chunks belongs to the expected doc).
fn recall_at_k(
    qvecs: &[Vec<f32>],
    chunk_vecs: &[Vec<f32>],
    chunk_doc_ids: &[usize],
    k: usize,
) -> (f64, f64, f64) {
    let mut correct_at_1 = 0_usize;
    let mut correct_at_k = 0_usize;
    let mut margin_sum = 0_f64;
    for (i, q) in qvecs.iter().enumerate() {
        let mut sims: Vec<(f32, usize)> = chunk_vecs
            .iter()
            .zip(chunk_doc_ids.iter())
            .map(|(v, did)| (cosine(q, v), *did))
            .collect();
        sims.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let top1_doc = sims[0].1;
        if top1_doc == i {
            correct_at_1 += 1;
        }
        let top_k_docs: HashSet<usize> = sims.iter().take(k).map(|(_, d)| *d).collect();
        if top_k_docs.contains(&i) {
            correct_at_k += 1;
        }
        let correct_sim = sims
            .iter()
            .find(|(_, d)| *d == i)
            .map(|(s, _)| *s)
            .unwrap_or(0.0);
        margin_sum += f64::from(correct_sim - sims[0].0);
    }
    (
        correct_at_1 as f64 / qvecs.len() as f64,
        correct_at_k as f64 / qvecs.len() as f64,
        margin_sum / qvecs.len() as f64,
    )
}

#[tokio::test]
#[ignore = "needs a local llama.cpp embedding server; see the module docs"]
async fn probe_long_text_recall() {
    let base_url = std::env::var("TAURITAVERN_RECALL_EVAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let model = std::env::var("TAURITAVERN_RECALL_EVAL_MODEL")
        .unwrap_or_else(|_| "?".to_string());
    let ctx: u64 = std::env::var("TAURITAVERN_RECALL_EVAL_CTX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512);

    let docs: Vec<String> = (0..DOCS).map(build_doc).collect();
    let q_start: Vec<String> = (0..DOCS).map(|i| query_for(NAMES_START[i])).collect();
    let q_end: Vec<String> = (0..DOCS).map(|i| query_for(NAMES_END[i])).collect();

    let sample = &docs[0];
    let sample_chars = sample.chars().count();
    let pos_start = char_position(sample, NAMES_START[0]);
    let pos_end = char_position(sample, NAMES_END[0]);

    // STMBE's adaptive chunking: short-ctx models get 500-char chunks, long-ctx
    // models (Qwen3 at 32k) ingest the full message.
    let use_full = ctx >= 4096;
    let mut chunks: Vec<(usize, String)> = Vec::new(); // (doc_id, chunk_text)
    for (i, d) in docs.iter().enumerate() {
        if use_full {
            chunks.push((i, d.clone()));
        } else {
            for c in chunk_doc(d, SHORT_CTX_CHUNK_CHARS) {
                chunks.push((i, c));
            }
        }
    }

    println!("\n=== Long-text retrieval probe (STMBE adaptive chunking) ===");
    println!("endpoint         : {base_url}");
    println!("model            : {model} (n_ctx_train = {ctx})");
    println!(
        "chunking         : {} ({} chunks/doc, {} chunks total)",
        if use_full { "FULL doc" } else { "auto, ≤{SHORT_CTX_CHUNK_CHARS} chars" },
        if use_full { 1 } else { sample_chars.div_ceil(SHORT_CTX_CHUNK_CHARS) },
        chunks.len()
    );
    println!("docs             : {DOCS}, sample len = {sample_chars} chars");
    println!(
        "doc[0] anchors   : start_name @ char {pos_start}, end_name @ char {pos_end}"
    );

    // Confirm the end-name lives in the last chunk under short-ctx chunking.
    if !use_full {
        let doc_chunks: Vec<&String> = chunks
            .iter()
            .filter(|(did, _)| *did == 0)
            .map(|(_, t)| t)
            .collect();
        let end_in_last = char_position(doc_chunks.last().unwrap(), NAMES_END[0]) > 0
            || doc_chunks.last().unwrap().contains(NAMES_END[0]);
        println!(
            "                 : end_name {} in last chunk of doc[0] ({} chunks)",
            if end_in_last { "IS" } else { "is NOT" },
            doc_chunks.len()
        );
    }

    // Embed all chunks in batches.
    const BATCH: usize = 4;
    let mut chunk_vecs: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    let mut total_tokens = 0_u64;
    let started = Instant::now();
    for batch in chunks.chunks(BATCH) {
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let (vecs, tokens) = embed(&base_url, &texts).expect("chunk embed failed");
        total_tokens += tokens;
        for v in vecs {
            chunk_vecs.push(normalize(v));
        }
    }
    let chunk_elapsed = started.elapsed();
    let chunk_doc_ids: Vec<usize> = chunks.iter().map(|(d, _)| *d).collect();

    println!(
        "embedded chunks  : {} in {:.1} s ({total_tokens} total prompt tokens)",
        chunk_vecs.len(),
        chunk_elapsed.as_secs_f64()
    );

    // Embed queries.
    let (qstart_vecs, qs_tokens) = embed(&base_url, &q_start).expect("start queries embed");
    let (qend_vecs, qe_tokens) = embed(&base_url, &q_end).expect("end queries embed");
    let qs_vecs: Vec<Vec<f32>> = qstart_vecs.into_iter().map(normalize).collect();
    let qe_vecs: Vec<Vec<f32>> = qend_vecs.into_iter().map(normalize).collect();

    let (r1_s, r3_s, margin_s) = recall_at_k(&qs_vecs, &chunk_vecs, &chunk_doc_ids, 3);
    let (r1_e, r3_e, margin_e) = recall_at_k(&qe_vecs, &chunk_vecs, &chunk_doc_ids, 3);

    println!(
        "\nresults (cosine over unit vectors, top-1 chunk → doc, prompt_tokens qs={qs_tokens} qe={qe_tokens}):"
    );
    println!(
        "  start_name @ char {pos_start:<5} (in chunk 1):  \
         Recall@1 = {r1_s:.3}  Recall@3 = {r3_s:.3}  margin = {margin_s:+.4}"
    );
    println!(
        "  end_name   @ char {pos_end:<5} (in {} chunk):  \
         Recall@1 = {r1_e:.3}  Recall@3 = {r3_e:.3}  margin = {margin_e:+.4}",
        if use_full || pos_end < SHORT_CTX_CHUNK_CHARS { "first" } else { "last" }
    );
    println!(
        "\ndelta (end − start) = Recall@1 {:+.3}  Recall@3 {:+.3}  margin {:+.4}",
        r1_e - r1_s,
        r3_e - r3_s,
        margin_e - margin_s
    );
    if r1_s >= 0.9 && r1_e >= 0.9 {
        println!("verdict          : end-position retrieval survives — chunking / full-doc both work");
    } else if r1_s >= 0.9 && r1_e < 0.7 {
        println!(
            "verdict          : end-position recall drops (ΔRecall@1 = {:+.3}); \
             chunking alone does not save it",
            r1_e - r1_s
        );
    } else if r1_s < 0.5 && r1_e < 0.5 {
        println!(
            "verdict          : retrieval signal drowned overall (start={:.3} end={:.3}); \
             mean pooling over a long boilerplate-heavy doc loses the discriminative signal",
            r1_s, r1_e
        );
    } else if r1_e < 0.5 {
        println!(
            "verdict          : end-position recall low ({:.3}); \
             partial retrieval only",
            r1_e
        );
    } else {
        println!(
            "verdict          : mixed results — Recall@1 start={:.3} end={:.3} \
             Δ={:+.3}",
            r1_s, r1_e, r1_e - r1_s
        );
    }
    println!();

    assert_eq!(chunk_vecs.len(), chunks.len(), "every chunk must have an embedding");
    assert_eq!(qs_vecs.len(), DOCS, "every start query must have an embedding");
    assert_eq!(qe_vecs.len(), DOCS, "every end query must have an embedding");
}