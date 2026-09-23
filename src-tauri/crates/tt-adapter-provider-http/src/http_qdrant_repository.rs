//! Qdrant REST adapter for the Similharity compatibility bridge.
//!
//! Faithful forwarding is the point: any HTTP answer from Qdrant (including 4xx
//! and 5xx) is returned to the caller with its status and body intact, because
//! the VectFox extension parses Qdrant's own error payloads. Only transport
//! failures, timeouts and unreadable bodies become `DomainError`s.
//!
//! Timeouts, proxy handling and connection reuse come from `HttpClientPool`, so
//! this adapter adds no client-side retry policy of its own.

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde_json::Value;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::repositories::qdrant_repository::{
    QdrantConfig, QdrantRepository, QdrantResponse,
};

pub struct HttpQdrantRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpQdrantRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn client(&self) -> Result<Client, DomainError> {
        self.http_clients.client(HttpClientProfile::Default)
    }

    fn base_url(config: &QdrantConfig) -> Result<String, DomainError> {
        config.base_url().ok_or_else(|| {
            DomainError::InvalidData(
                "Qdrant is not initialized: provide a url or host/port before querying".to_string(),
            )
        })
    }

    async fn send(
        &self,
        config: &QdrantConfig,
        request: RequestBuilder,
    ) -> Result<QdrantResponse, DomainError> {
        let request = self.authorize(config, request)?;
        let response = request.send().await.map_err(|error| {
            DomainError::upstream_failure(crate::http_error::reqwest_transport_failure(&error))
        })?;

        let status = response.status().as_u16();
        let endpoint = response.url().clone();
        let text = response.text().await.map_err(|error| {
            DomainError::upstream_failure(crate::http_error::reqwest_body_failure(
                &error,
                Some(&endpoint),
            ))
        })?;

        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| {
                serde_json::json!({ "error": text.chars().take(1_000).collect::<String>() })
            })
        };

        Ok(QdrantResponse { status, body })
    }

    fn authorize(
        &self,
        config: &QdrantConfig,
        request: RequestBuilder,
    ) -> Result<RequestBuilder, DomainError> {
        match config.api_key.as_deref() {
            Some(api_key) if !api_key.is_empty() => Ok(request.header("api-key", api_key)),
            _ => Ok(request),
        }
    }

    async fn request_json(
        &self,
        config: &QdrantConfig,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<QdrantResponse, DomainError> {
        let url = format!("{}{}", Self::base_url(config)?, path);
        let client = self.client()?;
        let mut request = client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.send(config, request).await
    }

    async fn request_collection(
        &self,
        config: &QdrantConfig,
        method: reqwest::Method,
        collection: &str,
        suffix: &str,
        body: Option<Value>,
    ) -> Result<QdrantResponse, DomainError> {
        let collection = validated_collection_name(collection)?;
        self.request_json(config, method, &format!("/collections/{collection}{suffix}"), body)
            .await
    }
}

/// Collection names travel straight into the request path, so they are checked
/// against Qdrant's own naming alphabet instead of being escaped.
fn validated_collection_name(collection: &str) -> Result<String, DomainError> {
    let value = collection.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "collectionId is required".to_string(),
        ));
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid collectionId: {value}"
        )));
    }
    Ok(value.to_string())
}

#[async_trait]
impl QdrantRepository for HttpQdrantRepository {
    async fn probe(&self, config: &QdrantConfig) -> Result<QdrantResponse, DomainError> {
        self.request_json(config, reqwest::Method::GET, "/", None)
            .await
    }

    async fn list_collections(&self, config: &QdrantConfig) -> Result<QdrantResponse, DomainError> {
        self.request_json(config, reqwest::Method::GET, "/collections", None)
            .await
    }

    async fn collection_info(
        &self,
        config: &QdrantConfig,
        collection: &str,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(config, reqwest::Method::GET, collection, "", None)
            .await
    }

    async fn create_collection(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(config, reqwest::Method::PUT, collection, "", Some(body))
            .await
    }

    async fn delete_collection(
        &self,
        config: &QdrantConfig,
        collection: &str,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(config, reqwest::Method::DELETE, collection, "", None)
            .await
    }

    async fn create_payload_index(
        &self,
        config: &QdrantConfig,
        collection: &str,
        field: &str,
        schema: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::PUT,
            collection,
            "/index",
            Some(serde_json::json!({ "field_name": field, "field_schema": schema })),
        )
        .await
    }

    async fn upsert_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::PUT,
            collection,
            "/points?wait=true",
            Some(body),
        )
        .await
    }

    async fn scroll_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/scroll",
            Some(body),
        )
        .await
    }

    async fn get_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points",
            Some(body),
        )
        .await
    }

    async fn count_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/count",
            Some(body),
        )
        .await
    }

    async fn search_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/search",
            Some(body),
        )
        .await
    }

    async fn query_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/query",
            Some(body),
        )
        .await
    }

    async fn delete_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/delete?wait=true",
            Some(body),
        )
        .await
    }

    async fn set_payload(
        &self,
        config: &QdrantConfig,
        collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        self.request_collection(
            config,
            reqwest::Method::POST,
            collection,
            "/points/payload?wait=true",
            Some(body),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_prefers_explicit_url_over_host_port() {
        let cloud = QdrantConfig {
            url: Some("https://cloud.example/qdrant/".to_string()),
            host: Some("ignored".to_string()),
            port: Some(9),
            api_key: None,
        };
        assert_eq!(
            cloud.base_url().as_deref(),
            Some("https://cloud.example/qdrant"),
        );

        let local = QdrantConfig {
            host: Some("127.0.0.1".to_string()),
            port: None,
            ..QdrantConfig::default()
        };
        assert_eq!(local.base_url().as_deref(), Some("http://127.0.0.1:6333"));

        assert_eq!(QdrantConfig::default().base_url(), None);
    }

    #[test]
    fn collection_names_reject_path_traversal() {
        assert_eq!(
            validated_collection_name("vectfox_main").expect("valid name"),
            "vectfox_main",
        );
        assert!(validated_collection_name("").is_err());
        assert!(validated_collection_name("../collections").is_err());
        assert!(validated_collection_name("name/with/slash").is_err());
    }
}
