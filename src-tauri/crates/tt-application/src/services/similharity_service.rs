//! Similharity compatibility bridge.
//!
//! Serves `/api/plugins/similharity/*` for the VectFox extension by forwarding
//! its chunk operations to a local Qdrant instance. The bridge is A3-only: every
//! collection lives in Qdrant, so the host's own vector storage is untouched and
//! no second retrieval implementation is introduced.
//!
//! Two behaviours deliberately differ from the SillyTavern server plugin:
//!
//! 1. A dimension conflict never triggers an implicit `purgeAll` — the engine's
//!    error is forwarded so the user decides.
//! 2. Transport failures surface as host errors instead of an empty result.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde_json::{Map, Value, json};

use crate::dto::similharity_dto::{
    SimilharityRequestDto, SimilharityResponseDto, SimilharityResponseKindDto,
};
use crate::dto::vector_dto::VectorRouteRequestDto;
use crate::errors::ApplicationError;
use crate::services::vector_service::VectorService;
use tt_domain::errors::DomainError;
use tt_domain::models::secret::SecretKeys;
use tt_domain::models::similharity::{
    PLUGIN_VERSION, PointInput, RerankParams, SENTINEL_POINT_ID, TenantFields,
    build_hybrid_query_body, build_hybrid_rerank_query_body, build_point, eventbase_payload_indexes,
    normalize_collection_name,
};
use tt_ports::repositories::qdrant_repository::{
    QdrantConfig, QdrantRepository, QdrantResponse,
};
use tt_ports::repositories::secret_repository::SecretRepository;

/// Payload type marking a chat event or chunk; mirrors the plugin's default.
const DEFAULT_TENANT_TYPE: &str = "chat";
const DEFAULT_SCROLL_LIMIT: u64 = 100;
/// Formula queries (used by the EventBase rerank path) landed in Qdrant 1.13.
const FORMULA_MIN_MAJOR: u64 = 1;
const FORMULA_MIN_MINOR: u64 = 13;

pub struct SimilharityService {
    qdrant_repository: Arc<dyn QdrantRepository>,
    vector_service: Arc<VectorService>,
    secret_repository: Arc<dyn SecretRepository>,
    /// Qdrant connection chosen by the extension through `/backend/init/qdrant`.
    /// Kept in memory, like the plugin's backend instance.
    connection: RwLock<Option<QdrantConfig>>,
}

/// A failure inside the bridge: either a host-level application error, or an
/// answer from Qdrant that is forwarded verbatim (status included).
enum BridgeError {
    Application(ApplicationError),
    Engine { status: u16, message: String },
}

impl From<ApplicationError> for BridgeError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

impl From<DomainError> for BridgeError {
    fn from(error: DomainError) -> Self {
        Self::Application(error.into())
    }
}

type BridgeResult<T> = Result<T, BridgeError>;

impl SimilharityService {
    pub fn new(
        qdrant_repository: Arc<dyn QdrantRepository>,
        vector_service: Arc<VectorService>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            qdrant_repository,
            vector_service,
            secret_repository,
            connection: RwLock::new(None),
        }
    }

    pub async fn handle_request(
        &self,
        method: &str,
        endpoint: &str,
        request: SimilharityRequestDto,
    ) -> Result<SimilharityResponseDto, ApplicationError> {
        let segments = endpoint
            .trim()
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        let is_get = method.eq_ignore_ascii_case("get");

        let result = match segments.as_slice() {
            ["health"] => Ok(json_response(health_body())),
            ["version"] => Ok(json_response(json!({ "pluginVersion": PLUGIN_VERSION }))),
            ["collections"] => self.collections().await,
            ["sources"] => self.sources().await,
            ["purge-all"] => self.purge_all().await,
            ["backend", "health", backend] => self.backend_health(backend).await,
            ["backend", "init", backend] => self.backend_init(backend, &request).await,
            ["qdrant", "key-status"] => self.qdrant_key_status().await,
            ["chunks", "insert"] => self.chunks_insert(&request).await,
            ["chunks", "list"] => self.chunks_list(&request).await,
            ["chunks", "query"] => self.chunks_query(&request).await,
            ["chunks", "hybrid-query"] => self.chunks_hybrid_query(&request, false).await,
            ["chunks", "hybrid-query-rerank"] => self.chunks_hybrid_query(&request, true).await,
            ["chunks", "delete"] => self.chunks_delete(&request).await,
            ["chunks", "purge"] => self.chunks_purge(&request).await,
            ["chunks", "stats"] => self.chunks_stats(&request).await,
            ["chunks", "collection-metadata"] => self.chunks_collection_metadata(&request).await,
            ["chunks", "ensure-eventbase-indexes"] => {
                self.chunks_ensure_eventbase_indexes(&request).await
            }
            ["get-embedding"] => self.get_embedding(&request).await,
            ["batch-embeddings"] => self.batch_embeddings(&request).await,
            ["chunks", hash] if is_get => self.chunk_get(hash, &request).await,
            ["chunks", hash, "text"] => self.chunk_update_text(hash, &request).await,
            ["chunks", hash, "metadata"] => self.chunk_update_metadata(hash, &request).await,
            other => Err(BridgeError::Application(ApplicationError::NotFound(
                format!("Unsupported Similharity endpoint: {}", other.join("/")),
            ))),
        };

        match result {
            Ok(response) => Ok(response),
            // Qdrant answered with an error status: hand it back unchanged so the
            // extension reads the engine's own message and status code.
            Err(BridgeError::Engine { status, message }) => {
                Ok(json_status(status, json!({ "success": false, "error": message })))
            }
            Err(BridgeError::Application(error)) => Err(error),
        }
    }

    // ---------------------------------------------------------------- health

    async fn backend_health(&self, backend: &str) -> BridgeResult<SimilharityResponseDto> {
        match backend.to_ascii_lowercase().as_str() {
            "qdrant" => {
                let config = self.connection()?;
                let response = self.qdrant_repository.probe(&config).await?;
                let healthy = response.status < 400;
                let message = if healthy {
                    "Qdrant is reachable".to_string()
                } else {
                    qdrant_error_message(&response.body)
                };
                Ok(json_response(json!({
                    "backend": "qdrant",
                    "healthy": healthy,
                    "message": message,
                })))
            }
            "vectra" | "standard" => Ok(json_response(json!({
                "backend": backend,
                "healthy": true,
                "message": "Vectra requires no initialization",
            }))),
            other => Err(BridgeError::Application(ApplicationError::ValidationError(
                format!("Unknown backend: {other}"),
            ))),
        }
    }

    async fn backend_init(
        &self,
        backend: &str,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        if !backend.eq_ignore_ascii_case("qdrant") {
            return Ok(json_response(json!({
                "success": true,
                "message": "Vectra requires no initialization",
            })));
        }

        let url = request
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string);
        // The extension sends `apiKey: null` when it expects the host to resolve
        // the key from its secret store, exactly as the plugin does.
        let api_key = match request.api_key.as_deref() {
            Some(key) if !key.is_empty() => Some(key.to_string()),
            _ => self
                .secret_repository
                .read_secret(SecretKeys::QDRANT, None)
                .await?,
        };
        let config = QdrantConfig {
            url,
            host: request.host.clone(),
            port: request.port,
            api_key,
        };
        if config.base_url().is_none() {
            return Err(BridgeError::Application(ApplicationError::ValidationError(
                "Qdrant configuration requires a url or a host".to_string(),
            )));
        }

        let mut guard = self.connection.write().map_err(|_| {
            BridgeError::Application(ApplicationError::InternalError(
                "Qdrant connection lock poisoned".to_string(),
            ))
        })?;
        *guard = Some(config);

        Ok(json_response(json!({
            "success": true,
            "message": "Qdrant initialized",
        })))
    }

    async fn qdrant_key_status(&self) -> BridgeResult<SimilharityResponseDto> {
        let key = self
            .secret_repository
            .read_secret(SecretKeys::QDRANT, None)
            .await?;
        let (set, masked) = match key.as_deref() {
            Some(value) if !value.is_empty() => (true, mask_secret(value)),
            _ => (false, String::new()),
        };
        Ok(json_response(json!({ "set": set, "masked": masked })))
    }

    // ----------------------------------------------------------- collections

    async fn collections(&self) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let listed = engine(self.qdrant_repository.list_collections(&config).await?)?;
        let names = collection_names(&listed);

        let mut collections = Vec::with_capacity(names.len());
        for name in names {
            let count = self.count_points(&config, &name, None).await.unwrap_or(0);
            let (source, id) = parse_source(&name);
            collections.push(json!({
                "id": id,
                "source": source,
                "backend": "qdrant",
                "chunkCount": count,
                "modelCount": 1,
            }));
        }

        Ok(json_response(json!({
            "success": true,
            "count": collections.len(),
            "qdrantScanned": true,
            "collections": collections,
        })))
    }

    async fn sources(&self) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let listed = engine(self.qdrant_repository.list_collections(&config).await?)?;
        let mut sources = Vec::new();
        for name in collection_names(&listed) {
            let (source, _) = parse_source(&name);
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        sources.sort();

        Ok(json_response(json!({ "success": true, "sources": sources })))
    }

    async fn purge_all(&self) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let listed = engine(self.qdrant_repository.list_collections(&config).await?)?;
        for name in collection_names(&listed) {
            tracing::warn!(collection = %name, "Similharity bridge: purge-all deleting collection");
            self.qdrant_repository
                .delete_collection(&config, &name)
                .await?;
        }
        Ok(json_response(json!({
            "success": true,
            "message": "All vectors purged",
        })))
    }

    // ----------------------------------------------------------------- chunks

    async fn chunks_insert(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let items = request.items.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "items must be an array".to_string(),
            ))
        })?;
        if items.is_empty() {
            return Ok(json_response(json!({
                "success": true,
                "backend": "qdrant",
                "collectionId": collection,
                "inserted": 0,
            })));
        }

        let native_sparse = request.native_sparse();
        let filters = request.filters();
        let tenant = tenant_fields(&filters, request);

        let pending_texts = items
            .iter()
            .filter(|item| item.vector.is_none())
            .map(|item| item.text.clone())
            .collect::<Vec<_>>();
        let mut embeddings = if pending_texts.is_empty() {
            Vec::new()
        } else {
            self.embed(request, pending_texts, false).await?
        }
        .into_iter();
        let mut embedded = 0usize;

        let mut points = Vec::with_capacity(items.len());
        let mut dimension = 0usize;
        for item in &items {
            let hash = parse_hash(&item.hash)?;
            let vector = match &item.vector {
                Some(vector) => vector.clone(),
                None => {
                    embedded += 1;
                    embeddings.next().ok_or_else(|| {
                        BridgeError::Application(ApplicationError::InternalError(
                            "embedding count does not match the item count".to_string(),
                        ))
                    })?
                }
            };
            dimension = vector.len();
            points.push(build_point(
                &PointInput {
                    hash,
                    text: item.text.clone(),
                    index: item.index,
                    metadata: item.metadata.clone().unwrap_or_default(),
                    vector,
                    sparse: item.sparse_vector.clone(),
                    importance: item.importance.clone(),
                    keywords: item.keywords.clone(),
                    conditions: item.conditions.clone(),
                    is_summary_chunk: item.is_summary_chunk.clone(),
                    parent_hash: item.parent_hash.clone(),
                    tenant: tenant.clone(),
                },
                native_sparse,
            ));
        }

        self.ensure_collection(&config, &collection, dimension, native_sparse)
            .await?;
        engine(
            self.qdrant_repository
                .upsert_points(&config, &collection, json!({ "points": points }))
                .await?,
        )?;

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "inserted": items.len(),
            "embedded": embedded,
        })))
    }

    async fn chunks_list(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let limit = request
            .scroll_limit
            .or(request.limit)
            .filter(|limit| *limit > 0)
            .unwrap_or(DEFAULT_SCROLL_LIMIT);

        let mut body = json!({
            "filter": tt_domain::models::similharity::build_filter(&request.filters()),
            "limit": limit,
            "with_payload": true,
            "with_vector": request.include_vectors.unwrap_or(false),
        });
        if let Some(offset) = request.offset.as_ref().filter(|offset| !offset.is_null()) {
            body["offset"] = offset.clone();
        }

        let result = engine(
            self.qdrant_repository
                .scroll_points(&config, &collection, body)
                .await?,
        )?;
        let points = result["result"]["points"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let items = points.iter().map(shape_chunk).collect::<Vec<_>>();
        let next_offset = result["result"]["next_page_offset"].clone();

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "items": items,
            "total": items.len(),
            "offset": request.offset,
            "limit": limit,
            "hasMore": !next_offset.is_null(),
        })))
    }

    async fn chunk_get(
        &self,
        hash: &str,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let hash = parse_hash_str(hash)?;
        let result = engine(
            self.qdrant_repository
                .get_points(&config, &collection, json!({ "ids": [hash], "with_payload": true }))
                .await?,
        )?;

        let point = result["result"]
            .as_array()
            .and_then(|points| points.first())
            .cloned();
        let Some(point) = point else {
            return Err(BridgeError::Application(ApplicationError::NotFound(
                format!("Chunk {hash} was not found"),
            )));
        };

        Ok(json_response(json!({
            "success": true,
            "chunk": shape_chunk(&point),
        })))
    }

    async fn chunk_update_text(
        &self,
        hash: &str,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let hash = parse_hash_str(hash)?;
        let text = require_text(request)?;

        let existing = engine(
            self.qdrant_repository
                .get_points(&config, &collection, json!({ "ids": [hash], "with_payload": true }))
                .await?,
        )?;
        let mut payload = existing["result"]
            .as_array()
            .and_then(|points| points.first())
            .and_then(|point| point["payload"].as_object().cloned())
            .unwrap_or_default();
        payload.insert("text".to_string(), json!(text));

        let mut embeddings = self.embed(request, vec![text.clone()], false).await?;
        let vector = embeddings.pop().unwrap_or_default();

        engine(
            self.qdrant_repository
                .upsert_points(
                    &config,
                    &collection,
                    json!({
                        "points": [{ "id": hash, "vector": vector, "payload": payload }],
                    }),
                )
                .await?,
        )?;

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "oldHash": hash,
            "newHash": hash,
            "text": text,
        })))
    }

    async fn chunk_update_metadata(
        &self,
        hash: &str,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let hash = parse_hash_str(hash)?;
        let metadata = request.metadata.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "metadata is required".to_string(),
            ))
        })?;

        engine(
            self.qdrant_repository
                .set_payload(
                    &config,
                    &collection,
                    json!({ "points": [hash], "payload": metadata }),
                )
                .await?,
        )?;

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "hash": hash,
            "metadata": metadata,
        })))
    }

    async fn chunks_delete(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let hashes = request.hashes.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "hashes must be an array".to_string(),
            ))
        })?;
        let ids = hashes
            .iter()
            .filter_map(|value| parse_hash(value).ok())
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            engine(
                self.qdrant_repository
                    .delete_points(&config, &collection, json!({ "points": ids }))
                    .await?,
            )?;
        }

        // The plugin reports the requested count, not the number of points that
        // actually existed; the extension relies on that shape.
        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "deleted": hashes.len(),
        })))
    }

    async fn chunks_purge(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let filters = request.filters();

        if filters.is_empty() {
            self.qdrant_repository
                .delete_collection(&config, &collection)
                .await?;
        } else {
            engine(
                self.qdrant_repository
                    .delete_points(
                        &config,
                        &collection,
                        json!({ "filter": tt_domain::models::similharity::build_filter(&filters) }),
                    )
                    .await?,
            )?;
        }

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "message": format!("Collection {collection} purged"),
        })))
    }

    async fn chunks_stats(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let filters = request.filters();
        let filter = if filters.is_empty() {
            None
        } else {
            Some(tt_domain::models::similharity::build_filter(&filters))
        };
        let count = self.count_points(&config, &collection, filter).await?;
        let dimension = self
            .collection_dimension(&config, &collection)
            .await
            .unwrap_or(0);

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "stats": {
                "chunkCount": count,
                "totalPoints": count,
                "totalCharacters": 0,
                "totalTokens": 0,
                "storageSize": 0,
                "embeddingDimensions": dimension,
                "avgChunkSize": 0,
                "messageCount": 0,
                "sources": [],
                "backend": "qdrant",
            },
        })))
    }

    async fn chunks_collection_metadata(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let result = engine(
            self.qdrant_repository
                .get_points(
                    &config,
                    &collection,
                    json!({ "ids": [SENTINEL_POINT_ID], "with_payload": true }),
                )
                .await?,
        )?;
        let payload = result["result"]
            .as_array()
            .and_then(|points| points.first())
            .map(|point| point["payload"].clone())
            .unwrap_or(Value::Null);

        Ok(json_response(json!({ "payload": payload, "supported": true })))
    }

    async fn chunks_ensure_eventbase_indexes(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        self.create_payload_indexes(&config, &collection).await;
        Ok(json_response(json!({ "ensured": true, "collectionId": collection })))
    }

    async fn chunks_query(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let top_k = request.top_k();

        let started = Instant::now();
        let (vector, embed_ms) = self.resolve_query_vector(request).await?;
        let mut body = json!({
            "query": vector,
            "limit": top_k,
            "with_payload": true,
            "filter": tt_domain::models::similharity::build_filter(&request.filters()),
        });
        if let Some(threshold) = request.threshold.filter(|value| *value > 0.0) {
            body["score_threshold"] = json!(threshold);
        }

        let result = engine(
            self.qdrant_repository
                .search_points(&config, &collection, body)
                .await?,
        )?;

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "count": result.as_array().map(Vec::len).unwrap_or(0),
            "results": shape_scored_results(&result, false),
            "timings": { "embedMs": embed_ms, "queryMs": started.elapsed().as_millis() as u64 },
        })))
    }

    async fn chunks_hybrid_query(
        &self,
        request: &SimilharityRequestDto,
        rerank: bool,
    ) -> BridgeResult<SimilharityResponseDto> {
        let config = self.connection()?;
        let collection = require_collection(request)?;
        let sparse = request.sparse_query_vector.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "sparseQueryVector is required".to_string(),
            ))
        })?;
        let top_k = request
            .top_k
            .unwrap_or(if rerank { 16 } else { 10 });
        let options = merged_options(request);
        let prefetch_limit = options
            .get("prefetchLimit")
            .and_then(Value::as_u64)
            .filter(|limit| *limit > 0);

        let started = Instant::now();
        let (dense, embed_ms) = self.resolve_query_vector(request).await?;
        let filters = request.filters();

        let body = if rerank {
            let params_value = request.rerank_params.clone().ok_or_else(|| {
                BridgeError::Application(ApplicationError::ValidationError(
                    "rerankParams is required".to_string(),
                ))
            })?;
            let params: RerankParams = serde_json::from_value(params_value).map_err(|error| {
                BridgeError::Application(ApplicationError::ValidationError(format!(
                    "Invalid rerankParams: {error}"
                )))
            })?;
            if !self.supports_formula(&config).await? {
                return Err(BridgeError::Engine {
                    status: 400,
                    message: format!(
                        "Qdrant >= {FORMULA_MIN_MAJOR}.{FORMULA_MIN_MINOR} is required for the server-side rerank formula; disable eventbase_native_rerank to rerank in the extension"
                    ),
                });
            }
            build_hybrid_rerank_query_body(
                &dense,
                &sparse,
                top_k,
                prefetch_limit,
                &params,
                &filters,
            )
        } else {
            let fusion = options
                .get("fusion")
                .and_then(Value::as_str)
                .map(str::to_string);
            build_hybrid_query_body(
                &dense,
                &sparse,
                top_k,
                prefetch_limit,
                fusion.as_deref(),
                &filters,
            )
        };

        let result = engine(
            self.qdrant_repository
                .query_points(&config, &collection, body)
                .await?,
        )?;
        let points = result["result"]["points"].clone();

        Ok(json_response(json!({
            "success": true,
            "backend": "qdrant",
            "collectionId": collection,
            "count": points.as_array().map(Vec::len).unwrap_or(0),
            "rerankApplied": rerank,
            "results": shape_scored_results(&points, true),
            "timings": { "embedMs": embed_ms, "queryMs": started.elapsed().as_millis() as u64 },
        })))
    }

    // -------------------------------------------------------------- embedding

    async fn get_embedding(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let text = require_text(request)?;
        if request.source().is_empty() {
            return Err(BridgeError::Application(ApplicationError::ValidationError(
                "text and source are required".to_string(),
            )));
        }
        let mut embeddings = self.embed(request, vec![text], false).await?;
        let embedding = embeddings.pop().unwrap_or_default();

        Ok(json_response(json!({ "success": true, "embedding": embedding })))
    }

    async fn batch_embeddings(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<SimilharityResponseDto> {
        let texts = request.texts.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "texts must be an array".to_string(),
            ))
        })?;
        let embeddings = self.embed(request, texts, false).await?;

        Ok(json_response(json!({
            "success": true,
            "embeddings": embeddings,
        })))
    }

    /// Embedding is delegated to `VectorService`, so provider selection and
    /// normalization stay in the single implementation the vector routes use.
    async fn embed(
        &self,
        request: &SimilharityRequestDto,
        texts: Vec<String>,
        is_query: bool,
    ) -> BridgeResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let vector_request = embedding_request(request);
        Ok(self
            .vector_service
            .embed_texts(&vector_request, texts, is_query)
            .await?)
    }

    async fn resolve_query_vector(
        &self,
        request: &SimilharityRequestDto,
    ) -> BridgeResult<(Vec<f32>, Option<u64>)> {
        if let Some(vector) = request.query_vector.clone().filter(|vector| !vector.is_empty()) {
            return Ok((vector, None));
        }
        let text = require_text(request)?;
        let started = Instant::now();
        let mut embeddings = self.embed(request, vec![text], true).await?;
        let vector = embeddings.pop().ok_or_else(|| {
            BridgeError::Application(ApplicationError::InternalError(
                "embedding provider returned no vector".to_string(),
            ))
        })?;
        Ok((vector, Some(started.elapsed().as_millis() as u64)))
    }

    // ----------------------------------------------------------------- helpers

    fn connection(&self) -> BridgeResult<QdrantConfig> {
        let guard = self.connection.read().map_err(|_| {
            BridgeError::Application(ApplicationError::InternalError(
                "Qdrant connection lock poisoned".to_string(),
            ))
        })?;
        guard.clone().ok_or_else(|| {
            BridgeError::Application(ApplicationError::ValidationError(
                "Qdrant is not initialized: call /backend/init/qdrant first".to_string(),
            ))
        })
    }

    /// Create the collection and its payload indexes when missing. Index
    /// creation is best-effort: an index failure must not lose the insert.
    async fn ensure_collection(
        &self,
        config: &QdrantConfig,
        collection: &str,
        dimension: usize,
        native_sparse: bool,
    ) -> BridgeResult<()> {
        let listed = engine(self.qdrant_repository.list_collections(config).await?)?;
        if collection_names(&listed).iter().any(|name| name == collection) {
            return Ok(());
        }

        let mut body = json!({
            "vectors": { "size": dimension.max(1), "distance": "Cosine" },
        });
        if native_sparse {
            body["sparse_vectors"] = json!({ "text_sparse": { "modifier": "idf" } });
        }

        let created = self
            .qdrant_repository
            .create_collection(config, collection, body)
            .await?;
        // 409 means a concurrent request created it first, which is fine.
        if created.status >= 400 && created.status != 409 {
            return Err(BridgeError::Engine {
                status: created.status,
                message: qdrant_error_message(&created.body),
            });
        }

        self.create_payload_indexes(config, collection).await;
        Ok(())
    }

    async fn create_payload_indexes(&self, config: &QdrantConfig, collection: &str) {
        for (field, schema) in eventbase_payload_indexes() {
            match self
                .qdrant_repository
                .create_payload_index(config, collection, field, schema)
                .await
            {
                Ok(response) if response.status >= 400 && response.status != 409 => {
                    tracing::warn!(
                        collection = %collection,
                        field = %field,
                        status = response.status,
                        "Similharity bridge: payload index creation failed",
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        collection = %collection,
                        field = %field,
                        error = %error,
                        "Similharity bridge: payload index request failed",
                    );
                }
            }
        }
    }

    async fn count_points(
        &self,
        config: &QdrantConfig,
        collection: &str,
        filter: Option<Value>,
    ) -> BridgeResult<u64> {
        let mut body = json!({ "exact": true });
        if let Some(filter) = filter {
            body["filter"] = filter;
        } else {
            body["filter"] = tt_domain::models::similharity::build_filter(&Map::new());
        }

        let result = engine(
            self.qdrant_repository
                .count_points(config, collection, body)
                .await?,
        )?;
        Ok(result["result"]["count"].as_u64().unwrap_or(0))
    }

    async fn collection_dimension(
        &self,
        config: &QdrantConfig,
        collection: &str,
    ) -> BridgeResult<u64> {
        let info = engine(
            self.qdrant_repository
                .collection_info(config, collection)
                .await?,
        )?;
        let vectors = &info["result"]["config"]["params"]["vectors"];
        let size = vectors["size"]
            .as_u64()
            .or_else(|| vectors.as_object().and_then(|map| map.values().next()?.get("size")?.as_u64()));
        Ok(size.unwrap_or(0))
    }

    /// Formula queries require Qdrant 1.13+; the version comes from `GET /`.
    async fn supports_formula(&self, config: &QdrantConfig) -> BridgeResult<bool> {
        let response = self.qdrant_repository.probe(config).await?;
        if response.status >= 400 {
            return Ok(false);
        }
        let Some(version) = response.body["version"].as_str() else {
            return Ok(false);
        };
        let mut parts = version.split('.');
        let major = parts.next().and_then(|part| part.parse::<u64>().ok());
        let minor = parts.next().and_then(|part| part.parse::<u64>().ok());
        let (Some(major), Some(minor)) = (major, minor) else {
            return Ok(false);
        };
        Ok((major, minor) >= (FORMULA_MIN_MAJOR, FORMULA_MIN_MINOR))
    }
}

fn health_body() -> Value {
    json!({
        "status": "ok",
        "plugin": "similharity",
        "version": PLUGIN_VERSION,
        "backends": ["vectra", "qdrant"],
    })
}

fn json_response(body: Value) -> SimilharityResponseDto {
    json_status(200, body)
}

fn json_status(status: u16, body: Value) -> SimilharityResponseDto {
    SimilharityResponseDto {
        status,
        kind: SimilharityResponseKindDto::Json,
        body,
    }
}

/// Treat Qdrant's error statuses as a forwarded answer rather than a host error.
fn engine(response: QdrantResponse) -> BridgeResult<Value> {
    if response.status >= 400 {
        return Err(BridgeError::Engine {
            status: response.status,
            message: qdrant_error_message(&response.body),
        });
    }
    Ok(response.body)
}

fn qdrant_error_message(body: &Value) -> String {
    let message = body["status"]["error"]
        .as_str()
        .or_else(|| body["error"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| body.to_string());
    message.chars().take(1_000).collect()
}

fn collection_names(listed: &Value) -> Vec<String> {
    listed["result"]["collections"]
        .as_array()
        .map(|collections| {
            collections
                .iter()
                .filter_map(|collection| collection["name"].as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_source(collection: &str) -> (String, String) {
    tt_domain::models::similharity::parse_collection_source(collection)
        .unwrap_or_else(|| ("unknown".to_string(), collection.to_string()))
}

fn require_collection(request: &SimilharityRequestDto) -> BridgeResult<String> {
    let collection = normalize_collection_name(request.collection_id());
    if collection.is_empty() {
        return Err(BridgeError::Application(ApplicationError::ValidationError(
            "collectionId is required".to_string(),
        )));
    }
    Ok(collection)
}

fn require_text(request: &SimilharityRequestDto) -> BridgeResult<String> {
    let text = request.text.clone().unwrap_or_default();
    if text.is_empty() {
        return Err(BridgeError::Application(ApplicationError::ValidationError(
            "text is required".to_string(),
        )));
    }
    Ok(text)
}

/// Qdrant point ids are integers; the extension sometimes sends numeric strings.
fn parse_hash(value: &Value) -> BridgeResult<i64> {
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    if let Some(text) = value.as_str() {
        return parse_hash_str(text);
    }
    Err(BridgeError::Application(ApplicationError::ValidationError(
        format!("Invalid chunk hash: {value}"),
    )))
}

fn parse_hash_str(value: &str) -> BridgeResult<i64> {
    value.trim().parse::<i64>().map_err(|_| {
        BridgeError::Application(ApplicationError::ValidationError(format!(
            "Invalid chunk hash: {value}"
        )))
    })
}

fn tenant_fields(
    filters: &Map<String, Value>,
    request: &SimilharityRequestDto,
) -> TenantFields {
    let from_filters = |key: &str, fallback: &str| {
        filters
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
            .to_string()
    };
    let embedding_source = if request.source().is_empty() {
        from_filters("embeddingSource", "transformers")
    } else {
        request.source().to_string()
    };

    TenantFields {
        content_type: from_filters("type", DEFAULT_TENANT_TYPE),
        source_id: from_filters("sourceId", "unknown"),
        embedding_source,
        embedding_model: request.model().to_string(),
    }
}

fn embedding_request(request: &SimilharityRequestDto) -> VectorRouteRequestDto {
    VectorRouteRequestDto {
        source: request.source().to_string(),
        model: request.model().to_string(),
        api_url: request.api_url.clone().unwrap_or_default(),
        extras_url: request.extras_url.clone().unwrap_or_default(),
        extras_key: request.extras_key.clone().unwrap_or_default(),
        keep: request.keep.unwrap_or(false),
        ..VectorRouteRequestDto::default()
    }
}

fn merged_options(request: &SimilharityRequestDto) -> Map<String, Value> {
    let mut options = Map::new();
    for source in [request.options.as_ref(), request.hybrid_options.as_ref()] {
        if let Some(Value::Object(map)) = source {
            options.extend(map.clone());
        }
    }
    options
}

/// Shape one Qdrant point into the extension's chunk contract.
fn shape_chunk(point: &Value) -> Value {
    let payload = point["payload"].as_object().cloned().unwrap_or_default();
    let index = payload
        .get("messageIndex")
        .and_then(Value::as_i64)
        .or_else(|| payload.get("source_window_end").and_then(Value::as_i64))
        .or_else(|| payload.get("source_window_start").and_then(Value::as_i64))
        .or_else(|| payload.get("startIndex").and_then(Value::as_i64));

    let mut chunk = json!({
        "hash": payload.get("hash").cloned().unwrap_or_else(|| point["id"].clone()),
        "text": payload.get("text").cloned().unwrap_or(Value::Null),
        "index": index.map_or(Value::Null, |index| json!(index)),
        "metadata": Value::Object(payload),
    });
    if !point["vector"].is_null() {
        chunk["vector"] = point["vector"].clone();
    }
    chunk
}

/// `/chunks/query` returns a bare array of scored points; `/chunks/hybrid-*`
/// returns the `points` array from a query result. Both map to the same shape.
fn shape_scored_results(result: &Value, native_sparse: bool) -> Vec<Value> {
    result
        .as_array()
        .map(|points| {
            points
                .iter()
                .map(|point| {
                    let payload = point["payload"].as_object().cloned().unwrap_or_default();
                    let mut entry = json!({
                        "hash": payload.get("hash").cloned().unwrap_or_else(|| point["id"].clone()),
                        "text": payload.get("text").cloned().unwrap_or(Value::Null),
                        "score": point["score"].clone(),
                        "metadata": Value::Object(payload),
                    });
                    if native_sparse {
                        entry["nativeSparse"] = json!(true);
                        entry["fusionMethod"] = json!("rrf");
                    }
                    entry
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mask_secret(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    if characters.len() <= 4 {
        return "****".to_string();
    }
    let visible = characters[characters.len() - 4..].iter().collect::<String>();
    format!("****{visible}")
}

#[cfg(test)]
mod tests;
