//! Storage of first-party recall records.
//!
//! The vector port owns "a collection of embeddings"; this one owns the two
//! things only recall needs: what an index says about itself, and turning a
//! published state version into the floors it belongs to. Both are separate
//! from `VectorRepository` on purpose — the vectors extension must not have to
//! grow a sense of floors it never binds.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use tt_domain::errors::DomainError;

use super::vector_repository::{VectorMetadata, VectorScope};

/// What one index says about itself.
///
/// A scope whose stored dimension disagrees with the model writing into it
/// holds two vector spaces at once; that is a conflict to report, not something
/// to average over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallScopeMeta {
    pub dims: u32,
    /// The embedding profile the scope was built with.
    pub model_profile: String,
    pub doc_count: u64,
    pub updated_at: DateTime<Utc>,
}

/// Which floor one published state version landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateFloorBinding {
    /// [`tt_domain::models::recall_fact::state_version_index`] of the version.
    pub version_index: i64,
    pub floor: i64,
}

#[async_trait]
pub trait RecallRepository: Send + Sync {
    /// Every record in the scope, so a sync can diff against what is indexed.
    async fn list_metadata(&self, scope: &VectorScope) -> Result<Vec<VectorMetadata>, DomainError>;

    /// Point the records one or more state versions wrote at their floors.
    ///
    /// The batch shape is the operation itself rather than a convenience: a
    /// backfill walks a whole chat, and one transaction should cover it.
    /// Returns how many records changed; zero is a normal answer for a version
    /// whose facts were all already indexed.
    async fn bind_state_floors(
        &self,
        scope: &VectorScope,
        bindings: &[StateFloorBinding],
    ) -> Result<usize, DomainError>;

    async fn read_scope_meta(
        &self,
        scope: &VectorScope,
    ) -> Result<Option<RecallScopeMeta>, DomainError>;

    async fn write_scope_meta(
        &self,
        scope: &VectorScope,
        meta: &RecallScopeMeta,
    ) -> Result<(), DomainError>;
}
