use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use tt_domain::errors::DomainError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalEmbeddingModel {
    #[default]
    Qwen3Embedding06B,
}

impl LocalEmbeddingModel {
    pub const SUPPORTED: [Self; 1] = [Self::Qwen3Embedding06B];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Qwen3Embedding06B => "Qwen/Qwen3-Embedding-0.6B",
        }
    }

    pub const fn profile(self) -> &'static str {
        match self {
            Self::Qwen3Embedding06B => "Qwen/Qwen3-Embedding-0.6B|f32-v1|max=8192|query-prompt-v1",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Self::SUPPORTED
            .into_iter()
            .find(|model| model.id() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorScope {
    pub collection_id: String,
    pub source: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorMetadata {
    pub hash: i64,
    pub text: String,
    pub index: i64,
    /// The floor this record was published at, for channels that have one.
    ///
    /// Absent until the chat message that carries the version is saved: the
    /// backend publishes a state version before the frontend knows which floor
    /// it landed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<i64>,
    /// The state key a state-history record belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_key: Option<String>,
    /// What kind of object the record is: `fact`, `event`, `summary`, `chunk`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorRecord {
    pub metadata: VectorMetadata,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub metadata: VectorMetadata,
    pub score: f32,
}

#[async_trait]
pub trait VectorRepository: Send + Sync {
    async fn list_hashes(&self, scope: &VectorScope) -> Result<Vec<i64>, DomainError>;

    async fn upsert(
        &self,
        scope: &VectorScope,
        records: Vec<VectorRecord>,
    ) -> Result<(), DomainError>;

    async fn delete_hashes(&self, scope: &VectorScope, hashes: &[i64]) -> Result<(), DomainError>;

    async fn query(
        &self,
        scope: &VectorScope,
        embedding: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<VectorMatch>, DomainError>;

    async fn purge_collection(&self, collection_id: &str) -> Result<(), DomainError>;

    async fn purge_all(&self) -> Result<(), DomainError>;
}

pub struct RemoteEmbeddingRequest {
    pub protocol: RemoteEmbeddingProtocol,
    pub texts: Vec<String>,
    pub is_query: bool,
}

pub struct RemoteEmbeddingBatch {
    pub embeddings: Vec<Vec<f32>>,
    pub reported_model: Option<String>,
}

pub struct LocalEmbeddingRequest {
    pub model: LocalEmbeddingModel,
    pub texts: Vec<String>,
    pub is_query: bool,
}

#[derive(Clone)]
pub enum RemoteEmbeddingProtocol {
    OpenAi {
        provider: String,
        base_url: String,
        api_key: String,
        model: String,
        omit_model: bool,
        headers: HashMap<String, String>,
    },
    Cohere {
        api_key: String,
        model: String,
    },
    Nomic {
        api_key: String,
    },
    Extras {
        base_url: String,
        api_key: Option<String>,
    },
    GoogleAiStudio {
        api_key: String,
        model: String,
    },
    VertexAi {
        model: String,
        region: String,
        auth: VertexEmbeddingAuth,
    },
    Ollama {
        base_url: String,
        model: String,
        keep: bool,
    },
    LlamaCpp {
        base_url: String,
        api_key: Option<String>,
    },
    Vllm {
        base_url: String,
        api_key: Option<String>,
        model: String,
    },
    KoboldCpp {
        base_url: String,
        api_key: Option<String>,
    },
}

#[derive(Clone)]
pub enum VertexEmbeddingAuth {
    Express {
        api_key: String,
        project_id: Option<String>,
    },
    ServiceAccount {
        json: String,
    },
}

#[async_trait]
pub trait RemoteEmbeddingRepository: Send + Sync {
    async fn embed(
        &self,
        request: RemoteEmbeddingRequest,
    ) -> Result<RemoteEmbeddingBatch, DomainError>;
}

#[async_trait]
pub trait LocalEmbeddingRepository: Send + Sync {
    async fn embed(&self, request: LocalEmbeddingRequest) -> Result<Vec<Vec<f32>>, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_embedding_ids_round_trip_and_profiles_are_distinct() {
        let profiles = LocalEmbeddingModel::SUPPORTED.map(|model| {
            assert_eq!(LocalEmbeddingModel::from_id(model.id()), Some(model));
            model.profile()
        });

        for (index, profile) in profiles.iter().enumerate() {
            assert!(!profiles[..index].contains(profile));
        }
        assert!(LocalEmbeddingModel::from_id("unknown").is_none());
    }

    #[test]
    fn metadata_stored_before_the_recall_fields_still_decodes() {
        let decoded: VectorMetadata =
            serde_json::from_str(r#"{"hash":7,"text":"a fact","index":3}"#)
                .expect("a database written by an older version must keep decoding");

        assert_eq!(decoded.hash, 7);
        assert!(decoded.floor.is_none(), "a floor that was never bound is absent");
        assert!(decoded.field_key.is_none());
        assert!(decoded.kind.is_none());
    }

    #[test]
    fn recall_fields_round_trip_without_changing_other_records() {
        let fact = VectorMetadata {
            hash: 1,
            text: "时间 (环境/时间): 夜晚".to_string(),
            index: 2,
            floor: Some(9),
            field_key: Some("环境/时间".to_string()),
            kind: Some("fact".to_string()),
        };
        let encoded = serde_json::to_string(&fact).expect("a fact must encode");
        assert_eq!(
            serde_json::from_str::<VectorMetadata>(&encoded).expect("a fact must decode"),
            fact
        );

        let chunk = VectorMetadata {
            hash: 1,
            text: "t".to_string(),
            index: 2,
            floor: None,
            field_key: None,
            kind: None,
        };
        assert_eq!(
            serde_json::to_string(&chunk).expect("a chunk must encode"),
            r#"{"hash":1,"text":"t","index":2}"#,
            "a record written for the vectors extension keeps the wire shape it already had"
        );
    }
}
