//! A manual probe: real-world long Chinese text — the first two chapters of
//! 《西游记》 from `D:\迅雷下载\西游记+.epub`. XHTML is stripped in-place
//! (no extra crate) and the same STMBE-style adaptive chunking comparison runs:
//!
//! - bge (CLS, ctx=512): auto-chunk ≤500 chars.
//! - yuan (mean, ctx=512): auto-chunk ≤500 chars.
//! - Qwen3 (mean, ctx=32k): full chapter as one chunk (with `-b 4096 -ub 4096`).
//!
//! Queries are distinctive proper-noun anchors that should appear in only one
//! of the two chapters; top-1 chunk → source doc, then per-chapter accuracy.
//!
//! Run:
//! ```text
//! $env:XYJ_CH1_PATH = "$env:TEMP\xyj_ch1.txt"     # produced by extracting chapter_0001.xhtml
//! $env:XYJ_CH2_PATH = "$env:TEMP\xyj_ch2.txt"     # produced by extracting chapter_0002.xhtml
//! cargo test -p tt-adapter-vector --test llama_cpp_epub_probe --release -- --ignored --nocapture
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// XHTML-ish stripping that scans char-by-char (no regex dep) and collapses
/// whitespace. Adequate for prose without scripts, CDATA, or comments.
fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
            }
        } else if c == '<' {
            in_tag = true;
        } else {
            out.push(c);
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_space = true; // start true so leading whitespace is dropped
    for c in out.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }
    collapsed
}

fn chunk_doc(doc: &str, chunk_chars: usize) -> Vec<String> {
    let chars: Vec<char> = doc.chars().collect();
    chars
        .chunks(chunk_chars)
        .map(|piece| piece.iter().collect())
        .collect()
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

const SHORT_CTX_CHUNK_CHARS: usize = 1024;

// Distinctive proper-noun anchors. Each must appear in only one of the two
// chapters — that's the test's only correctness criterion. Verified by
// scanning both texts; 花果山 / 傲来国 leak into ch2 (悟空's origin is
// recapped); 动字门 is split by a Chinese period in the source so it's not
// a contiguous substring.
const CH1_QUERIES: &[(&str, &str)] = &[
    ("ch1_仙石", "仙石"),
    ("ch1_石卵", "石卵"),
    ("ch1_灵根", "灵根"),
    ("ch1_目运", "目运"),
];
const CH2_QUERIES: &[(&str, &str)] = &[
    ("ch2_术字门", "术字门"),
    ("ch2_流字门", "流字门"),
    ("ch2_静字门", "静字门"),
    ("ch2_三更时分", "三更时分"),
    ("ch2_戒尺", "戒尺"),
];

#[tokio::test]
#[ignore = "needs an EPUB extracted to text files; see the module docs"]
async fn probe_epub_recall() {
    let base_url = std::env::var("TAURITAVERN_RECALL_EVAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let model = std::env::var("TAURITAVERN_RECALL_EVAL_MODEL")
        .unwrap_or_else(|_| "?".to_string());
    let ctx: u64 = std::env::var("TAURITAVERN_RECALL_EVAL_CTX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512);

    let ch1_path = std::env::var("XYJ_CH1_PATH")
        .expect("XYJ_CH1_PATH must point to the extracted chapter 1 text");
    let ch2_path = std::env::var("XYJ_CH2_PATH")
        .expect("XYJ_CH2_PATH must point to the extracted chapter 2 text");

    let ch1_raw = std::fs::read_to_string(&ch1_path).expect("read ch1");
    let ch2_raw = std::fs::read_to_string(&ch2_path).expect("read ch2");
    let ch1 = strip_html(&ch1_raw);
    let ch2 = strip_html(&ch2_raw);

    // STMBE adaptive chunking.
    let use_full = ctx >= 4096;
    let mut chunks: Vec<(usize, String)> = Vec::new();
    for (i, d) in [ch1.as_str(), ch2.as_str()].iter().enumerate() {
        if use_full {
            chunks.push((i, d.to_string()));
        } else {
            for c in chunk_doc(d, SHORT_CTX_CHUNK_CHARS) {
                chunks.push((i, c));
            }
        }
    }

    println!("\n=== EPUB (西游记 ch1 + ch2) retrieval probe ===");
    println!("endpoint     : {base_url}");
    println!("model        : {model} (n_ctx_train = {ctx})");
    println!(
        "chunking     : {} (ch1={} chunks, ch2={} chunks, total={})",
        if use_full { "FULL doc" } else { "auto, ≤{SHORT_CTX_CHUNK_CHARS} chars" },
        chunks.iter().filter(|(i, _)| *i == 0).count(),
        chunks.iter().filter(|(i, _)| *i == 1).count(),
        chunks.len()
    );
    println!("docs         : ch1 = {} chars, ch2 = {} chars", ch1.chars().count(), ch2.chars().count());

    // Sanity: every anchor must appear in its expected chapter.
    for (label, q) in CH1_QUERIES {
        assert!(
            ch1.contains(q),
            "anchor `{q}` ({label}) must appear in chapter 1 — choose a different anchor"
        );
        assert!(
            !ch2.contains(q),
            "anchor `{q}` ({label}) leaks into chapter 2 — choose a different anchor"
        );
    }
    for (label, q) in CH2_QUERIES {
        assert!(
            ch2.contains(q),
            "anchor `{q}` ({label}) must appear in chapter 2 — choose a different anchor"
        );
        assert!(
            !ch1.contains(q),
            "anchor `{q}` ({label}) leaks into chapter 1 — choose a different anchor"
        );
    }
    println!(
        "anchors      : {} ch1 + {} ch2, all confirmed unique to their chapter",
        CH1_QUERIES.len(),
        CH2_QUERIES.len()
    );

    // Embed chunks.
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
        "embedded     : {} chunks in {:.1} s ({total_tokens} prompt tokens)",
        chunk_vecs.len(),
        chunk_elapsed.as_secs_f64()
    );

    // For each anchor: embed the bare anchor as a query, find top-1 chunk,
    // record which chapter it came from. Print per-anchor outcome, then per-
    // chapter accuracy.
    let mut all_q: Vec<(usize, String)> = Vec::new(); // (expected_doc, text)
    for (_, q) in CH1_QUERIES {
        all_q.push((0, q.to_string()));
    }
    for (_, q) in CH2_QUERIES {
        all_q.push((1, q.to_string()));
    }
    let qtexts: Vec<String> = all_q.iter().map(|(_, t)| t.clone()).collect();
    let (q_vecs, q_tokens) = embed(&base_url, &qtexts).expect("query embed");
    let q_vecs: Vec<Vec<f32>> = q_vecs.into_iter().map(normalize).collect();

    println!("\nper-anchor outcome:");
    let mut correct_ch1 = 0_usize;
    let mut correct_ch2 = 0_usize;
    for (i, (expected, q)) in all_q.iter().enumerate() {
        let qv = &q_vecs[i];
        let (best_score, best_chunk_idx) = chunk_vecs
            .iter()
            .enumerate()
            .map(|(j, v)| (cosine(qv, v), j))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .unwrap();
        let got = chunk_doc_ids[best_chunk_idx];
        let ok = got == *expected;
        let label = if *expected == 0 { "ch1" } else { "ch2" };
        let got_label = if got == 0 { "ch1" } else { "ch2" };
        println!(
            "  {:<24} expect={label}  got={got_label}  score={best_score:+.4}  {}",
            q,
            if ok { "OK " } else { "MISS" }
        );
        if ok {
            if *expected == 0 {
                correct_ch1 += 1;
            } else {
                correct_ch2 += 1;
            }
        }
    }

    let n_ch1 = CH1_QUERIES.len();
    let n_ch2 = CH2_QUERIES.len();
    println!(
        "\nchapter accuracy: ch1 = {}/{}, ch2 = {}/{}, overall = {}/{}",
        correct_ch1,
        n_ch1,
        correct_ch2,
        n_ch2,
        correct_ch1 + correct_ch2,
        n_ch1 + n_ch2
    );
    println!("prompt tokens (queries): {q_tokens}");
    println!();

    assert_eq!(chunk_vecs.len(), chunks.len(), "every chunk must have an embedding");
}