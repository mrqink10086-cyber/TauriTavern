//! Similharity compatibility bridge — pure request shaping.
//!
//! TauriTavern ships a bridge that lets the VectFox extension run against a
//! local Qdrant instance without the SillyTavern server plugin. Everything
//! Qdrant-facing is built here from the exact JSON shapes the plugin produces
//! (`similharity-plugin/qdrant-backend.js`), so the extension needs no changes.
//!
//! These functions are pure: the adapter only performs the HTTP call, and the
//! bridge only reshapes the engine's response.

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Version reported by `/health` and `/version`. The extension probes `/health`
/// for a version field to decide whether the plugin is usable, so this must stay
/// in step with the plugin contract rather than with the host version.
pub const PLUGIN_VERSION: &str = "3.3.4";
/// Collection holding chat events (EventBase) and chunks when multitenancy is on.
pub const MULTITENANCY_COLLECTION: &str = "vectfox_main";
/// Sentinel point id carrying per-collection metadata (CJK tokenizer lock, ...).
pub const SENTINEL_POINT_ID: &str = "00000000-0000-0000-0000-0000feedf00d";
/// Sentinel `type` payload value; excluded from every read path.
pub const SENTINEL_POINT_TYPE: &str = "_vectfox_meta";
/// Named sparse vector holding client-encoded BM25 terms (Qdrant applies IDF).
pub const SPARSE_VECTOR_KEY: &str = "text_sparse";
/// Qdrant's default (unnamed) dense vector is addressed as an empty name once a
/// named sparse vector exists in the same collection.
pub const DEFAULT_DENSE_VECTOR_KEY: &str = "";
pub const DEFAULT_FUSION: &str = "rrf";
pub const DEFAULT_PREFETCH_MULTIPLIER: u64 = 4;
const DEFAULT_IMPORTANCE: f64 = 100.0;

/// Collection prefixes the extension may attach to a collection id.
const COLLECTION_PREFIXES: [&str; 2] = ["qdrant:", "source:"];

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankWeights {
    #[serde(default = "default_cosine_weight")]
    pub cosine: f64,
    #[serde(default = "default_importance_weight")]
    pub importance: f64,
    #[serde(default = "default_persist_weight")]
    pub persist: f64,
    #[serde(default = "default_recency_weight")]
    pub recency: f64,
}

impl Default for RerankWeights {
    fn default() -> Self {
        Self {
            cosine: default_cosine_weight(),
            importance: default_importance_weight(),
            persist: default_persist_weight(),
            recency: default_recency_weight(),
        }
    }
}

/// `rerankParams` from `/chunks/hybrid-query-rerank`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankParams {
    #[serde(default)]
    pub weights: RerankWeights,
    #[serde(default)]
    pub chat_length: f64,
    #[serde(default = "default_half_life")]
    pub half_life: f64,
    #[serde(default = "default_min_importance")]
    pub min_importance: f64,
    #[serde(default = "default_visible_threshold")]
    pub visible_threshold: f64,
    #[serde(default = "default_true")]
    pub apply_context_dedup_filter: bool,
    #[serde(default = "default_rrf_score_scale")]
    pub rrf_score_scale: f64,
}

impl Default for RerankParams {
    fn default() -> Self {
        Self {
            weights: RerankWeights::default(),
            chat_length: 0.0,
            half_life: default_half_life(),
            min_importance: default_min_importance(),
            visible_threshold: default_visible_threshold(),
            apply_context_dedup_filter: true,
            rrf_score_scale: default_rrf_score_scale(),
        }
    }
}

fn default_cosine_weight() -> f64 {
    0.4
}

fn default_importance_weight() -> f64 {
    0.2
}

fn default_persist_weight() -> f64 {
    0.2
}

fn default_recency_weight() -> f64 {
    0.2
}

fn default_half_life() -> f64 {
    40.0
}

fn default_min_importance() -> f64 {
    1.0
}

fn default_visible_threshold() -> f64 {
    -1.0
}

fn default_rrf_score_scale() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

/// Tenant fields written on every point; they are what payload filters target.
#[derive(Debug, Clone, Default)]
pub struct TenantFields {
    pub content_type: String,
    pub source_id: String,
    pub embedding_source: String,
    pub embedding_model: String,
}

/// One `/chunks/insert` item, already resolved to a dense vector.
#[derive(Debug, Clone)]
pub struct PointInput {
    pub hash: i64,
    pub text: String,
    pub index: Option<i64>,
    pub metadata: Map<String, Value>,
    pub vector: Vec<f32>,
    pub sparse: Option<Value>,
    pub importance: Option<Value>,
    pub keywords: Option<Value>,
    pub conditions: Option<Value>,
    pub is_summary_chunk: Option<Value>,
    pub parent_hash: Option<Value>,
    pub tenant: TenantFields,
}

/// Qdrant filter for hybrid queries. Mirrors the plugin's `_buildHybridFilter`:
/// scalar/range filters are hard `must`, planner `*_any` fields are soft
/// `should`, and the sentinel point is excluded on every path.
pub fn build_filter(filters: &Map<String, Value>) -> Value {
    let mut must = Vec::new();
    let mut should = Vec::new();

    for field in [
        "type",
        "sourceId",
        "characterName",
        "chatId",
        "embeddingSource",
        "content_type",
    ] {
        if let Some(clause) = scalar_match(filters, field, field) {
            must.push(clause);
        }
    }
    if let Some(clause) = range_gte(filters, "minImportance", "importance") {
        must.push(clause);
    }
    if let Some(clause) = range_gte(filters, "timestampAfter", "timestamp") {
        must.push(clause);
    }
    if let Some(clause) = range_gte(filters, "importance_gte", "importance") {
        must.push(clause);
    }

    for (source, payload_key) in [
        ("characters_any", "characters"),
        ("locations_any", "locations"),
        ("factions_any", "factions"),
        ("concepts_any", "concepts"),
        ("items_any", "items"),
        ("event_type_any", "event_type"),
    ] {
        let values = filters
            .get(source)
            .and_then(Value::as_array)
            .filter(|values| !values.is_empty());
        if let Some(values) = values {
            should.push(json!({ "key": payload_key, "match": { "any": values } }));
        }
    }

    compose_filter(must, should, None)
}

/// `None` when the outer filter would carry nothing but the sentinel exclusion
/// and the caller asked for no dedup/importance constraints… which never
/// happens in practice: the sentinel exclusion is always worth sending.
fn compose_filter(must: Vec<Value>, should: Vec<Value>, must_not: Option<Vec<Value>>) -> Value {
    let mut out = Map::new();
    if !must.is_empty() {
        out.insert("must".to_string(), Value::Array(must));
    }
    let must_not = must_not.unwrap_or_else(|| {
        vec![json!({ "key": "type", "match": { "value": SENTINEL_POINT_TYPE } })]
    });
    out.insert("must_not".to_string(), Value::Array(must_not));
    if !should.is_empty() {
        out.insert("should".to_string(), Value::Array(should));
    }
    Value::Object(out)
}

fn scalar_match(
    filters: &Map<String, Value>,
    source_key: &str,
    payload_key: &str,
) -> Option<Value> {
    let text = filters.get(source_key).and_then(Value::as_str)?;
    if text.is_empty() {
        return None;
    }
    Some(json!({ "key": payload_key, "match": { "value": text } }))
}

fn range_gte(
    filters: &Map<String, Value>,
    source_key: &str,
    payload_key: &str,
) -> Option<Value> {
    let bound = filters.get(source_key).and_then(Value::as_f64)?;
    Some(json!({ "key": payload_key, "range": { "gte": bound } }))
}

/// Dense + sparse prefetch fused server-side. `using` is omitted for the dense
/// leg: Qdrant expects the default vector to be addressed by absence, not `""`.
pub fn build_hybrid_query_body(
    dense: &[f32],
    sparse: &Value,
    top_k: u64,
    prefetch_limit: Option<u64>,
    fusion: Option<&str>,
    filters: &Map<String, Value>,
) -> Value {
    let prefetch_limit = prefetch_limit.unwrap_or(top_k * DEFAULT_PREFETCH_MULTIPLIER);
    let filter = build_filter(filters);

    json!({
        "prefetch": [
            { "query": dense, "limit": prefetch_limit, "filter": filter },
            {
                "query": sparse,
                "using": SPARSE_VECTOR_KEY,
                "limit": prefetch_limit,
                "filter": filter,
            },
        ],
        "query": { "fusion": fusion.unwrap_or(DEFAULT_FUSION) },
        "limit": top_k,
        "with_payload": true,
    })
}

/// Same hybrid retrieval, wrapped in one outer formula query that computes the
/// EventBase weighted score inside Qdrant (RRF fused score, importance, persist
/// flag, floor-distance decay). Requires Qdrant 1.13+.
pub fn build_hybrid_rerank_query_body(
    dense: &[f32],
    sparse: &Value,
    top_k: u64,
    prefetch_limit: Option<u64>,
    params: &RerankParams,
    filters: &Map<String, Value>,
) -> Value {
    let prefetch_limit = prefetch_limit.unwrap_or(top_k * DEFAULT_PREFETCH_MULTIPLIER);
    let hybrid_limit = top_k.max(prefetch_limit / 2);
    let tenant_filter = build_filter(filters);

    let mut outer_must = Vec::new();
    if params.min_importance > 0.0 {
        outer_must.push(json!({ "key": "importance", "range": { "gte": params.min_importance } }));
    }
    if params.apply_context_dedup_filter && params.visible_threshold >= 0.0 {
        outer_must.push(json!({
            "key": "source_window_end",
            "range": { "lt": params.visible_threshold },
        }));
    }
    if let Some(must) = tenant_filter.get("must").and_then(Value::as_array) {
        outer_must.extend(must.iter().cloned());
    }
    let must_not = tenant_filter
        .get("must_not")
        .and_then(Value::as_array)
        .cloned();

    let mut body = json!({
        "prefetch": [{
            "prefetch": [
                { "query": dense, "limit": prefetch_limit, "filter": tenant_filter },
                {
                    "query": sparse,
                    "using": SPARSE_VECTOR_KEY,
                    "limit": prefetch_limit,
                    "filter": tenant_filter,
                },
            ],
            "query": { "fusion": DEFAULT_FUSION },
            "limit": hybrid_limit,
        }],
        "query": { "formula": build_rerank_formula(params) },
        "limit": top_k,
        "with_payload": true,
    });

    if !outer_must.is_empty() || must_not.is_some() {
        let outer = compose_filter(outer_must, Vec::new(), must_not);
        body["filter"] = outer;
    }

    body
}

/// Formula expression grammar mirrors `qdrant-backend.js:1081-1097` exactly:
/// `div` is a structured object (not an array), a bare field condition is itself
/// an expression returning 1/0, and decay uses `x`/`target`/`scale`/`midpoint`.
pub fn build_rerank_formula(params: &RerankParams) -> Value {
    let weights = params.weights;
    json!({
        "sum": [
            { "mult": [weights.cosine, { "mult": [params.rrf_score_scale, "$score"] }] },
            { "mult": [weights.importance, { "div": { "left": "importance", "right": 10 } }] },
            {
                "mult": [
                    weights.persist,
                    { "key": "should_persist", "match": { "value": true } },
                ],
            },
            {
                "mult": [
                    weights.recency,
                    {
                        "exp_decay": {
                            "x": "source_window_end",
                            "target": params.chat_length,
                            "scale": params.half_life,
                            "midpoint": 0.5,
                        },
                    },
                ],
            },
        ],
    })
}

/// Build one Qdrant point. Metadata is spread first so the core fields below
/// override it — the same precedence the plugin applies.
pub fn build_point(input: &PointInput, native_sparse: bool) -> Value {
    let mut payload = input.metadata.clone();

    payload.insert("text".to_string(), Value::String(input.text.clone()));
    payload.insert("hash".to_string(), json!(input.hash));
    payload.insert(
        "type".to_string(),
        Value::String(non_empty(&input.tenant.content_type, "chat")),
    );
    payload.insert(
        "sourceId".to_string(),
        Value::String(non_empty(&input.tenant.source_id, "unknown")),
    );
    payload.insert(
        "embeddingSource".to_string(),
        Value::String(non_empty(&input.tenant.embedding_source, "transformers")),
    );
    payload.insert(
        "embeddingModel".to_string(),
        Value::String(input.tenant.embedding_model.clone()),
    );
    if !payload.contains_key("timestamp") {
        payload.insert("timestamp".to_string(), json!(Utc::now().timestamp_millis()));
    }
    let message_index = input
        .index
        .or_else(|| payload.get("messageIndex").and_then(Value::as_i64));
    payload.insert(
        "messageIndex".to_string(),
        message_index.map_or(Value::Null, |index| json!(index)),
    );
    payload.insert(
        "importance".to_string(),
        input.importance.clone().unwrap_or_else(|| {
            payload
                .get("importance")
                .cloned()
                .unwrap_or_else(|| json!(DEFAULT_IMPORTANCE))
        }),
    );
    payload.insert(
        "keywords".to_string(),
        input
            .keywords
            .clone()
            .or_else(|| payload.get("keywords").cloned())
            .unwrap_or_else(|| json!([])),
    );
    payload.insert(
        "conditions".to_string(),
        input
            .conditions
            .clone()
            .or_else(|| payload.get("conditions").cloned())
            .unwrap_or(Value::Null),
    );
    payload.insert(
        "isSummaryChunk".to_string(),
        input
            .is_summary_chunk
            .clone()
            .or_else(|| payload.get("isSummaryChunk").cloned())
            .unwrap_or(Value::Bool(false)),
    );
    payload.insert(
        "parentHash".to_string(),
        input
            .parent_hash
            .clone()
            .or_else(|| payload.get("parentHash").cloned())
            .unwrap_or(Value::Null),
    );

    let vector = if native_sparse {
        if let Some(sparse) = &input.sparse {
            json!({ DEFAULT_DENSE_VECTOR_KEY: input.vector, SPARSE_VECTOR_KEY: sparse })
        } else {
            json!(input.vector)
        }
    } else {
        json!(input.vector)
    };

    json!({ "id": input.hash, "vector": vector, "payload": Value::Object(payload) })
}

/// Payload indexes the EventBase paths filter and score on.
pub fn eventbase_payload_indexes() -> Vec<(&'static str, Value)> {
    vec![
        ("type", json!({ "type": "keyword", "is_tenant": true })),
        ("sourceId", json!({ "type": "keyword", "is_tenant": true })),
        ("embeddingSource", json!("keyword")),
        ("embeddingModel", json!("keyword")),
        ("characterName", json!("keyword")),
        ("chatId", json!("keyword")),
        ("keywords", json!("keyword")),
        ("text", json!("text")),
        ("hash", json!("integer")),
        ("timestamp", json!("integer")),
        ("importance", json!("integer")),
        ("source_window_end", json!("integer")),
        ("timeline_sort_key", json!("integer")),
        ("should_persist", json!("bool")),
        ("characters", json!("keyword")),
        ("locations", json!("keyword")),
        ("factions", json!("keyword")),
        ("concepts", json!("keyword")),
        ("items", json!("keyword")),
        ("event_type", json!("keyword")),
    ]
}

/// Strip the registry prefixes the extension may attach to a collection id.
pub fn normalize_collection_name(name: &str) -> String {
    let mut value = name.trim();
    loop {
        let mut stripped = false;
        for prefix in COLLECTION_PREFIXES {
            if let Some(rest) = value.strip_prefix(prefix) {
                value = rest;
                stripped = true;
            }
        }
        if !stripped {
            return value.to_string();
        }
    }
}

/// Derive the `source` a collection belongs to, using the same rules as the
/// plugin's collection scan (`backend:source:id` → `source`, `source:id` → first
/// segment, otherwise unknown).
pub fn parse_collection_source(name: &str) -> Option<(String, String)> {
    let normalized = name.trim().trim_matches('/');
    let parts = normalized.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [backend, source, id] if !backend.is_empty() && !source.is_empty() && !id.is_empty() => {
            Some(((*source).to_string(), (*id).to_string()))
        }
        [source, id] if !source.is_empty() && !id.is_empty() => {
            Some(((*source).to_string(), (*id).to_string()))
        }
        _ => None,
    }
}

fn non_empty(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters(entries: &[(&str, Value)]) -> Map<String, Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn filter_always_excludes_the_sentinel_and_keeps_scalar_filters_hard() {
        let filter = build_filter(&filters(&[
            ("type", json!("chat")),
            ("sourceId", json!("abc")),
            ("minImportance", json!(3)),
        ]));

        let must = filter["must"].as_array().expect("must clauses");
        assert!(must.contains(&json!({ "key": "type", "match": { "value": "chat" } })));
        assert!(must.contains(&json!({ "key": "sourceId", "match": { "value": "abc" } })));
        assert!(must.contains(&json!({ "key": "importance", "range": { "gte": 3.0 } })));
        assert_eq!(
            filter["must_not"],
            json!([{ "key": "type", "match": { "value": SENTINEL_POINT_TYPE } }]),
        );
    }

    #[test]
    fn planner_entity_filters_stay_soft_so_untagged_events_are_not_lost() {
        let filter = build_filter(&filters(&[
            ("characters_any", json!(["Mayla"])),
            ("event_type_any", json!(["promise"])),
        ]));

        assert!(filter.get("must").is_none());
        assert_eq!(
            filter["should"],
            json!([
                { "key": "characters", "match": { "any": ["Mayla"] } },
                { "key": "event_type", "match": { "any": ["promise"] } },
            ]),
        );
    }

    #[test]
    fn rerank_formula_matches_the_plugin_expression_grammar() {
        let params = RerankParams::default();
        let formula = build_rerank_formula(&params);

        assert_eq!(
            formula,
            json!({
                "sum": [
                    { "mult": [0.4, { "mult": [1.0, "$score"] }] },
                    { "mult": [0.2, { "div": { "left": "importance", "right": 10 } }] },
                    { "mult": [0.2, { "key": "should_persist", "match": { "value": true } }] },
                    {
                        "mult": [
                            0.2,
                            {
                                "exp_decay": {
                                    "x": "source_window_end",
                                    "target": 0.0,
                                    "scale": 40.0,
                                    "midpoint": 0.5,
                                },
                            },
                        ],
                    },
                ],
            }),
        );
    }

    #[test]
    fn rerank_body_adds_dedup_and_min_importance_to_the_outer_filter() {
        let params = RerankParams {
            chat_length: 120.0,
            min_importance: 4.0,
            visible_threshold: 80.0,
            ..RerankParams::default()
        };
        let body = build_hybrid_rerank_query_body(
            &[0.1, 0.2],
            &json!({ "indices": [1], "values": [0.5] }),
            16,
            None,
            &params,
            &filters(&[("sourceId", json!("abc"))]),
        );

        let must = body["filter"]["must"].as_array().expect("outer must");
        assert!(must.contains(&json!({ "key": "importance", "range": { "gte": 4.0 } })));
        assert!(must.contains(&json!({ "key": "source_window_end", "range": { "lt": 80.0 } })));
        assert!(must.contains(&json!({ "key": "sourceId", "match": { "value": "abc" } })));
        assert_eq!(body["limit"], json!(16));
        // Prefetch nests the fused hybrid leg so the formula scores already
        // tenant-scoped candidates.
        assert_eq!(body["prefetch"][0]["query"], json!({ "fusion": "rrf" }));
    }

    #[test]
    fn hybrid_body_omits_using_for_the_default_dense_vector() {
        let body = build_hybrid_query_body(
            &[0.25, 0.75],
            &json!({ "indices": [], "values": [] }),
            5,
            None,
            None,
            &Map::new(),
        );

        let prefetch = body["prefetch"].as_array().expect("prefetch legs");
        assert!(prefetch[0].get("using").is_none());
        assert_eq!(prefetch[1]["using"], json!(SPARSE_VECTOR_KEY));
        // No filters supplied still yields a sentinel-only filter object.
        assert_eq!(
            prefetch[0]["filter"],
            json!({ "must_not": [{ "key": "type", "match": { "value": SENTINEL_POINT_TYPE } }] }),
        );
    }

    #[test]
    fn point_payload_lets_core_fields_override_supplied_metadata() {
        let mut metadata = Map::new();
        metadata.insert("text".to_string(), json!("stale"));
        metadata.insert("importance".to_string(), json!(2));
        metadata.insert("speaker".to_string(), json!("Tav"));

        let point = build_point(
            &PointInput {
                hash: 977206,
                text: "fresh".to_string(),
                index: Some(41),
                metadata,
                vector: vec![0.5, 0.5],
                sparse: Some(json!({ "indices": [7], "values": [0.9] })),
                importance: None,
                keywords: None,
                conditions: None,
                is_summary_chunk: None,
                parent_hash: None,
                tenant: TenantFields {
                    content_type: "chat".to_string(),
                    source_id: "chat-1".to_string(),
                    embedding_source: "transformers".to_string(),
                    embedding_model: "bge".to_string(),
                },
            },
            true,
        );

        assert_eq!(point["id"], json!(977206));
        assert_eq!(point["payload"]["text"], json!("fresh"));
        assert_eq!(point["payload"]["importance"], json!(2));
        assert_eq!(point["payload"]["messageIndex"], json!(41));
        assert_eq!(point["payload"]["speaker"], json!("Tav"));
        // Named sparse vector forces the dense vector into its named slot.
        assert_eq!(point["vector"][DEFAULT_DENSE_VECTOR_KEY], json!([0.5, 0.5]));
        assert_eq!(point["vector"][SPARSE_VECTOR_KEY]["indices"], json!([7]));
    }

    #[test]
    fn collection_names_survive_registry_prefixes() {
        assert_eq!(normalize_collection_name("qdrant:source:vectfox_main"), "vectfox_main");
        assert_eq!(normalize_collection_name("  vectfox_main "), "vectfox_main");
        assert_eq!(
            parse_collection_source("qdrant:chat:abc123"),
            Some(("chat".to_string(), "abc123".to_string())),
        );
        assert_eq!(parse_collection_source("vectfox_main"), None);
    }

    #[test]
    fn eventbase_indexes_cover_every_field_the_formula_and_filters_touch() {
        let fields = eventbase_payload_indexes()
            .into_iter()
            .map(|(field, _)| field)
            .collect::<Vec<_>>();

        for required in [
            "importance",
            "source_window_end",
            "should_persist",
            "characters",
            "event_type",
        ] {
            assert!(fields.contains(&required), "missing index for {required}");
        }
    }
}
