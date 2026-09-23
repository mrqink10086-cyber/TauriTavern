//! Indexing the facts a published state version carries.
//!
//! The state system already produces, once per floor, exactly the object a
//! retrieval index wants: a small set of model-written, declaration-checked
//! facts. This service turns them into vectors — and only the ones that
//! changed, because a floor usually moves a handful of fields.
//!
//! It is deliberately not part of the run's correctness: publication happens
//! first and cannot be rolled back, so an indexing failure is reported and the
//! run continues. The index is derived, and the backfill path rebuilds it.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;

use tt_domain::models::recall::RecallChannelId;
use tt_domain::models::recall_fact::{
    FACT_KIND, RecallFact, render_fact_sentences, state_version_index,
};
use tt_domain::models::state::{StateDeclaration, StateDocument};
use tt_ports::repositories::recall_repository::{
    RecallRepository, RecallScopeMeta, StateFloorBinding,
};
use tt_ports::repositories::vector_repository::{
    LocalEmbeddingModel, LocalEmbeddingRepository, LocalEmbeddingRequest, VectorMetadata,
    VectorRecord, VectorRepository, VectorScope,
};

use crate::errors::ApplicationError;
use crate::services::vector_service::normalize_embeddings;

/// The scope one chat's state-history index lives in.
///
/// The scope identity is `collection + source + profile`. Changing the
/// embedding model therefore moves to a different scope rather than mixing two
/// vector spaces, and the collection name is the channel's own id so the
/// storage key cannot drift from the channel it belongs to.
pub fn state_history_scope(stable_chat_id: &str) -> VectorScope {
    let channel = RecallChannelId::StateHistory;
    VectorScope {
        collection_id: format!("{}:{stable_chat_id}", channel.as_str()),
        source: channel.as_str().to_string(),
        profile: LocalEmbeddingModel::default().profile().to_string(),
    }
}

/// A published state version, ready to be indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedState {
    pub stable_chat_id: String,
    pub state_id: String,
    pub document: StateDocument,
    pub declaration: StateDeclaration,
}

/// Which floor one published state version landed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFloorAssignment {
    pub state_id: String,
    pub floor: i64,
}

/// What one published state version did to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallIndexReport {
    pub embedded: usize,
    pub unchanged: usize,
}

pub struct RecallService {
    recall_repository: Arc<dyn RecallRepository>,
    vector_repository: Arc<dyn VectorRepository>,
    local_embedding_repository: Arc<dyn LocalEmbeddingRepository>,
}

impl RecallService {
    pub fn new(
        recall_repository: Arc<dyn RecallRepository>,
        vector_repository: Arc<dyn VectorRepository>,
        local_embedding_repository: Arc<dyn LocalEmbeddingRepository>,
    ) -> Self {
        Self {
            recall_repository,
            vector_repository,
            local_embedding_repository,
        }
    }

    /// Index the facts one published state version carries.
    ///
    /// Incremental by content hash: a fact already indexed under the same
    /// embedding profile is not embedded again, so the steady-state cost of a
    /// floor is the fields that actually moved. A field that went back to an
    /// earlier value is not written again either — the index records where a
    /// fact *appeared*, which is the granularity an evolution query wants.
    pub async fn index_published_state(
        &self,
        published: &PublishedState,
    ) -> Result<RecallIndexReport, ApplicationError> {
        let scope = state_history_scope(&published.stable_chat_id);
        let facts =
            render_fact_sentences(&published.document, &published.declaration, &scope.profile);

        let records = self.recall_repository.list_metadata(&scope).await?;
        let indexed = records
            .iter()
            .filter_map(|metadata| Some((metadata.field_key.clone()?, metadata.hash)))
            .collect::<HashSet<_>>();

        let pending = facts
            .iter()
            .filter(|fact| !indexed.contains(&(fact.field_key.clone(), fact.hash)))
            .collect::<Vec<&RecallFact>>();
        let unchanged = facts.len() - pending.len();

        if pending.is_empty() {
            return Ok(RecallIndexReport {
                embedded: 0,
                unchanged,
            });
        }

        let embeddings = self
            .embed_documents(pending.iter().map(|fact| fact.sentence.clone()).collect())
            .await?;
        let dims = u32::try_from(embeddings.first().map_or(0, Vec::len)).map_err(|_| {
            ApplicationError::InternalError(
                "recall.embedding_dimension_overflow: the local model returned an unusable dimension"
                    .to_string(),
            )
        })?;

        let version_index = state_version_index(&published.state_id);
        let new_records = pending
            .iter()
            .zip(embeddings)
            .map(|(fact, embedding)| VectorRecord {
                metadata: VectorMetadata {
                    hash: fact.hash,
                    text: fact.sentence.clone(),
                    index: version_index,
                    // The floor is not known until the frontend saves the
                    // message that carries this version.
                    floor: None,
                    field_key: Some(fact.field_key.clone()),
                    kind: Some(FACT_KIND.to_string()),
                },
                embedding,
            })
            .collect::<Vec<_>>();
        self.vector_repository.upsert(&scope, new_records).await?;

        // The scope's dimension and size are recorded on the same path that
        // writes the vectors, so the two can only disagree if something else
        // wrote into this scope.
        self.recall_repository
            .write_scope_meta(
                &scope,
                &RecallScopeMeta {
                    dims,
                    model_profile: scope.profile.clone(),
                    doc_count: (records.len() + pending.len()) as u64,
                    updated_at: Utc::now(),
                },
            )
            .await?;

        Ok(RecallIndexReport {
            embedded: pending.len(),
            unchanged,
        })
    }

    /// Point the records of one or more state versions at their floors.
    ///
    /// One call covers both the message that was just saved and a whole-chat
    /// backfill: they are the same operation, and a repeated binding is a no-op
    /// in storage.
    pub async fn bind_state_floors(
        &self,
        stable_chat_id: &str,
        assignments: &[StateFloorAssignment],
    ) -> Result<usize, ApplicationError> {
        if assignments.is_empty() {
            return Ok(0);
        }
        let scope = state_history_scope(stable_chat_id);
        let bindings = assignments
            .iter()
            .map(|assignment| StateFloorBinding {
                version_index: state_version_index(&assignment.state_id),
                floor: assignment.floor,
            })
            .collect::<Vec<_>>();
        Ok(self
            .recall_repository
            .bind_state_floors(&scope, &bindings)
            .await?)
    }

    /// Embed document text with the local model.
    ///
    /// Normalization is not repeated here: the vectors extension and recall
    /// must produce comparable vectors, and two normalizers would be two
    /// definitions of "cosine".
    async fn embed_documents(
        &self,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, ApplicationError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let expected_count = texts.len();
        let embeddings = self
            .local_embedding_repository
            .embed(LocalEmbeddingRequest {
                model: LocalEmbeddingModel::default(),
                texts,
                is_query: false,
            })
            .await?;
        normalize_embeddings(embeddings, expected_count)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use tt_domain::errors::DomainError;
    use tt_ports::repositories::vector_repository::{VectorMatch, VectorRecord};

    use super::*;

    /// One in-memory index standing in for the redb one, serving both ports as
    /// the real adapter does.
    #[derive(Default)]
    struct FakeIndex {
        records: Mutex<Vec<VectorRecord>>,
        meta: Mutex<Option<RecallScopeMeta>>,
    }

    impl FakeIndex {
        fn metadata(&self) -> Vec<VectorMetadata> {
            self.records
                .lock()
                .expect("records")
                .iter()
                .map(|record| record.metadata.clone())
                .collect()
        }
    }

    #[async_trait]
    impl VectorRepository for FakeIndex {
        async fn list_hashes(&self, _scope: &VectorScope) -> Result<Vec<i64>, DomainError> {
            Ok(self
                .records
                .lock()
                .expect("records")
                .iter()
                .map(|record| record.metadata.hash)
                .collect())
        }

        async fn upsert(
            &self,
            _scope: &VectorScope,
            records: Vec<VectorRecord>,
        ) -> Result<(), DomainError> {
            let mut stored = self.records.lock().expect("records");
            for record in records {
                let position = stored.iter().position(|existing| {
                    existing.metadata.hash == record.metadata.hash
                        && existing.metadata.index == record.metadata.index
                        && existing.metadata.text == record.metadata.text
                });
                match position {
                    Some(index) => stored[index] = record,
                    None => stored.push(record),
                }
            }
            Ok(())
        }

        async fn delete_hashes(
            &self,
            _scope: &VectorScope,
            _hashes: &[i64],
        ) -> Result<(), DomainError> {
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

    #[async_trait]
    impl RecallRepository for FakeIndex {
        async fn list_metadata(
            &self,
            _scope: &VectorScope,
        ) -> Result<Vec<VectorMetadata>, DomainError> {
            Ok(self.metadata())
        }

        async fn bind_state_floors(
            &self,
            _scope: &VectorScope,
            bindings: &[StateFloorBinding],
        ) -> Result<usize, DomainError> {
            let mut stored = self.records.lock().expect("records");
            let mut updated = 0;
            for record in stored.iter_mut() {
                let Some(binding) = bindings
                    .iter()
                    .find(|binding| binding.version_index == record.metadata.index)
                else {
                    continue;
                };
                if record.metadata.floor == Some(binding.floor) {
                    continue;
                }
                record.metadata.floor = Some(binding.floor);
                updated += 1;
            }
            Ok(updated)
        }

        async fn read_scope_meta(
            &self,
            _scope: &VectorScope,
        ) -> Result<Option<RecallScopeMeta>, DomainError> {
            Ok(self.meta.lock().expect("meta").clone())
        }

        async fn write_scope_meta(
            &self,
            _scope: &VectorScope,
            meta: &RecallScopeMeta,
        ) -> Result<(), DomainError> {
            *self.meta.lock().expect("meta") = Some(meta.clone());
            Ok(())
        }
    }

    /// Records which texts were embedded, so a test can assert that unchanged
    /// facts are never sent to the model again.
    #[derive(Default)]
    struct FakeEmbeddings {
        calls: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl LocalEmbeddingRepository for FakeEmbeddings {
        async fn embed(
            &self,
            request: LocalEmbeddingRequest,
        ) -> Result<Vec<Vec<f32>>, DomainError> {
            self.calls.lock().expect("calls").push(request.texts.clone());
            Ok(request
                .texts
                .iter()
                .map(|_| vec![1.0_f32, 0.0_f32])
                .collect())
        }
    }

    fn document(fields: &[(&str, &[&str])]) -> StateDocument {
        StateDocument {
            fields: fields
                .iter()
                .map(|(key, values)| tt_domain::models::state::StateField {
                    key: tt_domain::models::state::StateKey::parse(key)
                        .unwrap_or_else(|error| panic!("`{key}` must parse: {error}")),
                    values: values.iter().map(|value| value.to_string()).collect(),
                })
                .collect(),
        }
    }

    fn published(state_id: &str, fields: &[(&str, &[&str])]) -> PublishedState {
        PublishedState {
            stable_chat_id: "chat-a".to_string(),
            state_id: state_id.to_string(),
            document: document(fields),
            declaration: StateDeclaration::default(),
        }
    }

    fn service() -> (RecallService, Arc<FakeIndex>, Arc<FakeEmbeddings>) {
        let index = Arc::new(FakeIndex::default());
        let embeddings = Arc::new(FakeEmbeddings::default());
        (
            RecallService::new(
                index.clone(),
                index.clone(),
                embeddings.clone(),
            ),
            index,
            embeddings,
        )
    }

    #[tokio::test]
    async fn a_first_publish_indexes_every_fact_and_records_the_index_size() {
        let (service, index, embeddings) = service();

        let report = service
            .index_published_state(&published(
                "v1",
                &[
                    ("环境/时间", &["夜晚"]),
                    ("环境/地点", &["咖啡馆"]),
                    ("环境/地点/备注", &[]),
                ],
            ))
            .await
            .expect("indexing must succeed");

        assert_eq!(report.embedded, 2, "a cleared field has no fact");
        assert_eq!(report.unchanged, 0);
        assert_eq!(
            embeddings.calls.lock().expect("calls").len(),
            1,
            "one batch, not one call per fact"
        );
        let mut keys = index
            .metadata()
            .into_iter()
            .map(|metadata| metadata.field_key.unwrap_or_default())
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, ["环境/地点", "环境/时间"]);
        assert!(
            index
                .metadata()
                .iter()
                .all(|metadata| metadata.kind.as_deref() == Some("fact")),
            "every record this service writes is a state fact"
        );
        assert_eq!(
            index.metadata()[0].floor,
            None,
            "the floor is bound by the frontend after the message is saved"
        );

        let meta = index
            .read_scope_meta(&state_history_scope("chat-a"))
            .await
            .expect("meta")
            .expect("a written scope must describe itself");
        assert_eq!(meta.dims, 2, "the dimension comes from the embeddings that were stored");
        assert_eq!(meta.doc_count, 2);
        assert_eq!(meta.model_profile, state_history_scope("chat-a").profile);
    }

    #[tokio::test]
    async fn a_second_publish_embeds_only_the_facts_that_moved() {
        let (service, index, embeddings) = service();
        service
            .index_published_state(&published(
                "v1",
                &[("环境/时间", &["夜晚"]), ("环境/地点", &["咖啡馆"])],
            ))
            .await
            .expect("indexing must succeed");
        embeddings.calls.lock().expect("calls").clear();

        let report = service
            .index_published_state(&published(
                "v2",
                &[("环境/时间", &["夜晚"]), ("环境/地点", &["图书馆"])],
            ))
            .await
            .expect("indexing must succeed");

        assert_eq!(report.embedded, 1);
        assert_eq!(report.unchanged, 1);
        assert_eq!(
            embeddings.calls.lock().expect("calls").as_slice(),
            &[vec!["地点 (环境/地点): 图书馆".to_string()]],
            "an unchanged fact must not be sent to the model again"
        );
        assert_eq!(index.metadata().len(), 3, "history keeps the earlier value");
    }

    #[tokio::test]
    async fn a_fact_that_returns_to_an_earlier_value_is_not_indexed_twice() {
        let (service, index, _embeddings) = service();
        service
            .index_published_state(&published("v1", &[("环境/时间", &["夜晚"])]))
            .await
            .expect("indexing must succeed");
        service
            .index_published_state(&published("v2", &[("环境/时间", &["清晨"])]))
            .await
            .expect("indexing must succeed");

        let report = service
            .index_published_state(&published("v3", &[("环境/时间", &["夜晚"])]))
            .await
            .expect("indexing must succeed");

        assert_eq!(report.embedded, 0);
        assert_eq!(report.unchanged, 1);
        assert_eq!(index.metadata().len(), 2);
    }

    #[tokio::test]
    async fn a_publish_with_no_facts_writes_nothing() {
        let (service, index, embeddings) = service();

        let report = service
            .index_published_state(&published("v1", &[("环境/地点", &[])]))
            .await
            .expect("an empty document is not a failure");

        assert_eq!(
            report,
            RecallIndexReport {
                embedded: 0,
                unchanged: 0
            }
        );
        assert!(embeddings.calls.lock().expect("calls").is_empty());
        assert!(index.metadata().is_empty());
    }

    #[tokio::test]
    async fn binding_floors_points_only_the_named_version_at_its_floor() {
        let (service, index, _embeddings) = service();
        service
            .index_published_state(&published("v1", &[("环境/时间", &["夜晚"])]))
            .await
            .expect("indexing must succeed");
        service
            .index_published_state(&published("v2", &[("环境/地点", &["咖啡馆"])]))
            .await
            .expect("indexing must succeed");

        let bound = service
            .bind_state_floors(
                "chat-a",
                &[StateFloorAssignment {
                    state_id: "v2".to_string(),
                    floor: 7,
                }],
            )
            .await
            .expect("binding must succeed");

        assert_eq!(bound, 1);
        let mut floors = index
            .metadata()
            .into_iter()
            .map(|metadata| metadata.floor)
            .collect::<Vec<_>>();
        floors.sort();
        assert_eq!(floors, vec![None, Some(7)]);
    }
}
