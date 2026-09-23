use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::*;
use tt_domain::models::secret::Secrets;
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingRepository, LocalEmbeddingRequest, RemoteEmbeddingBatch,
    RemoteEmbeddingRepository, RemoteEmbeddingRequest, VectorMatch, VectorRecord,
    VectorRepository, VectorScope,
};

#[derive(Default)]
struct FakeQdrant {
    calls: Mutex<Vec<String>>,
    bodies: Mutex<Vec<(String, Value)>>,
    responses: Mutex<HashMap<String, QdrantResponse>>,
}

impl FakeQdrant {
    fn responding(label: &str, status: u16, body: Value) -> Arc<Self> {
        let fake = Arc::new(Self::default());
        fake.responses.lock().expect("responses").insert(
            label.to_string(),
            QdrantResponse { status, body },
        );
        fake
    }

    fn record(&self, label: &str, body: Value) -> QdrantResponse {
        self.calls.lock().expect("calls").push(label.to_string());
        self.bodies
            .lock()
            .expect("bodies")
            .push((label.to_string(), body));
        if let Some(response) = self.responses.lock().expect("responses").get(label) {
            return response.clone();
        }
        let default = match label {
            "list" => json!({ "result": { "collections": [] } }),
            _ => json!({ "result": {} }),
        };
        QdrantResponse {
            status: 200,
            body: default,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls").clone()
    }

    fn body(&self, label: &str) -> Value {
        self.bodies
            .lock()
            .expect("bodies")
            .iter()
            .rev()
            .find(|(recorded, _)| recorded == label)
            .map(|(_, body)| body.clone())
            .expect("recorded body")
    }
}

#[async_trait]
impl QdrantRepository for FakeQdrant {
    async fn probe(&self, _config: &QdrantConfig) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("probe", Value::Null))
    }

    async fn list_collections(&self, _config: &QdrantConfig) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("list", Value::Null))
    }

    async fn collection_info(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("collection_info", Value::Null))
    }

    async fn create_collection(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("create", body))
    }

    async fn delete_collection(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("delete_collection", Value::Null))
    }

    async fn create_payload_index(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        field: &str,
        schema: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("index", json!({ "field": field, "schema": schema })))
    }

    async fn upsert_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("upsert", body))
    }

    async fn scroll_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("scroll", body))
    }

    async fn get_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("get", body))
    }

    async fn count_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("count", body))
    }

    async fn search_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("search", body))
    }

    async fn query_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("query", body))
    }

    async fn delete_points(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("delete_points", body))
    }

    async fn set_payload(
        &self,
        _config: &QdrantConfig,
        _collection: &str,
        body: Value,
    ) -> Result<QdrantResponse, DomainError> {
        Ok(self.record("set_payload", body))
    }
}

#[derive(Default)]
struct FakeSecrets {
    values: Mutex<HashMap<String, String>>,
}

impl FakeSecrets {
    fn with(key: &str, value: &str) -> Arc<Self> {
        let fake = Arc::new(Self::default());
        fake.values
            .lock()
            .expect("secrets")
            .insert(key.to_string(), value.to_string());
        fake
    }
}

#[async_trait]
impl SecretRepository for FakeSecrets {
    async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
        Ok(())
    }

    async fn load(&self) -> Result<Secrets, DomainError> {
        Ok(Secrets::default())
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        Ok(())
    }

    async fn write_secret(
        &self,
        key: &str,
        value: &str,
        _label: &str,
    ) -> Result<String, DomainError> {
        self.values
            .lock()
            .expect("secrets")
            .insert(key.to_string(), value.to_string());
        Ok(key.to_string())
    }

    async fn read_secret(
        &self,
        key: &str,
        _id: Option<&str>,
    ) -> Result<Option<String>, DomainError> {
        Ok(self.values.lock().expect("secrets").get(key).cloned())
    }

    async fn delete_secret(&self, key: &str, _id: Option<&str>) -> Result<(), DomainError> {
        self.values.lock().expect("secrets").remove(key);
        Ok(())
    }

    async fn rotate_secret(&self, _key: &str, _id: &str) -> Result<(), DomainError> {
        Ok(())
    }

    async fn rename_secret(
        &self,
        _key: &str,
        _id: &str,
        _label: &str,
    ) -> Result<(), DomainError> {
        Ok(())
    }
}

/// Embedding through these ports is a test failure by default: every test that
/// needs a vector supplies one, which proves the bridge does not re-embed work
/// the extension already vectorized.
struct FakeVectorRepository;

#[async_trait]
impl VectorRepository for FakeVectorRepository {
    async fn list_hashes(&self, _scope: &VectorScope) -> Result<Vec<i64>, DomainError> {
        Ok(Vec::new())
    }

    async fn upsert(
        &self,
        _scope: &VectorScope,
        _records: Vec<VectorRecord>,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn delete_hashes(&self, _scope: &VectorScope, _hashes: &[i64]) -> Result<(), DomainError> {
        Ok(())
    }

    async fn query(
        &self,
        _scope: &VectorScope,
        _embedding: Vec<f32>,
        _limit: usize,
    ) -> Result<Vec<VectorMatch>, DomainError> {
        Ok(Vec::new())
    }

    async fn purge_collection(&self, _collection_id: &str) -> Result<(), DomainError> {
        Ok(())
    }

    async fn purge_all(&self) -> Result<(), DomainError> {
        Ok(())
    }
}

struct FakeRemoteEmbeddings;

#[async_trait]
impl RemoteEmbeddingRepository for FakeRemoteEmbeddings {
    async fn embed(
        &self,
        _request: RemoteEmbeddingRequest,
    ) -> Result<RemoteEmbeddingBatch, DomainError> {
        Err(DomainError::InternalError(
            "remote embedding must not be used in these tests".to_string(),
        ))
    }
}

struct FakeLocalEmbeddings {
    calls: Mutex<usize>,
}

#[async_trait]
impl LocalEmbeddingRepository for FakeLocalEmbeddings {
    async fn embed(&self, request: LocalEmbeddingRequest) -> Result<Vec<Vec<f32>>, DomainError> {
        *self.calls.lock().expect("calls") += request.texts.len();
        Ok(request
            .texts
            .iter()
            .map(|_| vec![0.25_f32; 4])
            .collect::<Vec<_>>())
    }
}

fn service(qdrant: Arc<FakeQdrant>, secrets: Arc<FakeSecrets>) -> SimilharityService {
    let local = Arc::new(FakeLocalEmbeddings {
        calls: Mutex::new(0),
    });
    let vector_service = Arc::new(VectorService::new(
        Arc::new(FakeVectorRepository),
        Arc::new(FakeRemoteEmbeddings),
        local,
        secrets.clone(),
        tt_domain::ios_policy::resolve_ios_policy_activation_report(
            tt_domain::ios_policy::IosPolicyScope::Ignored,
            None,
        )
        .expect("ios policy"),
    ));
    SimilharityService::new(qdrant, vector_service, secrets)
}

async fn initialized(
    qdrant: Arc<FakeQdrant>,
    secrets: Arc<FakeSecrets>,
) -> SimilharityService {
    let service = service(qdrant, secrets);
    service
        .handle_request(
            "post",
            "backend/init/qdrant",
            request(json!({ "host": "127.0.0.1", "port": 6333 })),
        )
        .await
        .expect("init succeeds");
    service
}

fn request(body: Value) -> SimilharityRequestDto {
    serde_json::from_value(body).expect("request dto")
}

#[tokio::test]
async fn health_and_version_report_the_plugin_contract() {
    let service = service(Arc::new(FakeQdrant::default()), Arc::new(FakeSecrets::default()));

    let health = service
        .handle_request("get", "health", request(json!({})))
        .await
        .expect("health");
    assert_eq!(health.body["plugin"], json!("similharity"));
    assert_eq!(health.body["version"], json!(PLUGIN_VERSION));
    assert_eq!(health.body["backends"], json!(["vectra", "qdrant"]));

    let version = service
        .handle_request("get", "version", request(json!({})))
        .await
        .expect("version");
    assert_eq!(version.body["pluginVersion"], json!(PLUGIN_VERSION));
}

#[tokio::test]
async fn qdrant_endpoints_require_an_initialized_connection() {
    let service = service(Arc::new(FakeQdrant::default()), Arc::new(FakeSecrets::default()));

    let error = service
        .handle_request("post", "chunks/list", request(json!({ "collectionId": "vectfox_main" })))
        .await
        .expect_err("uninitialized connection is rejected");

    assert!(matches!(error, ApplicationError::ValidationError(_)));
}

#[tokio::test]
async fn init_resolves_the_api_key_from_secrets_and_key_status_masks_it() {
    let secrets = FakeSecrets::with("api_key_qdrant", "super-secret-1234");
    let service = service(Arc::new(FakeQdrant::default()), secrets);

    service
        .handle_request(
            "post",
            "backend/init/qdrant",
            request(json!({
                "url": "https://cloud.example",
                "apiKey": Value::Null,
            })),
        )
        .await
        .expect("init succeeds");

    let status = service
        .handle_request("get", "qdrant/key-status", request(json!({})))
        .await
        .expect("key status");
    assert_eq!(status.body, json!({ "set": true, "masked": "****1234" }));
}

#[tokio::test]
async fn insert_creates_the_collection_then_upserts_extension_shaped_points() {
    let qdrant = Arc::new(FakeQdrant::default());
    let service = initialized(qdrant.clone(), Arc::new(FakeSecrets::default())).await;

    let response = service
        .handle_request(
            "post",
            "chunks/insert",
            request(json!({
                "collectionId": "qdrant:chat:abc",
                "source": "transformers",
                "model": "bge",
                "nativeSparse": true,
                "filters": { "type": "chat", "sourceId": "abc" },
                "items": [{
                    "hash": "977206",
                    "text": "塔芙买了护胸。",
                    "index": 41,
                    "vector": [0.5, 0.5, 0.5],
                    "sparseVector": { "indices": [7], "values": [0.9] },
                    "metadata": { "importance": 6, "characters": ["塔芙"] }
                }]
            })),
        )
        .await
        .expect("insert");

    assert_eq!(response.body["success"], json!(true));
    assert_eq!(response.body["inserted"], json!(1));
    assert_eq!(response.body["embedded"], json!(0));

    let calls = qdrant.calls();
    assert!(calls.contains(&"create".to_string()));
    assert!(calls.contains(&"upsert".to_string()));

    let create = qdrant.body("create");
    assert_eq!(create["vectors"]["size"], json!(3));
    assert_eq!(create["sparse_vectors"]["text_sparse"]["modifier"], json!("idf"));

    let upsert = qdrant.body("upsert");
    let point = &upsert["points"][0];
    assert_eq!(point["id"], json!(977206));
    assert_eq!(point["payload"]["text"], json!("塔芙买了护胸。"));
    assert_eq!(point["payload"]["type"], json!("chat"));
    assert_eq!(point["payload"]["sourceId"], json!("abc"));
    assert_eq!(point["payload"]["messageIndex"], json!(41));
    assert_eq!(point["payload"]["importance"], json!(6));
    // Sparse vectors force the dense vector into its named slot.
    assert_eq!(point["vector"][""], json!([0.5, 0.5, 0.5]));
    assert_eq!(point["vector"]["text_sparse"]["indices"], json!([7]));
}

#[tokio::test]
async fn insert_embeds_only_the_items_that_arrive_without_vectors() {
    let qdrant = Arc::new(FakeQdrant::default());
    let service = initialized(qdrant.clone(), Arc::new(FakeSecrets::default())).await;

    service
        .handle_request(
            "post",
            "chunks/insert",
            request(json!({
                "collectionId": "vectfox_main",
                "items": [
                    { "hash": 1, "text": "already vectorized", "vector": [1.0, 0.0] },
                    { "hash": 2, "text": "needs embedding" }
                ]
            })),
        )
        .await
        .expect("insert");

    let upsert = qdrant.body("upsert");
    // Without `nativeSparse` the dense vector keeps the plain-array form, which
    // is what pre-existing collections were created with. A vector supplied by
    // the extension is written as-is: no re-embedding, no re-normalization.
    assert_eq!(upsert["points"][0]["vector"], json!([1.0, 0.0]));
    // Items that arrive without a vector are embedded through the shared vector
    // service, so they come back L2-normalized (0.25 × 4 → 0.5 × 4).
    assert_eq!(upsert["points"][1]["vector"], json!([0.5, 0.5, 0.5, 0.5]));
}

#[tokio::test]
async fn engine_errors_are_forwarded_with_the_engine_status() {
    let qdrant = FakeQdrant::responding(
        "upsert",
        400,
        json!({ "status": { "error": "Wrong input: vector dimension error" } }),
    );
    let service = initialized(qdrant, Arc::new(FakeSecrets::default())).await;

    let response = service
        .handle_request(
            "post",
            "chunks/insert",
            request(json!({
                "collectionId": "vectfox_main",
                "items": [{ "hash": 5, "text": "x", "vector": [0.1, 0.2] }]
            })),
        )
        .await
        .expect("engine errors come back as a response");

    assert_eq!(response.status, 400);
    assert_eq!(response.body["success"], json!(false));
    assert_eq!(
        response.body["error"],
        json!("Wrong input: vector dimension error")
    );
}

#[tokio::test]
async fn hybrid_query_requires_a_sparse_query_vector() {
    let service = initialized(Arc::new(FakeQdrant::default()), Arc::new(FakeSecrets::default())).await;

    let error = service
        .handle_request(
            "post",
            "chunks/hybrid-query",
            request(json!({
                "collectionId": "vectfox_main",
                "queryVector": [0.1, 0.2]
            })),
        )
        .await
        .expect_err("sparse query vector is required");

    assert!(matches!(error, ApplicationError::ValidationError(_)));
}

#[tokio::test]
async fn hybrid_rerank_reports_a_clear_error_below_qdrant_1_13() {
    let qdrant = FakeQdrant::responding("probe", 200, json!({ "version": "1.12.4" }));
    let service = initialized(qdrant, Arc::new(FakeSecrets::default())).await;

    let response = service
        .handle_request(
            "post",
            "chunks/hybrid-query-rerank",
            request(json!({
                "collectionId": "vectfox_main",
                "queryVector": [0.1, 0.2],
                "sparseQueryVector": { "indices": [1], "values": [0.5] },
                "rerankParams": { "chatLength": 40 }
            })),
        )
        .await
        .expect("unsupported formula path answers with a status");

    assert_eq!(response.status, 400);
    assert!(
        response.body["error"]
            .as_str()
            .expect("message")
            .contains("1.13"),
        "unexpected message: {}",
        response.body["error"],
    );
}

#[tokio::test]
async fn hybrid_rerank_builds_the_formula_query_on_supported_qdrant() {
    let qdrant = FakeQdrant::responding("probe", 200, json!({ "version": "1.13.1" }));
    let service = initialized(qdrant.clone(), Arc::new(FakeSecrets::default())).await;

    service
        .handle_request(
            "post",
            "chunks/hybrid-query-rerank",
            request(json!({
                "collectionId": "vectfox_main",
                "queryVector": [0.1, 0.2],
                "sparseQueryVector": { "indices": [1], "values": [0.5] },
                "filters": { "sourceId": "abc" },
                "rerankParams": { "chatLength": 120, "minImportance": 4 }
            })),
        )
        .await
        .expect("rerank query");

    let body = qdrant.body("query");
    assert_eq!(body["prefetch"][0]["query"], json!({ "fusion": "rrf" }));
    assert_eq!(body["query"]["formula"]["sum"][0]["mult"][0], json!(0.4));
    let outer_must = body["filter"]["must"].as_array().expect("outer must");
    assert!(outer_must.contains(&json!({ "key": "importance", "range": { "gte": 4.0 } })));
    assert!(outer_must.contains(&json!({ "key": "sourceId", "match": { "value": "abc" } })));
}

#[tokio::test]
async fn purge_without_filters_drops_the_collection() {
    let qdrant = Arc::new(FakeQdrant::default());
    let service = initialized(qdrant.clone(), Arc::new(FakeSecrets::default())).await;

    service
        .handle_request(
            "post",
            "chunks/purge",
            request(json!({ "collectionId": "vectfox_main" })),
        )
        .await
        .expect("purge");

    assert!(qdrant.calls().contains(&"delete_collection".to_string()));
}

#[tokio::test]
async fn unknown_endpoints_are_not_found() {
    let service = service(Arc::new(FakeQdrant::default()), Arc::new(FakeSecrets::default()));

    let error = service
        .handle_request("post", "chunks/mystery", request(json!({})))
        .await
        .expect_err("unknown endpoint");

    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[test]
fn mask_secret_keeps_only_the_tail() {
    assert_eq!(mask_secret("super-secret-1234"), "****1234");
    assert_eq!(mask_secret("abc"), "****");
}
