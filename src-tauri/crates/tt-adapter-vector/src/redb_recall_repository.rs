//! The recall half of the redb vector index.
//!
//! Same file, same scope prefixes, one extra table: an index that already
//! stores embeddings is exactly where "which floor did this fact come from" and
//! "how big is this index" belong. Sharing the database handle is not a
//! preference — redb holds an exclusive lock on the file, so a second
//! `Database` over the same path would refuse to open.

use std::collections::HashMap;

use async_trait::async_trait;
use redb::{ReadableDatabase, ReadableTable};
use tt_domain::errors::DomainError;
use tt_ports::repositories::recall_repository::{
    RecallRepository, RecallScopeMeta, StateFloorBinding,
};
use tt_ports::repositories::vector_repository::{VectorMetadata, VectorScope};

use super::redb_vector_repository::{
    METADATA, RedbVectorRepository, SCOPE_META, SCOPES, prefix_end, scope_prefix, storage_error,
};

#[async_trait]
impl RecallRepository for RedbVectorRepository {
    async fn list_metadata(&self, scope: &VectorScope) -> Result<Vec<VectorMetadata>, DomainError> {
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_read()
                .map_err(|error| storage_error("read recall records", error))?;
            let table = transaction
                .open_table(METADATA)
                .map_err(|error| storage_error("open recall records", error))?;

            let mut records: Vec<VectorMetadata> = Vec::new();
            let end = prefix_end(&prefix);
            for entry in table
                .range(prefix.as_str()..end.as_str())
                .map_err(|error| storage_error("scan recall records", error))?
            {
                let (_, value) =
                    entry.map_err(|error| storage_error("read recall record", error))?;
                records.push(
                    serde_json::from_slice(value.value())
                        .map_err(|error| storage_error("decode recall record", error))?,
                );
            }
            Ok(records)
        })
        .await
    }

    async fn bind_state_floors(
        &self,
        scope: &VectorScope,
        bindings: &[StateFloorBinding],
    ) -> Result<usize, DomainError> {
        if bindings.is_empty() {
            return Ok(0);
        }
        let floors = resolve_bindings(bindings)?;
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin floor binding", error))?;
            let mut updated = 0_usize;
            {
                let mut table = transaction
                    .open_table(METADATA)
                    .map_err(|error| storage_error("open recall records", error))?;

                // A table cannot be iterated and written at the same time, so
                // the records to rewrite are collected before any of them land.
                let mut rewrites = Vec::new();
                let end = prefix_end(&prefix);
                for entry in table
                    .range(prefix.as_str()..end.as_str())
                    .map_err(|error| storage_error("scan recall records", error))?
                {
                    let (key, value) =
                        entry.map_err(|error| storage_error("read recall record", error))?;
                    let mut metadata: VectorMetadata = serde_json::from_slice(value.value())
                        .map_err(|error| storage_error("decode recall record", error))?;
                    let Some(floor) = floors.get(&metadata.index) else {
                        continue;
                    };
                    // Binding the same version to the floor it already has is
                    // what a repeated backfill does, and it is not a change.
                    if metadata.floor == Some(*floor) {
                        continue;
                    }
                    metadata.floor = Some(*floor);
                    rewrites.push((key.value().to_string(), metadata));
                }

                for (key, metadata) in rewrites {
                    let encoded = serde_json::to_vec(&metadata)
                        .map_err(|error| storage_error("encode recall record", error))?;
                    table
                        .insert(key.as_str(), encoded.as_slice())
                        .map_err(|error| storage_error("store recall record", error))?;
                    updated += 1;
                }
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit floor binding", error))?;
            Ok(updated)
        })
        .await
    }

    async fn read_scope_meta(
        &self,
        scope: &VectorScope,
    ) -> Result<Option<RecallScopeMeta>, DomainError> {
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_read()
                .map_err(|error| storage_error("read recall scope metadata", error))?;
            let table = transaction
                .open_table(SCOPE_META)
                .map_err(|error| storage_error("open recall scope metadata", error))?;
            let Some(value) = table
                .get(prefix.as_str())
                .map_err(|error| storage_error("read recall scope metadata", error))?
            else {
                return Ok(None);
            };
            serde_json::from_slice(value.value())
                .map(Some)
                .map_err(|error| storage_error("decode recall scope metadata", error))
        })
        .await
    }

    async fn write_scope_meta(
        &self,
        scope: &VectorScope,
        meta: &RecallScopeMeta,
    ) -> Result<(), DomainError> {
        let prefix = scope_prefix(scope);
        let dims = meta.dims;
        let encoded = serde_json::to_vec(meta)
            .map_err(|error| storage_error("encode recall scope metadata", error))?;
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin recall scope metadata write", error))?;
            {
                // The dimension lives here and in `scopes_v1`, and one path
                // writes both. A disagreement means the scope now holds two
                // vector spaces, which has to be reported rather than read
                // across.
                let scopes = transaction
                    .open_table(SCOPES)
                    .map_err(|error| storage_error("open vector scopes", error))?;
                let stored_dimension = scopes
                    .get(prefix.as_str())
                    .map_err(|error| storage_error("read vector dimension", error))?;
                if let Some(stored) = stored_dimension
                    && stored.value() != dims
                {
                    return Err(DomainError::InvalidData(format!(
                        "Recall index dimension disagrees with the stored scope: stored {}, received {dims}",
                        stored.value(),
                    )));
                }

                let mut table = transaction
                    .open_table(SCOPE_META)
                    .map_err(|error| storage_error("open recall scope metadata", error))?;
                table
                    .insert(prefix.as_str(), encoded.as_slice())
                    .map_err(|error| storage_error("store recall scope metadata", error))?;
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit recall scope metadata", error))?;
            Ok(())
        })
        .await
    }
}

/// Turn the requested bindings into one version-index lookup.
///
/// A version bound to two floors would make the answer depend on the order the
/// frontend happened to walk its messages in, so it is refused instead.
fn resolve_bindings(
    bindings: &[StateFloorBinding],
) -> Result<HashMap<i64, i64>, DomainError> {
    let mut floors = HashMap::with_capacity(bindings.len());
    for binding in bindings {
        match floors.insert(binding.version_index, binding.floor) {
            Some(previous) if previous == binding.floor => {}
            Some(previous) => {
                return Err(DomainError::InvalidData(format!(
                    "State version index {} is bound to two floors ({previous} and {})",
                    binding.version_index, binding.floor
                )));
            }
            None => {}
        }
    }
    Ok(floors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tt_ports::repositories::vector_repository::{VectorRecord, VectorRepository};
    use uuid::Uuid;

    struct TempDatabase(std::path::PathBuf);

    impl TempDatabase {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("tauritavern-recall-{}", Uuid::new_v4()));
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

    fn scope() -> VectorScope {
        VectorScope {
            collection_id: "sthist:chat-a".to_string(),
            source: "state-history".to_string(),
            profile: "test-profile".to_string(),
        }
    }

    fn fact(hash: i64, version_index: i64, text: &str) -> VectorRecord {
        VectorRecord {
            metadata: VectorMetadata {
                hash,
                text: text.to_string(),
                index: version_index,
                floor: None,
                field_key: Some(text.to_string()),
                kind: Some("fact".to_string()),
            },
            embedding: vec![1.0, 0.0],
        }
    }

    fn meta(dims: u32, doc_count: u64) -> RecallScopeMeta {
        RecallScopeMeta {
            dims,
            model_profile: "test-profile".to_string(),
            doc_count,
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn binding_a_version_only_touches_its_own_records_and_is_idempotent() {
        let temp = TempDatabase::new();
        let repository = RedbVectorRepository::new(temp.0.clone());
        let scope = scope();
        repository
            .upsert(
                &scope,
                vec![
                    fact(1, 100, "环境/时间"),
                    fact(2, 100, "环境/地点"),
                    fact(3, 200, "环境/天气"),
                ],
            )
            .await
            .expect("facts must be stored");

        let updated = repository
            .bind_state_floors(
                &scope,
                &[StateFloorBinding {
                    version_index: 100,
                    floor: 5,
                }],
            )
            .await
            .expect("binding must succeed");
        assert_eq!(updated, 2, "only the records of that version move");

        let mut floors = repository
            .list_metadata(&scope)
            .await
            .expect("records must be readable")
            .into_iter()
            .map(|metadata| (metadata.index, metadata.floor))
            .collect::<Vec<_>>();
        floors.sort();
        assert_eq!(floors, vec![(100, Some(5)), (100, Some(5)), (200, None)]);

        assert_eq!(
            repository
                .bind_state_floors(
                    &scope,
                    &[StateFloorBinding {
                        version_index: 100,
                        floor: 5,
                    }],
                )
                .await
                .expect("a repeated binding must be accepted"),
            0,
            "a backfill that runs again must not rewrite what it already bound"
        );
    }

    #[tokio::test]
    async fn one_version_bound_to_two_floors_is_refused() {
        let temp = TempDatabase::new();
        let repository = RedbVectorRepository::new(temp.0.clone());
        let scope = scope();
        repository
            .upsert(&scope, vec![fact(1, 100, "环境/时间")])
            .await
            .expect("facts must be stored");

        let error = repository
            .bind_state_floors(
                &scope,
                &[
                    StateFloorBinding {
                        version_index: 100,
                        floor: 5,
                    },
                    StateFloorBinding {
                        version_index: 100,
                        floor: 6,
                    },
                ],
            )
            .await
            .expect_err("one version cannot belong to two floors");

        assert!(matches!(error, DomainError::InvalidData(_)));
        assert_eq!(
            repository.list_metadata(&scope).await.expect("records")[0].floor,
            None,
            "a refused batch must not bind anything"
        );
    }

    #[tokio::test]
    async fn scope_metadata_survives_a_reopen_and_checks_the_stored_dimension() {
        let temp = TempDatabase::new();
        let repository = RedbVectorRepository::new(temp.0.clone());
        let scope = scope();
        assert!(
            repository
                .read_scope_meta(&scope)
                .await
                .expect("reading a fresh index must not fail")
                .is_none(),
            "an index nothing has been written to has no metadata yet"
        );

        repository
            .upsert(&scope, vec![fact(1, 100, "环境/时间")])
            .await
            .expect("facts must be stored");
        let written = meta(2, 1);
        repository
            .write_scope_meta(&scope, &written)
            .await
            .expect("metadata must be stored");

        assert!(
            repository.write_scope_meta(&scope, &meta(3, 1)).await.is_err(),
            "a second dimension in one scope must be refused, not averaged"
        );

        drop(repository);
        let reopened = RedbVectorRepository::new(temp.0.clone());
        assert_eq!(
            reopened
                .read_scope_meta(&scope)
                .await
                .expect("metadata must be readable after a reopen"),
            Some(written)
        );
    }
}
