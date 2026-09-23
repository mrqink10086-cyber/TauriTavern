//! DTOs for the Similharity compatibility bridge (`/api/plugins/similharity/*`).
//!
//! The VectFox extension sends `null` for several optional fields (notably
//! `apiKey` and `url` when it expects the server to resolve a secret), so the
//! fields it may null out are modelled as `Option`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilharityRequestDto {
    pub backend: Option<String>,
    pub collection_id: Option<String>,
    pub source: Option<String>,
    pub model: Option<String>,
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub keep: Option<bool>,
    pub extras_url: Option<String>,
    pub extras_key: Option<String>,
    pub text: Option<String>,
    pub texts: Option<Vec<String>>,
    pub items: Option<Vec<SimilharityItemDto>>,
    pub hashes: Option<Vec<Value>>,
    pub query_vector: Option<Vec<f32>>,
    pub search_text: Option<String>,
    pub sparse_query_vector: Option<Value>,
    pub top_k: Option<u64>,
    pub threshold: Option<f64>,
    pub offset: Option<Value>,
    pub limit: Option<u64>,
    pub include_vectors: Option<bool>,
    pub scroll_limit: Option<u64>,
    pub filters: Option<Map<String, Value>>,
    pub options: Option<Value>,
    pub hybrid_options: Option<Value>,
    pub rerank_params: Option<Value>,
    pub metadata: Option<Map<String, Value>>,
    pub native_sparse: Option<bool>,
    pub cjk_tokenizer_mode: Option<Value>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Fields the extension sends that the bridge forwards to Qdrant untouched.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SimilharityRequestDto {
    pub fn collection_id(&self) -> &str {
        self.collection_id.as_deref().unwrap_or_default()
    }

    pub fn source(&self) -> &str {
        self.source.as_deref().unwrap_or_default()
    }

    pub fn model(&self) -> &str {
        self.model.as_deref().unwrap_or_default()
    }

    pub fn filters(&self) -> Map<String, Value> {
        self.filters.clone().unwrap_or_default()
    }

    pub fn top_k(&self) -> u64 {
        self.top_k.unwrap_or(10)
    }

    /// `nativeSparse` arrives either at the top level or inside `filters`,
    /// depending on the call site in the extension.
    pub fn native_sparse(&self) -> bool {
        self.native_sparse.unwrap_or(false)
            || self
                .filters
                .as_ref()
                .and_then(|filters| filters.get("nativeSparse"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilharityItemDto {
    /// Qdrant point ids must be integers; the extension may send a numeric
    /// string, so this is parsed rather than typed.
    pub hash: Value,
    #[serde(default)]
    pub text: String,
    pub index: Option<i64>,
    pub vector: Option<Vec<f32>>,
    pub sparse_vector: Option<Value>,
    pub metadata: Option<Map<String, Value>>,
    pub importance: Option<Value>,
    pub keywords: Option<Value>,
    pub conditions: Option<Value>,
    pub is_summary_chunk: Option<Value>,
    pub parent_hash: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SimilharityResponseKindDto {
    Json,
    Empty,
}

/// Mirrors the vector route response envelope so the frontend shim can treat
/// both compatibility layers identically.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilharityResponseDto {
    pub status: u16,
    pub kind: SimilharityResponseKindDto,
    pub body: Value,
}
