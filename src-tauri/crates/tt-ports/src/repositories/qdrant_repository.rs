//! Outbound port for a Qdrant instance spoken to over its REST API.
//!
//! The Similharity compatibility bridge forwards the VectFox extension's chunk
//! operations here. Bodies and payloads stay opaque JSON on purpose: the
//! extension owns the payload schema, and Qdrant owns the query engine — the
//! bridge only decides *which* operations are allowed.

use async_trait::async_trait;
use serde_json::Value;
use tt_domain::errors::DomainError;

/// Connection settings supplied by the extension through
/// `POST /backend/init/qdrant`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QdrantConfig {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub api_key: Option<String>,
}

impl QdrantConfig {
    /// Base URL used for requests. `url` wins over `host`/`port`, matching the
    /// plugin: local installs send host/port, Qdrant Cloud sends a full URL.
    pub fn base_url(&self) -> Option<String> {
        if let Some(url) = self.url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
            return Some(url.trim_end_matches('/').to_string());
        }

        let host = self
            .host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())?;
        let port = self.port.unwrap_or(6333);
        Some(format!("http://{host}:{port}"))
    }
}

/// One Qdrant REST response. The status is carried through so the extension sees
/// the engine's own error codes instead of a bridge-invented rewrite.
#[derive(Debug, Clone)]
pub struct QdrantResponse {
    pub status: u16,
    pub body: Value,
}

#[async_trait]
pub trait QdrantRepository: Send + Sync {
    /// `GET /` — used for the version probe that gates the formula query path.
    async fn probe(&self, config: &QdrantConfig) -> Result<QdrantResponse, DomainError>;

    async fn list_collections(&self, config: &QdrantConfig) -> Result<QdrantResponse, DomainError>;

    async fn collection_info(
        &self,
        config: &QdrantConfig,
        collection: &str,
    ) -> Result<QdrantResponse, DomainError>;

    async fn create_collection(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn delete_collection(
        &self,
        config: &QdrantConfig,
        collection: &str,
    ) -> Result<QdrantResponse, DomainError>;

    async fn create_payload_index(
        &self,
        config: &QdrantConfig,
        collection: &str,
        field: &str,
        schema: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn upsert_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn scroll_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn get_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn count_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    /// `POST /collections/{name}/points/search` (dense only).
    async fn search_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    /// `POST /collections/{name}/points/query` (prefetch / fusion / formula).
    async fn query_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn delete_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;

    async fn set_payload(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError>;
}
