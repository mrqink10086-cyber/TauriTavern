//! Real-world end-to-end probe: 西游记 first two chapters, jina-embeddings-v5
//! (L0) + jina-reranker-v3.5 (L1). Runs the full retrieval pipeline a user
//! actually cares about:
//!
//! 1. Read ch1/ch2 from `XYJ_CH1_PATH` / `XYJ_CH2_PATH`.
//! 2. Chunk each chapter into ~1024-char pieces (STMBE adaptive: chunk size is
//!    independent of the embedding model's ctx; here we force the short-ctx
//!    path even though jina v5 can swallow a full chapter).
//! 3. Embed every chunk via `TAURITAVERN_RECALL_EVAL_URL`.
//! 4. For each anchor query: embed → cosine → top-3 chunks → rerank via
//!    `TAURITAVERN_RERANK_EVAL_URL` → top-1 chunk → source doc → check.
//! 5. Print L0 top-1, L1 top-1, and the reranker lift per anchor.
//!
//! Endpoints required:
//! - L0 server: beellamacpp 0.4.7 (or newer) with `--embedding --pooling mean`
//! - L1 server: beellamacpp 0.4.7 (or newer) with `--reranking`
//!
//! ```text
//! $env:TAURITAVERN_RECALL_EVAL_URL = "http://127.0.0.1:8092"   # jina v5
//! $env:TAURITAVERN_RERANK_EVAL_URL = "http://127.0.0.1:8093"   # jina reranker v3.5
//! $env:TAURITAVERN_RECALL_EVAL_MODEL = "jina-embeddings-v5"
//! $env:TAURITAVERN_RERANK_EVAL_MODEL = "jina-reranker-v3"
//! cargo test -p tt-adapter-vector --test llama_cpp_epub_rerank_probe --release -- --ignored --nocapture
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

// 256 chars ≈ 340 BPE tokens; well under IKllamacpp's hard 512 ubatch ceiling
// for embeddings. Adjust per the embedding server's token cap.
const CHUNK_CHARS: usize = 256;

// Short keyword queries test the embedding's tokenizer robustness, not its
// semantic ranking. Reranker evaluation needs full sentences with disambiguating
// context — anything else just measures whether the models can rank tokens.
//
// Layout: chapter index → vec of (label, long_query, short_query, keyword).
// keyword is the short string we assert appears in this chapter and not in
// any of the others.
const ANCHORS: &[(usize, &str, &str, &str, &str)] = &[
    // ── 西游记 ch1 (idx 0) ──────────────────────────────
    (
        0,
        "xyj_ch1_仙石",
        "花果山顶有一块仙石，石头迸裂后化出了什么？",
        "仙石",
        "仙石",
    ),
    (
        0,
        "xyj_ch1_石卵",
        "仙石内部为何会化成石卵，石卵又化作何物？",
        "石卵",
        "石卵",
    ),
    (
        0,
        "xyj_ch1_灵根",
        "石猴出世后为何在花果山被称为灵根仙种？",
        "灵根",
        "灵根",
    ),
    (
        0,
        "xyj_ch1_目运",
        "石猴目运两道金光为何惊动天庭玉皇大帝？",
        "目运",
        "目运",
    ),
    // ── 西游记 ch2 (idx 1) ──────────────────────────────
    (
        1,
        "xyj_ch2_术字门",
        "菩提祖师传授的术字门功夫追求的是哪三种能力？",
        "术字门",
        "术字门",
    ),
    (
        1,
        "xyj_ch2_流字门",
        "菩提祖师传授的流字门功夫的核心要旨是什么？",
        "流字门",
        "流字门",
    ),
    (
        1,
        "xyj_ch2_静字门",
        "菩提祖师传授的静字门功夫讲究的是什么境界？",
        "静字门",
        "静字门",
    ),
    (
        1,
        "xyj_ch2_三更时分",
        "孙悟空是在哪个时辰从后门去找菩提祖师的？",
        "三更时分",
        "三更时分",
    ),
    (
        1,
        "xyj_ch2_戒尺",
        "菩提祖师用什么物件打了孙悟空的头三下？",
        "戒尺",
        "戒尺",
    ),
    // ── 西游记 ch3 (idx 2): 四海千山皆拱伏 ──────────
    (
        2,
        "xyj_ch3_十类",
        "美猴王下地府勾去了什么十类名籍？",
        "十类",
        "十类",
    ),
    (
        2,
        "xyj_ch3_四海千山",
        "美猴王从龙宫归来四海千山为何拱伏？",
        "四海千山",
        "四海千山",
    ),
    // ── 西游记 ch4 (idx 3): 官封弼马心何足 ──────────
    (
        3,
        "xyj_ch4_弼马",
        "玉帝最初给孙悟空封的官职叫什么名字？",
        "弼马",
        "弼马",
    ),
    (
        3,
        "xyj_ch4_蟠桃园",
        "孙悟空反下天庭之前偷偷去看了什么园子？",
        "蟠桃园",
        "蟠桃园",
    ),
    // ── 三国演义 ch1 (idx 4): 宴桃园豪杰三结义 ────────
    // 关键字「桃园」会被西游记 ch4 的「蟠桃园」污染；用「关羽」作为
    // 唯一 anchor 词。
    (
        4,
        "sgyy_ch1_关羽",
        "与刘备张飞在桃园结义的第三人是谁？",
        "关羽",
        "关羽",
    ),
    // ── 三国演义 ch2 (idx 5): 张翼德怒鞭督邮 ────────
    (
        5,
        "sgyy_ch2_督邮",
        "张飞怒鞭的督邮因何事前来巡察？",
        "督邮",
        "督邮",
    ),
    // ── 三国演义 ch3 (idx 6): 议温明董卓叱丁原 ──────
    // 关键字「赤兔」会被西游记 ch4 的天马列表污染；只用「李肃」+「丁原」。
    (
        6,
        "sgyy_ch3_李肃",
        "李肃用什么礼物说服吕布背叛丁原投靠董卓？",
        "李肃",
        "李肃",
    ),
    (
        6,
        "sgyy_ch3_丁原",
        "被吕布所杀而令其投靠董卓的旧主是谁？",
        "丁原",
        "丁原",
    ),
    // ── 三国演义 ch4 (idx 7): 废汉帝陈留践位 ────────
    (
        7,
        "sgyy_ch4_献刀",
        "曹操借献刀之名想刺杀谁却未能成功？",
        "献刀",
        "献刀",
    ),
    (
        7,
        "sgyy_ch4_七宝刀",
        "王允借给曹操刺杀董卓的宝刀叫什么？",
        "七宝刀",
        "七宝刀",
    ),
    (
        7,
        "sgyy_ch4_吕伯奢",
        "曹操多疑杀害的陈宫故交是谁？",
        "吕伯奢",
        "吕伯奢",
    ),
];

/// Read chapter paths, in order: XYJ ch1-4, then SGYY ch1-4.
/// Any chapter whose env var is unset is silently skipped (its anchors are
/// filtered out below).
fn load_chapters() -> Vec<(String, String)> {
    let mut out = Vec::new();
    // 西游记 ch1-4 (idx 0-3)
    for i in 1..=4 {
        let key = format!("XYJ_CH{i}_PATH");
        match std::env::var(&key).ok() {
            None => {} // skip silently
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(s) => out.push((format!("xyj_ch{i}"), s)),
                Err(e) => panic!("{key} ({p}): {e}"),
            },
        }
    }
    // 三国演义 ch1-4 (idx 4-7 when xyj 4 + sgyy 4 are present)
    for i in 1..=4 {
        let key = format!("SGYY_CH{i}_PATH");
        match std::env::var(&key).ok() {
            None => {}
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(s) => out.push((format!("sgyy_ch{i}"), s)),
                Err(e) => panic!("{key} ({p}): {e}"),
            },
        }
    }
    assert!(!out.is_empty(), "at least one chapter must be loaded");
    out
}

fn http_post(base_url: &str, path: &str, body: &str) -> Result<String, String> {
    let address = base_url
        .trim()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let (host, port) = address
        .split_once(':')
        .ok_or_else(|| format!("expected host:port in `{base_url}`"))?;
    let port: u16 = port.parse().map_err(|error| format!("bad port: {error}"))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\n\
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
    Ok(body)
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

fn embed(base_url: &str, inputs: &[String]) -> Result<(Vec<Vec<f32>>, u64), String> {
    let body = serde_json::json!({ "input": inputs, "model": "local" }).to_string();
    let resp = http_post(base_url, "/v1/embeddings", &body)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&resp).map_err(|error| format!("bad JSON: {error}"))?;
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

fn rerank(
    base_url: &str,
    model: &str,
    query: &str,
    documents: &[String],
) -> Result<Vec<(usize, f64)>, String> {
    let body = serde_json::json!({
        "model": model,
        "query": query,
        "documents": documents,
    })
    .to_string();
    let resp = http_post(base_url, "/v1/rerank", &body)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&resp).map_err(|error| format!("bad JSON: {error}"))?;
    let results = parsed["results"]
        .as_array()
        .ok_or_else(|| "rerank response has no results array".to_string())?;
    let mut out: Vec<(usize, f64)> = results
        .iter()
        .map(|entry| {
            let index = entry["index"].as_u64().unwrap_or(0) as usize;
            let score = entry["relevance_score"].as_f64().unwrap_or_default();
            (index, score)
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Ok(out)
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

fn cosine(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum()
}

/// Chinese-friendly tokenizer: 2-char sliding window. Each character also stands
/// alone so single-char concepts still match. We don't bother with stop words;
/// 56 chunks is small enough that noise is harmless.
fn bi_tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut tokens: Vec<String> = chars.iter().map(|c| c.to_string()).collect();
    for window in chars.windows(2) {
        tokens.push(window.iter().collect());
    }
    tokens
}

/// BM25 over the chunks, returned as (idx, score) sorted by score descending.
/// `k1` and `b` are the classical Okapi BM25 hyperparameters; the defaults
/// (k1=1.5, b=0.75) work well for short Chinese paragraphs.
fn bm25_rank(query: &str, docs: &[String]) -> Vec<(usize, f64)> {
    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let query_tokens = bi_tokenize(query);
    if query_tokens.is_empty() {
        return docs.iter().enumerate().map(|(i, _)| (i, 0.0)).collect();
    }

    let doc_tokens: Vec<Vec<String>> = docs.iter().map(|d| bi_tokenize(d)).collect();
    let avg_dl = if doc_tokens.is_empty() {
        1.0
    } else {
        doc_tokens.iter().map(|t| t.len() as f64).sum::<f64>() / doc_tokens.len() as f64
    };

    // Document frequencies: how many docs contain each unique token.
    use std::collections::HashMap;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for tokens in &doc_tokens {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in tokens {
            seen.insert(t.as_str());
        }
        for t in seen {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n = doc_tokens.len() as f64;

    let mut scores: Vec<(usize, f64)> = Vec::with_capacity(docs.len());
    for (i, tokens) in doc_tokens.iter().enumerate() {
        // Term frequencies within this doc.
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in tokens {
            *tf.entry(t.as_str()).or_insert(0) += 1;
        }
        let dl = tokens.len() as f64;
        let mut score = 0.0;
        for qt in &query_tokens {
            let f = *tf.get(qt.as_str()).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            let d_f = *df.get(qt.as_str()).unwrap_or(&0) as f64;
            let idf = ((n - d_f + 0.5) / (d_f + 0.5) + 1.0).ln();
            let norm = f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * dl / avg_dl));
            score += idf * norm;
        }
        scores.push((i, score));
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores
}

/// TF-IDF cosine over the same bi-gram tokenization. Returns (idx, score)
/// sorted descending. Useful when you want a normalized 0..1 lexical score
/// to combine with an embedding cosine.
fn tfidf_cosine_rank(query: &str, docs: &[String]) -> Vec<(usize, f64)> {
    use std::collections::HashMap;
    let doc_tokens: Vec<Vec<String>> = docs.iter().map(|d| bi_tokenize(d)).collect();

    let mut df: HashMap<&str, usize> = HashMap::new();
    for tokens in &doc_tokens {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in tokens {
            seen.insert(t.as_str());
        }
        for t in seen {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let n = doc_tokens.len() as f64;

    let to_tfidf = |tokens: &[String]| -> HashMap<String, f64> {
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in tokens {
            *tf.entry(t.as_str()).or_insert(0) += 1;
        }
        let mut out: HashMap<String, f64> = HashMap::new();
        for (t, count) in tf {
            let d_f = *df.get(t).unwrap_or(&0) as f64;
            let idf = ((n - d_f + 0.5) / (d_f + 0.5) + 1.0).ln();
            out.insert(t.to_string(), (count as f64) * idf);
        }
        out
    };

    let query_vec = to_tfidf(&bi_tokenize(query));
    let doc_vecs: Vec<HashMap<String, f64>> = doc_tokens.iter().map(|t| to_tfidf(t)).collect();

    let dot_norm = |a: &HashMap<String, f64>, b: &HashMap<String, f64>| -> f64 {
        let mut dot = 0.0;
        for (k, v) in a {
            if let Some(vb) = b.get(k) {
                dot += v * vb;
            }
        }
        let na: f64 = a.values().map(|x| x * x).sum::<f64>().sqrt();
        let nb: f64 = b.values().map(|x| x * x).sum::<f64>().sqrt();
        if na > 0.0 && nb > 0.0 {
            dot / (na * nb)
        } else {
            0.0
        }
    };

    let mut scores: Vec<(usize, f64)> = doc_vecs
        .iter()
        .enumerate()
        .map(|(i, v)| (i, dot_norm(&query_vec, v)))
        .collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores
}

/// Reciprocal Rank Fusion over an arbitrary set of rankings. Each input is a
/// slice of (idx, _) already sorted by score descending. `k` is the classical
/// smoothing constant (default 60). Items that appear in more rankings get
/// higher fused scores; absolute score values are discarded.
fn rrf_fuse(rankings: &[&[(usize, f64)]], k: usize) -> Vec<(usize, f64)> {
    use std::collections::HashMap;
    let mut fused: HashMap<usize, f64> = HashMap::new();
    for ranking in rankings {
        for (rank, (idx, _)) in ranking.iter().enumerate() {
            *fused.entry(*idx).or_insert(0.0) += 1.0 / (k as f64 + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(usize, f64)> = fused.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

#[tokio::test]
#[ignore = "needs two llama.cpp servers: jina-embeddings-v5 + jina-reranker-v3.5"]
async fn probe_epub_l0_l1_pipeline() {
    let embed_url = std::env::var("TAURITAVERN_RECALL_EVAL_URL")
        .expect("TAURITAVERN_RECALL_EVAL_URL must point at the embedding server");
    let embed_model = std::env::var("TAURITAVERN_RECALL_EVAL_MODEL")
        .unwrap_or_else(|_| "jina-embeddings-v5".to_string());
    let rerank_url = std::env::var("TAURITAVERN_RERANK_EVAL_URL").ok();
    let rerank_model = std::env::var("TAURITAVERN_RERANK_EVAL_MODEL")
        .unwrap_or_else(|_| "jina-reranker-v3".to_string());

    let chapters = load_chapters();
    let n_docs = chapters.len();

    // STMBE-style adaptive chunking at CHUNK_CHARS chars/chunk (independent of
    // the model's ctx — even though jina v5 could swallow a full chapter).
    let chunk_doc = |doc: &str| -> Vec<String> {
        let chars: Vec<char> = doc.chars().collect();
        chars
            .chunks(CHUNK_CHARS)
            .map(|c| c.iter().collect())
            .collect()
    };
    let mut chunks: Vec<(usize, String)> = Vec::new();
    for (doc_idx, (_, text)) in chapters.iter().enumerate() {
        for c in chunk_doc(text) {
            chunks.push((doc_idx, c));
        }
    }

    println!("\n=== EPUB (multi-book L0 → L1 pipeline probe) ===");
    println!("L0 (embed)   : {embed_url}  model={embed_model}");
    println!(
        "L1 (rerank)  : {}  model={rerank_model}",
        rerank_url.as_deref().unwrap_or("(disabled)")
    );
    println!("chapters     : {} (xyj=2 + sgyy={})", n_docs, n_docs - 2);
    println!(
        "chunking     : {} chars/chunk, total={} chunks (xyj={}+{}, sgyy={})",
        CHUNK_CHARS,
        chunks.len(),
        chapters[0].1.chars().count() / CHUNK_CHARS + 1,
        chapters[1].1.chars().count() / CHUNK_CHARS + 1,
        chunks.len() - chapters[0].1.chars().count() / CHUNK_CHARS - chapters[1].1.chars().count() / CHUNK_CHARS
    );

    // Verify anchors are unique to their chapter. Each anchor's keyword must
    // appear in its expected chapter and NOT in any other chapter.
    let query_mode = std::env::var("TAURITAVERN_RECALL_EVAL_QUERY_MODE")
        .unwrap_or_else(|_| "long".to_string());
    println!("query mode   : {query_mode}  (set TAURITAVERN_RECALL_EVAL_QUERY_MODE=short|long)");

    // Filter anchors whose chapter exists.
    let active_anchors: Vec<&(usize, &str, &str, &str, &str)> = ANCHORS
        .iter()
        .filter(|(idx, _, _, _, _)| *idx < n_docs)
        .collect();
    for (idx, label, _long, _short, keyword) in &active_anchors {
        let expected_text = &chapters[*idx].1;
        let others: Vec<usize> = (0..n_docs).filter(|i| i != idx).collect();
        assert!(
            expected_text.contains(keyword),
            "anchor keyword `{keyword}` ({label}) must appear in chapter {idx}"
        );
        for other in others {
            let other_text = &chapters[other].1;
            assert!(
                !other_text.contains(keyword),
                "anchor keyword `{keyword}` ({label}) leaks into chapter {other}"
            );
        }
    }

    // Embed chunks.
    const BATCH: usize = 4;
    let mut chunk_vecs: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    let mut total_embed_tokens = 0_u64;
    let started = Instant::now();
    for batch in chunks.chunks(BATCH) {
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let (vecs, tokens) = embed(&embed_url, &texts).expect("chunk embed failed");
        total_embed_tokens += tokens;
        for v in vecs {
            chunk_vecs.push(normalize(v));
        }
    }
    println!(
        "embedded     : {} chunks in {:.1} s ({total_embed_tokens} prompt tokens)",
        chunk_vecs.len(),
        started.elapsed().as_secs_f64()
    );

    let mut queries: Vec<(usize, String, String)> = Vec::new();
    for (idx, label, long_q, short_q, _keyword) in &active_anchors {
        let q = if query_mode == "short" {
            short_q.to_string()
        } else {
            long_q.to_string()
        };
        let _ = label; // (label used in print below)
        queries.push((*idx, label.to_string(), q));
    }
    let query_texts: Vec<String> = queries.iter().map(|(_, _, t)| t.clone()).collect();
    let (q_vecs, q_tokens) = embed(&embed_url, &query_texts).expect("query embed");
    let q_vecs: Vec<Vec<f32>> = q_vecs.into_iter().map(normalize).collect();

    let doc_ids: Vec<usize> = chunks.iter().map(|(d, _)| *d).collect();

    println!(
        "\nper-anchor outcome: three rankings compared — L0 (cosine top-1), \
         L1-over-L0-topK (rerank only the top-K chunks L0 handed it), and \
         L1-over-all (rerank every chunk from scratch — no L0 filter).{}",
        if rerank_url.is_none() {
            "  [TAURITAVERN_RERANK_EVAL_URL unset → L1 columns will be skipped]"
        } else {
            ""
        }
    );
    let run_rerank = rerank_url.is_some();
    let rerank_url_ref = rerank_url.as_deref().unwrap_or("");
    let mut l0_correct = 0_usize;
    let mut l1_k_correct = 0_usize;
    let mut l1_all_correct = 0_usize;
    let mut l1_k_helps = 0_usize;
    let mut bm25_top_k_correct = 0_usize;
    let mut bm25_top_k_helps = 0_usize;
    let mut tfidf_top_k_correct = 0_usize;
    let mut tfidf_top_k_helps = 0_usize;
    let mut bm25_all_correct = 0_usize;
    let mut rrf_correct = 0_usize;
    let mut l0_in_top_k = 0_usize;
    const RERANK_K: usize = 5;
    for (i, (expected, _label, q)) in queries.iter().enumerate() {
        let qv = &q_vecs[i];
        let mut scored: Vec<(usize, f64)> = chunk_vecs
            .iter()
            .enumerate()
            .map(|(j, v)| (j, cosine(qv, v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let l0_top_idx = scored[0].0;
        let l0_top_doc = doc_ids[l0_top_idx];
        let l0_ok = l0_top_doc == *expected;

        let top_k: Vec<usize> = scored.iter().take(RERANK_K).map(|(j, _)| *j).collect();
        let top_k_right = top_k
            .iter()
            .filter(|j| doc_ids[**j] == *expected)
            .count();
        if top_k_right > 0 {
            l0_in_top_k += 1;
        }
        if l0_ok {
            l0_correct += 1;
        }

        // ===== Algorithmic rerankers (no LLM, no world-knowledge priors) =====
        // BM25 and TF-IDF cosine applied to the top-K chunks L0 gave us.
        let top_k_docs: Vec<String> = top_k.iter().map(|j| chunks[*j].1.clone()).collect();
        let bm25_top_k = bm25_rank(q, &top_k_docs);
        let bm25_top_k_idx = top_k[bm25_top_k[0].0];
        let bm25_top_k_doc = doc_ids[bm25_top_k_idx];
        let bm25_top_k_ok = bm25_top_k_doc == *expected;
        if bm25_top_k_ok {
            bm25_top_k_correct += 1;
        }
        if !l0_ok && bm25_top_k_ok {
            bm25_top_k_helps += 1;
        }

        let tfidf_top_k = tfidf_cosine_rank(q, &top_k_docs);
        let tfidf_top_k_idx = top_k[tfidf_top_k[0].0];
        let tfidf_top_k_doc = doc_ids[tfidf_top_k_idx];
        let tfidf_top_k_ok = tfidf_top_k_doc == *expected;
        if tfidf_top_k_ok {
            tfidf_top_k_correct += 1;
        }
        if !l0_ok && tfidf_top_k_ok {
            tfidf_top_k_helps += 1;
        }

        // BM25 over all 56 chunks — pure lexical baseline (does BM25 alone
        // beat cosine alone?).
        let all_docs: Vec<String> = chunks.iter().map(|(_, t)| t.clone()).collect();
        let bm25_all = bm25_rank(q, &all_docs);
        let bm25_all_idx = bm25_all[0].0;
        let bm25_all_doc = doc_ids[bm25_all_idx];
        let bm25_all_ok = bm25_all_doc == *expected;
        if bm25_all_ok {
            bm25_all_correct += 1;
        }

        // RRF fusion of L0 cosine (full corpus) and BM25 (full corpus).
        let rrf = rrf_fuse(&[&scored, &bm25_all], 60);
        let rrf_top_doc = doc_ids[rrf[0].0];
        let rrf_ok = rrf_top_doc == *expected;
        if rrf_ok {
            rrf_correct += 1;
        }

        // ===== LLM rerankers (optional) =====
        let l1_k_top_doc;
        let l1_all_top_doc;
        let reranked_top_k_score;
        let reranked_all_score;
        if run_rerank {
            let reranked_top_k = rerank(rerank_url_ref, &rerank_model, q, &top_k_docs)
                .expect("rerank top-K failed");
            let l1_k_top_idx = top_k[reranked_top_k[0].0];
            l1_k_top_doc = doc_ids[l1_k_top_idx];
            reranked_top_k_score = reranked_top_k[0].1;
            if l1_k_top_doc == *expected {
                l1_k_correct += 1;
            }
            if !l0_ok && l1_k_top_doc == *expected {
                l1_k_helps += 1;
            }

            let reranked_all = rerank(rerank_url_ref, &rerank_model, q, &all_docs)
                .expect("rerank all failed");
            let l1_all_top_idx = reranked_all[0].0;
            l1_all_top_doc = doc_ids[l1_all_top_idx];
            reranked_all_score = reranked_all[0].1;
            if l1_all_top_doc == *expected {
                l1_all_correct += 1;
            }
        } else {
            l1_k_top_doc = usize::MAX;
            l1_all_top_doc = usize::MAX;
            reranked_top_k_score = 0.0;
            reranked_all_score = 0.0;
        }

        let expect_doc_label = chapters[*expected].0.clone();
        let l0_got = chapters[l0_top_doc].0.clone();
        let bm25_got = chapters[bm25_top_k_doc].0.clone();
        let tfidf_got = chapters[tfidf_top_k_doc].0.clone();
        let bm25_all_got = chapters[bm25_all_doc].0.clone();
        let rrf_got = chapters[rrf_top_doc].0.clone();
        let short_q: String = q.chars().take(28).collect();
        if run_rerank {
            let l1_k_got = chapters[l1_k_top_doc].0.clone();
            let l1_all_got = chapters[l1_all_top_doc].0.clone();
            println!(
                "  Q={short_q:<28}  expect={expect_doc_label}  L0={l0_got}({:+.4})  \
                 BM25@topK={bm25_got}({:+.2})  TFIDF@topK={tfidf_got}({:+.3})  \
                 BM25@all={bm25_all_got}({:+.2})  RRF={rrf_got}({:+.3})  \
                 LLM@topK={l1_k_got}({:+.2e})  LLM@all={l1_all_got}({:+.2e})",
                scored[0].1,
                bm25_top_k[0].1,
                tfidf_top_k[0].1,
                bm25_all[0].1,
                rrf[0].1,
                reranked_top_k_score,
                reranked_all_score
            );
        } else {
            println!(
                "  Q={short_q:<28}  expect={expect_doc_label}  L0={l0_got}({:+.4})  \
                 BM25@topK={bm25_got}({:+.2})  TFIDF@topK={tfidf_got}({:+.3})  \
                 BM25@all={bm25_all_got}({:+.2})  RRF={rrf_got}({:+.3})",
                scored[0].1, bm25_top_k[0].1, tfidf_top_k[0].1, bm25_all[0].1, rrf[0].1
            );
        }
    }

    let total = queries.len();
    println!(
        "\nL0 top-1       : {l0_correct}/{total}    L0 top-{RERANK_K} contains answer : \
         {l0_in_top_k}/{total}"
    );
    println!(
        "BM25 @topK     : {bm25_top_k_correct}/{total}    rescued L0 misses: \
         {bm25_top_k_helps}    | TFIDF @topK : {tfidf_top_k_correct}/{total}    \
         rescued: {tfidf_top_k_helps}"
    );
    println!("BM25 @all      : {bm25_all_correct}/{total}    RRF(cosine+BM25)@all : {rrf_correct}/{total}");
    if run_rerank {
        println!(
            "LLM  @topK     : {l1_k_correct}/{total}    rescued L0 misses: {l1_k_helps}    \
             | LLM  @all  : {l1_all_correct}/{total}"
        );
    } else {
        println!("LLM reranker   : disabled (set TAURITAVERN_RERANK_EVAL_URL to enable)");
    }
    println!("prompt tokens (queries): {q_tokens}");

    assert_eq!(chunk_vecs.len(), chunks.len(), "every chunk must have an embedding");
}