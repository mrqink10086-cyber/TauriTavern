//! A manual probe: R1's index path against a local llama.cpp embedding server.
//!
//! This is not a regression test. It needs a running server and a downloaded
//! model, so it is `#[ignore]`d and only run on purpose, to produce the numbers
//! the plan asks for before any of them are designed around: the model's
//! dimension, whether template-shaped fact sentences crowd together enough to
//! need centering, and what an exact scan over a realistic corpus actually
//! costs.
//!
//! Everything it measures is the real implementation — the same fact rendering,
//! the same scope naming rules, the same redb repository, the same exact scan.
//! Only the corpus is synthesized, because this machine has no chat with a
//! published state history; the values are drawn from the plan's own examples.
//!
//! ```text
//! TAURITAVERN_RECALL_EVAL_URL=http://127.0.0.1:8090 \
//!   cargo test -p tt-adapter-vector --test llama_cpp_recall_probe -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tt_adapter_vector::RedbVectorRepository;
use tt_domain::models::recall_fact::{RecallFact, render_fact_sentences, state_version_index};
use tt_domain::models::state::{DeclaredStateField, StateDeclaration, StateDocument, StateField};
use tt_domain::models::state_access::StateFieldAccess;
use tt_domain::models::state_key::{StateKey, StateKeyPattern};
use tt_ports::repositories::recall_repository::{
    RecallRepository, RecallScopeMeta, StateFloorBinding,
};
use tt_ports::repositories::vector_repository::{
    VectorMetadata, VectorRecord, VectorRepository, VectorScope,
};

/// Texts per embedding request. Kept small because the model's own context is
/// bounded, and a batch is what the server pads and pools together.
const BATCH: usize = 32;
/// How many floors of a plausible chat the corpus covers.
const FLOORS: i64 = 320;
/// Extra records the exact scan is timed with, on top of the real corpus. The
/// padding is copies: the cost of a dot product does not depend on the values,
/// and embedding 20k texts on a CPU would take hours.
const SCAN_PADDING: [usize; 3] = [0, 5_000, 20_000];
/// Pairs sampled for the mean cosine similarity.
const MCS_PAIRS: usize = 500;

/// Which model the running llama.cpp server loaded. The probe only talks to the
/// server over HTTP, so the caller names the model for the printed profile.
fn eval_model_label() -> String {
    std::env::var("TAURITAVERN_RECALL_EVAL_MODEL")
        .unwrap_or_else(|_| "yuan-embedding-2.0-zh".to_string())
}

/// The server's context window in tokens. Used only for the truncation warning:
/// a fact sentence longer than this would be silently cut by the model.
fn eval_ctx() -> u64 {
    std::env::var("TAURITAVERN_RECALL_EVAL_CTX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(512)
}

const PLACES: [&str; 12] = [
    "咖啡馆",
    "图书馆",
    "车站",
    "教室",
    "公园",
    "厨房",
    "屋顶",
    "走廊",
    "书店",
    "码头",
    "医院",
    "天台",
];
const TIMES: [&str; 6] = ["清晨", "上午", "正午", "下午", "傍晚", "深夜"];
const OUTFITS: [&str; 10] = [
    "风衣", "校服", "针织衫", "睡袍", "雨衣", "礼服", "夹克", "衬衫", "毛衣", "运动服",
];
const ITEMS: [&str; 8] = ["旧钥匙", "钢笔", "怀表", "雨伞", "手电", "相册", "口琴", "地图"];
const STAGES: [&str; 5] = ["陌生", "试探", "熟悉", "信任", "默契"];
const WEATHER: [&str; 8] = [
    "晴", "多云", "小雨", "大雨", "雾", "雪", "闷热", "微风",
];

/// The declaration a chat like this would actually have: a label per field, so
/// the rendered sentence carries both the label and the key path.
fn declaration() -> StateDeclaration {
    let entries = [
        ("环境/日期", "日期"),
        ("环境/时间", "时间"),
        ("环境/地点", "地点"),
        ("天气/描述", "天气"),
        ("角色/林/好感度", "林的好感度"),
        ("角色/林/着装", "林的着装"),
        ("角色/林/持有物", "林携带的物品"),
        ("关系/阶段", "关系阶段"),
        ("任务/主线/目标", "当前目标"),
        ("任务/主线/进展", "任务进展"),
        ("时间线/幕次/标签", "幕次"),
        ("装备/主手/附魔", "主手附魔"),
    ];
    StateDeclaration {
        fields: entries
            .iter()
            .map(|(pattern, label)| DeclaredStateField {
                pattern: StateKeyPattern::parse(pattern)
                    .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
                label: label.to_string(),
                access: StateFieldAccess::DECLARED,
                initial: Vec::new(),
            })
            .collect(),
        ..Default::default()
    }
}

fn key(raw: &str) -> StateKey {
    StateKey::parse(raw).unwrap_or_else(|error| panic!("`{raw}` must parse: {error}"))
}

fn field(raw: &str, value: String) -> StateField {
    StateField {
        key: key(raw),
        values: vec![value],
    }
}

/// One floor's state document: a few fields move every floor, the rest sit
/// still. Values are derived from the floor index so the corpus is
/// deterministic and re-runs produce identical numbers.
fn document_for_floor(floor: i64) -> StateDocument {
    let pick = |values: &[&str], index: i64| values[(index.rem_euclid(values.len() as i64)) as usize].to_string();
    StateDocument {
        fields: vec![
            field("环境/日期", format!("2026/09/{:02}", floor / 4 % 30 + 1)),
            field("环境/时间", pick(&TIMES, floor / 3)),
            field("环境/地点", pick(&PLACES, floor / 5)),
            field("天气/描述", pick(&WEATHER, floor)),
            field("角色/林/好感度", format!("{}", 40 + floor / 2)),
            field("角色/林/着装", pick(&OUTFITS, floor)),
            field("角色/林/持有物", pick(&ITEMS, floor / 2)),
            field("关系/阶段", pick(&STAGES, floor / 20)),
            field("任务/主线/目标", format!("第{}阶段的目标", floor / 40 + 1)),
            field("任务/主线/进展", format!("第{floor}步")),
            field("时间线/幕次/标签", format!("第{}幕", floor / 25 + 1)),
            field("装备/主手/附魔", pick(&ITEMS, floor / 7)),
        ],
    }
}

/// One indexed fact: what it was rendered from, and the vector the server gave.
struct EmbeddedFact {
    floor: i64,
    fact: RecallFact,
    vector: Vec<f32>,
}

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-recall-probe-{}",
            std::process::id()
        ));
        Self(root.join("index.redb"))
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        if let Some(root) = self.0.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// One OpenAI-compatible embeddings call, over a hand-written request: this is
/// a localhost probe and must not drag an HTTP client into the crate.
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
        return Err(format!("{status}: {}", body.chars().take(400).collect::<String>()));
    }
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
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
        let embedding = entry["embedding"]
            .as_array()
            .ok_or_else(|| "an entry has no embedding".to_string())?
            .iter()
            .map(|value| value.as_f64().unwrap_or_default() as f32)
            .collect::<Vec<f32>>();
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

/// The product normalizes before storing, so the probe does the same; without
/// it a dot product would not be a cosine.
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

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[tokio::test]
#[ignore = "needs a local llama.cpp embedding server; see the module docs"]
async fn probe_recall_index_with_a_local_embedding_server() {
    let base_url = std::env::var("TAURITAVERN_RECALL_EVAL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let profile = eval_model_label();
    let ctx = eval_ctx();
    let declaration = declaration();

    // --- the corpus ------------------------------------------------------
    let mut seen: HashSet<(String, i64)> = HashSet::new();
    let mut pending: Vec<(i64, RecallFact)> = Vec::new();
    let mut rendered = 0_usize;
    for floor in 0..FLOORS {
        for fact in render_fact_sentences(&document_for_floor(floor), &declaration, &profile) {
            rendered += 1;
            if seen.insert((fact.field_key.clone(), fact.hash)) {
                pending.push((floor, fact));
            }
        }
    }
    let longest = pending
        .iter()
        .max_by_key(|(_, fact)| fact.sentence.chars().count())
        .map(|(_, fact)| fact.sentence.clone())
        .expect("the corpus must not be empty");

    // --- embedding -------------------------------------------------------
    let mut embedded: Vec<EmbeddedFact> = Vec::new();
    let started = Instant::now();
    for chunk in pending.chunks(BATCH) {
        let texts = chunk
            .iter()
            .map(|(_, fact)| fact.sentence.clone())
            .collect::<Vec<_>>();
        let (vectors, _) = embed(&base_url, &texts).expect("embedding request failed");
        for ((floor, fact), vector) in chunk.iter().zip(vectors) {
            embedded.push(EmbeddedFact {
                floor: *floor,
                fact: fact.clone(),
                vector: normalize(vector),
            });
        }
    }
    let embed_elapsed = started.elapsed();
    let dims = embedded[0].vector.len();

    // A single call for the longest sentence tells us its token count, which is
    // what decides whether a 512-token context can hold our fact sentences.
    let (_, longest_tokens) = embed(&base_url, std::slice::from_ref(&longest))
        .expect("the longest sentence must embed");

    // --- storage: exactly what R1 does -----------------------------------
    let temp = TempDatabase::new();
    let repository = RedbVectorRepository::new(temp.0.clone());
    let scope = VectorScope {
        collection_id: "state-history:probe".to_string(),
        source: "state-history".to_string(),
        profile: profile.clone(),
    };
    let records = embedded
        .iter()
        .map(|item| VectorRecord {
            metadata: VectorMetadata {
                hash: item.fact.hash,
                text: item.fact.sentence.clone(),
                index: state_version_index(&format!("v{}", item.floor)),
                floor: None,
                field_key: Some(item.fact.field_key.clone()),
                kind: Some("fact".to_string()),
            },
            embedding: item.vector.clone(),
        })
        .collect::<Vec<_>>();
    repository
        .upsert(&scope, records)
        .await
        .expect("the corpus must store");
    let indexed = repository
        .list_metadata(&scope)
        .await
        .expect("stored records must be readable");
    repository
        .write_scope_meta(
            &scope,
            &RecallScopeMeta {
                dims: u32::try_from(dims).expect("a sane dimension"),
                model_profile: profile.clone(),
                doc_count: indexed.len() as u64,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .expect("scope metadata must store");

    // --- R1's invariants, with real vectors ------------------------------
    let bindings = embedded
        .iter()
        .map(|item| item.floor)
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|floor| StateFloorBinding {
            version_index: state_version_index(&format!("v{floor}")),
            floor,
        })
        .collect::<Vec<_>>();
    let bound = repository
        .bind_state_floors(&scope, &bindings)
        .await
        .expect("binding floors must succeed");
    let rebound = repository
        .bind_state_floors(&scope, &bindings)
        .await
        .expect("binding again must succeed");
    let replay = render_fact_sentences(&document_for_floor(FLOORS - 1), &declaration, &profile)
        .into_iter()
        .filter(|fact| {
            !indexed
                .iter()
                .any(|metadata| {
                    metadata.field_key.as_deref() == Some(fact.field_key.as_str())
                        && metadata.hash == fact.hash
                })
        })
        .count();

    // --- crowding --------------------------------------------------------
    let real = embedded.len();
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut pairs = 0_usize;
    let mut total = 0_f64;
    while pairs < MCS_PAIRS {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let left = (state >> 33) as usize % real;
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let right = (state >> 33) as usize % real;
        if left == right {
            continue;
        }
        total += f64::from(cosine(&embedded[left].vector, &embedded[right].vector));
        pairs += 1;
    }
    let mcs = total / pairs as f64;

    // --- exact scan ------------------------------------------------------
    let query = normalize(vec![0.01_f32; dims]);
    let mut scan_lines = Vec::new();
    let mut padding = 0_usize;
    for size in SCAN_PADDING {
        if size > 0 {
            let filler = normalize(
                (0..dims)
                    .map(|index| ((index as f32) * 0.7).sin())
                    .collect::<Vec<f32>>(),
            );
            let mut extra = Vec::new();
            for offset in padding..size {
                extra.push(VectorRecord {
                    metadata: VectorMetadata {
                        hash: -(offset as i64) - 1,
                        text: format!("padding {offset}"),
                        index: -1,
                        floor: None,
                        field_key: None,
                        kind: Some("probe-padding".to_string()),
                    },
                    embedding: filler.clone(),
                });
            }
            repository
                .upsert(&scope, extra)
                .await
                .expect("padding must store");
            padding = size;
        }

        let mut samples = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            let hits = repository
                .query(&scope, query.clone(), 10)
                .await
                .expect("query must succeed");
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(hits.len().min(10), hits.len(), "the scan returns at most the limit");
        }
        let total_records = real + padding;
        scan_lines.push(format!(
            "  n={total_records:<6} min {:>7.1} ms / median {:>7.1} ms / max {:>7.1} ms",
            samples.iter().copied().fold(f64::MAX, f64::min),
            median(samples.clone()),
            samples.iter().copied().fold(f64::MIN, f64::max),
        ));
    }

    println!("\n=== R1 probe: state-history index on a local llama.cpp model ===");
    println!("endpoint             : {base_url}");
    println!("embedding dims       : {dims}");
    println!(
        "longest sentence     : {} chars -> {longest_tokens} prompt tokens (model context {ctx})",
        longest.chars().count()
    );
    println!("floors rendered      : {FLOORS} ({rendered} facts, {} unique)", pending.len());
    println!(
        "embedded             : {} texts in {:.1} s ({:.0} texts/s, batch {BATCH})",
        embedded.len(),
        embed_elapsed.as_secs_f64(),
        embedded.len() as f64 / embed_elapsed.as_secs_f64()
    );
    println!("index records        : {}", indexed.len());
    println!("floors bound         : {bound} records, re-binding changed {rebound}");
    println!("diff on a replay     : {replay} facts would be embedded again");
    println!("MCS over {pairs} pairs  : {mcs:.4}");
    println!("exact scan (top-10):");
    for line in &scan_lines {
        println!("{line}");
    }
    if longest_tokens > ctx {
        println!(
            "WARNING              : the longest fact sentence is {longest_tokens} tokens, \
             past this model's {ctx}-token context — the server truncates it silently"
        );
    }
    println!();

    assert!(dims >= 64, "an embedding model must produce a usable dimension");
    assert_eq!(indexed.len(), real, "every fact must be indexed exactly once");
    assert_eq!(bound, real, "every indexed fact belongs to a floor");
    assert_eq!(rebound, 0, "re-binding must not rewrite anything");
    assert_eq!(replay, 0, "a replay must not re-embed what is already indexed");
}
