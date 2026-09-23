use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePath, WorkspacePersistentChangeSet,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub path: WorkspacePath,
    pub text: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceAppendResult {
    pub file: WorkspaceFile,
    pub previous_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub path: WorkspacePath,
    pub kind: WorkspaceEntryKind,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFileList {
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceWriteGuard {
    Unchecked,
    MustNotExist,
    MustMatchSha256(String),
}

/// One file a run-less write puts into a new persistent version.
///
/// A path inside the persistent roots, and the text to store there. Writes only
/// ever add or replace: a version always carries everything its base carried, so
/// no version can be read as a half-written one.
#[derive(Debug, Clone)]
pub struct PersistentFileWrite {
    pub path: WorkspacePath,
    pub text: String,
}

#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    /// Check the inherited version before creating a run or its workspace.
    async fn validate_persistent_state(
        &self,
        workspace_id: &str,
        state_id: &str,
    ) -> Result<(), DomainError>;

    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
        resolved_profile: &ResolvedAgentProfile,
    ) -> Result<(), DomainError>;

    /// Read one file of a published persistent state.
    ///
    /// Chat-scoped state has to be readable without a live run: the published
    /// version is the chat's current state, and the run that produced it may be
    /// long gone. Only files the version's manifest lists are readable, so a
    /// half-written version cannot be read around.
    async fn read_persistent_state_file(
        &self,
        workspace_id: &str,
        state_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError>;

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn write_text_guarded(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
        guard: WorkspaceWriteGuard,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn append_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError>;

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError>;

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError>;

    async fn commit_persistent_changes(
        &self,
        run_id: &str,
        previous_state_id: Option<&str>,
    ) -> Result<WorkspacePersistentChangeSet, DomainError>;

    /// Publish a new persistent version of a chat that no run stands behind.
    ///
    /// A user editing state is not a run, so this cannot go through
    /// `commit_persistent_changes`: it writes the given files on top of the
    /// version named by `base_state_id` and publishes the result the same way a
    /// run's completion does — a new `persistent-states/<state_id>/` with its own
    /// manifest, promoted atomically.
    ///
    /// An edit that changes nothing reuses the version it started from rather
    /// than publishing an empty one, so a repeated click is not a new floor.
    async fn publish_persistent_files(
        &self,
        workspace_id: &str,
        base_state_id: Option<&str>,
        files: &[PersistentFileWrite],
    ) -> Result<WorkspacePersistentChangeSet, DomainError>;
}
