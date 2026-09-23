use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use super::FileAgentRepository;
use super::fs_tree::{copy_directory_contents, scan_workspace_files, snapshot_map};
use super::paths::{PERSISTENT_STATES_DIR, validate_workspace_root_path};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{
    AgentRun, WorkspaceManifest, WorkspacePersistentChange, WorkspacePersistentChangeKind,
    WorkspacePersistentChangeSet, WorkspaceRootCommit, WorkspaceRootMount, WorkspaceRootScope,
};
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::workspace_repository::{
    PersistentFileWrite, WorkspaceRepository,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) base_state_id: Option<String>,
    pub(super) files: Vec<PersistentSnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentSnapshotFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistentStateManifest {
    version: u32,
    state_id: String,
    /// The run that published this version. Empty when a user edited state
    /// outside any run: there is no run to name, and inventing one would make
    /// the field lie about where a version came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_state_id: Option<String>,
    created_at: DateTime<Utc>,
    pub(super) files: Vec<PersistentSnapshotFile>,
    pub(super) changes: Vec<WorkspacePersistentChange>,
}

impl FileAgentRepository {
    pub(super) async fn initialize_projected_roots(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        run_dir: &Path,
    ) -> Result<PersistentSnapshot, DomainError> {
        let chat_dir = self.chat_dir(&run.workspace_id)?;
        fs::create_dir_all(&chat_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent chat workspace {}: {}",
                chat_dir.display(),
                error
            ))
        })?;

        let base_state = match run.persist_base_state_id.as_deref() {
            Some(state_id) => {
                let state_dir = self.persistent_state_dir(&run.workspace_id, state_id)?;
                let state_manifest = self
                    .read_persistent_state_manifest(&state_dir, state_id)
                    .await?;
                Some((state_dir, state_manifest))
            }
            None => None,
        };

        let mut files = Vec::new();
        for root in persistent_roots(manifest)? {
            let run_root = run_dir.join(&root);
            fs::create_dir_all(&run_root).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create projected persistent root {}: {}",
                    run_root.display(),
                    error
                ))
            })?;
            if let Some((base_state_dir, base_state_manifest)) = base_state.as_ref() {
                let base_root = base_state_dir.join(&root);
                let metadata = match fs::symlink_metadata(&base_root).await {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if !persistent_manifest_has_files_for_root(base_state_manifest, &root) {
                            continue;
                        }
                        return Err(DomainError::InvalidData(format!(
                            "agent.persistent_state_root_missing: state `{}` is missing root `{root}`",
                            run.persist_base_state_id.as_deref().unwrap_or_default()
                        )));
                    }
                    Err(error) => {
                        return Err(DomainError::InternalError(format!(
                            "Failed to inspect persistent state root {}: {}",
                            base_root.display(),
                            error
                        )));
                    }
                };
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(DomainError::InvalidData(format!(
                        "agent.persistent_state_root_invalid: {}",
                        base_root.display()
                    )));
                }
                copy_directory_contents(&base_root, &run_root).await?;
            }
            files.extend(scan_workspace_files(&run_root, &root).await?);
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(PersistentSnapshot {
            base_state_id: run.persist_base_state_id.clone(),
            files,
        })
    }

    pub(super) async fn compute_persistent_changes(
        &self,
        run_id: &str,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        let manifest = self.read_manifest(run_id).await?;
        let roots = persistent_roots(&manifest)?;
        let run_dir = self.run_dir(&run)?;
        let base_snapshot: PersistentSnapshot =
            Self::read_json(&run_dir.join("input").join("persist_snapshot.json")).await?;

        let base = snapshot_map(base_snapshot.files);
        let mut overlay = BTreeMap::new();

        for root in roots {
            overlay.extend(snapshot_map(
                scan_workspace_files(&run_dir.join(&root), &root).await?,
            ));
        }

        let mut changes = Vec::new();
        for (path, overlay_file) in &overlay {
            match base.get(path) {
                Some(base_file) if base_file.sha256 == overlay_file.sha256 => {}
                Some(_) => {
                    changes.push(WorkspacePersistentChange {
                        path: path.clone(),
                        kind: WorkspacePersistentChangeKind::Modified,
                        sha256: overlay_file.sha256.clone(),
                        bytes: overlay_file.bytes,
                    });
                }
                None => {
                    changes.push(WorkspacePersistentChange {
                        path: path.clone(),
                        kind: WorkspacePersistentChangeKind::Added,
                        sha256: overlay_file.sha256.clone(),
                        bytes: overlay_file.bytes,
                    });
                }
            }
        }

        for path in base.keys() {
            if !overlay.contains_key(path) {
                return Err(DomainError::InvalidData(format!(
                    "agent.persistent_delete_unsupported: persistent file `{path}` is missing from the run projection"
                )));
            }
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(WorkspacePersistentChangeSet {
            state_id: Uuid::new_v4().to_string(),
            base_state_id: base_snapshot.base_state_id,
            changes,
        })
    }

    pub(super) async fn commit_persistent_state(
        &self,
        run_id: &str,
        mut changes: WorkspacePersistentChangeSet,
        previous_state_id: Option<&str>,
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        let run = self.load_run(run_id).await?;
        if let Some(state_id) = previous_state_id {
            let state_dir = self.persistent_state_dir(&run.workspace_id, state_id)?;
            let previous = self
                .read_persistent_state_manifest(&state_dir, state_id)
                .await?;
            if changes.base_state_id == previous.base_state_id
                && changes.changes == previous.changes
            {
                changes.state_id = state_id.to_string();
                return Ok(changes);
            }
        }
        let manifest = self.read_manifest(run_id).await?;
        let roots = persistent_roots(&manifest)?;
        let run_dir = self.run_dir(&run)?;
        let state_dir = self.persistent_state_dir(&run.workspace_id, &changes.state_id)?;
        match fs::symlink_metadata(&state_dir).await {
            Ok(_) => {
                return Err(DomainError::InvalidData(format!(
                    "agent.persistent_state_exists: state `{}` already exists",
                    changes.state_id
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect persistent state {}: {}",
                    state_dir.display(),
                    error
                )));
            }
        }

        let states_dir = state_dir.parent().ok_or_else(|| {
            DomainError::InternalError(format!(
                "Persistent state path has no parent: {}",
                state_dir.display()
            ))
        })?;
        fs::create_dir_all(states_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent states directory {}: {}",
                states_dir.display(),
                error
            ))
        })?;

        let temp_dir = states_dir.join(format!(
            ".{}.tmp-{}",
            changes.state_id,
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&temp_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent state temp directory {}: {}",
                temp_dir.display(),
                error
            ))
        })?;

        let commit_result = async {
            let mut files = Vec::new();
            for root in roots {
                let source_root = run_dir.join(&root);
                let target_root = temp_dir.join(&root);
                fs::create_dir_all(&target_root).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create persistent state root {}: {}",
                        target_root.display(),
                        error
                    ))
                })?;
                copy_directory_contents(&source_root, &target_root).await?;
                files.extend(scan_workspace_files(&target_root, &root).await?);
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));

            let state_manifest = PersistentStateManifest {
                version: 1,
                state_id: changes.state_id.clone(),
                run_id: run.id.clone(),
                base_state_id: changes.base_state_id.clone(),
                created_at: Utc::now(),
                files,
                changes: changes.changes.clone(),
            };
            Self::write_json_atomic(&temp_dir.join("manifest.json"), &state_manifest).await?;
            fs::rename(&temp_dir, &state_dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to promote persistent state {} to {}: {}",
                    temp_dir.display(),
                    state_dir.display(),
                    error
                ))
            })?;
            Ok::<(), DomainError>(())
        }
        .await;

        if let Err(error) = commit_result {
            let _ = fs::remove_dir_all(&temp_dir).await;
            return Err(error);
        }

        Ok(changes)
    }

    /// Publish a new persistent version that no run stands behind.
    ///
    /// The version carries everything its base carried and the given files on
    /// top, so a reader never sees a version that is missing a root it inherited.
    /// The change list is the diff against the base, which is what makes the
    /// "already published" fingerprint comparable with a run's own versions.
    pub(super) async fn publish_persistent_files(
        &self,
        workspace_id: &str,
        base_state_id: Option<&str>,
        writes: &[PersistentFileWrite],
    ) -> Result<WorkspacePersistentChangeSet, DomainError> {
        if writes.is_empty() {
            return Err(DomainError::InvalidData(
                "agent.persistent_write_empty: a state write must name at least one file".to_string(),
            ));
        }

        // The whole publish is one critical section: two writers deriving from
        // the same base must not both believe they are the newest version.
        let _guard = self.persist_lock.lock().await;

        let base = match base_state_id {
            Some(state_id) => {
                let state_dir = self.persistent_state_dir(workspace_id, state_id)?;
                let manifest = self
                    .read_persistent_state_manifest(&state_dir, state_id)
                    .await?;
                Some((state_dir, manifest))
            }
            None => None,
        };
        let base_dir = base.as_ref().map(|(dir, _)| dir.clone());
        let base_files = snapshot_map(
            base.as_ref()
                .map(|(_, manifest)| manifest.files.clone())
                .unwrap_or_default(),
        );

        // The roots are the ones the base already has, plus any the new files
        // name: a chat whose first version came from a run keeps every root that
        // run published.
        let mut roots: Vec<String> = Vec::new();
        for path in base_files.keys().cloned().chain(
            writes
                .iter()
                .map(|write| write.path.as_str().to_string()),
        ) {
            let root = path.split('/').next().unwrap_or_default();
            if !root.is_empty() && !roots.iter().any(|known| known == root) {
                roots.push(root.to_string());
            }
        }
        roots.sort();

        let states_dir = self
            .chat_dir(workspace_id)?
            .join(PERSISTENT_STATES_DIR);
        fs::create_dir_all(&states_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent states directory {}: {}",
                states_dir.display(),
                error
            ))
        })?;

        let state_id = Uuid::new_v4().to_string();
        let state_dir = self.persistent_state_dir(workspace_id, &state_id)?;
        let temp_dir = states_dir.join(format!(
            ".{}.tmp-{}",
            state_id,
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&temp_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create persistent state temp directory {}: {}",
                temp_dir.display(),
                error
            ))
        })?;

        let publish_result = async {
            for root in &roots {
                let target_root = temp_dir.join(root);
                if let Some(source_root) = base_dir.as_ref().map(|dir| dir.join(root))
                    && fs::symlink_metadata(&source_root).await.is_ok()
                {
                    copy_directory_contents(&source_root, &target_root).await?;
                }
                // The target root has to exist even when it only carries new
                // files, so the scan below sees the same roots either way.
                fs::create_dir_all(&target_root).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create persistent state root {}: {}",
                        target_root.display(),
                        error
                    ))
                })?;
            }

            for write in writes {
                let target = temp_dir.join(write.path.as_str());
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).await.map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create persistent write parent {}: {}",
                            parent.display(),
                            error
                        ))
                    })?;
                }
                fs::write(&target, write.text.as_bytes())
                    .await
                    .map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to write persistent file {}: {}",
                            target.display(),
                            error
                        ))
                    })?;
            }

            let mut files = Vec::new();
            for root in &roots {
                files.extend(scan_workspace_files(&temp_dir.join(root), root).await?);
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));

            let mut changes = Vec::new();
            for file in &files {
                let kind = match base_files.get(&file.path) {
                    Some(base_file) if base_file.sha256 == file.sha256 => continue,
                    Some(_) => WorkspacePersistentChangeKind::Modified,
                    None => WorkspacePersistentChangeKind::Added,
                };
                changes.push(WorkspacePersistentChange {
                    path: file.path.clone(),
                    kind,
                    sha256: file.sha256.clone(),
                    bytes: file.bytes,
                });
            }
            changes.sort_by(|a, b| a.path.cmp(&b.path));

            Ok::<(Vec<PersistentSnapshotFile>, Vec<WorkspacePersistentChange>), DomainError>((
                files, changes,
            ))
        }
        .await;

        let (files, changes) = match publish_result {
            Ok(result) => result,
            Err(error) => {
                let _ = fs::remove_dir_all(&temp_dir).await;
                return Err(error);
            }
        };

        // Nothing changed: reuse the version this write started from instead of
        // publishing an identical one.
        if changes.is_empty() {
            let _ = fs::remove_dir_all(&temp_dir).await;
            let state_id = base_state_id.ok_or_else(|| {
                DomainError::InvalidData(
                    "agent.persistent_write_unchanged: a write with no base and no change has nothing to publish"
                        .to_string(),
                )
            })?;
            return Ok(WorkspacePersistentChangeSet {
                state_id: state_id.to_string(),
                base_state_id: base_state_id.map(str::to_string),
                changes,
            });
        }

        let manifest = PersistentStateManifest {
            version: 1,
            state_id: state_id.clone(),
            run_id: String::new(),
            base_state_id: base_state_id.map(str::to_string),
            created_at: Utc::now(),
            files,
            changes: changes.clone(),
        };
        if let Err(error) = Self::write_json_atomic(&temp_dir.join("manifest.json"), &manifest).await
        {
            let _ = fs::remove_dir_all(&temp_dir).await;
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp_dir, &state_dir).await {
            let _ = fs::remove_dir_all(&temp_dir).await;
            return Err(DomainError::InternalError(format!(
                "Failed to promote persistent state {} to {}: {}",
                temp_dir.display(),
                state_dir.display(),
                error
            )));
        }

        Ok(WorkspacePersistentChangeSet {
            state_id,
            base_state_id: base_state_id.map(str::to_string),
            changes,
        })
    }

    pub(super) async fn read_persistent_state_manifest(
        &self,
        state_dir: &Path,
        state_id: &str,
    ) -> Result<PersistentStateManifest, DomainError> {
        let metadata = fs::symlink_metadata(state_dir).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "agent.persistent_state_not_found: {}",
                    state_dir.display()
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to inspect persistent state {}: {}",
                    state_dir.display(),
                    error
                ))
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_invalid: {}",
                state_dir.display()
            )));
        }

        let manifest: PersistentStateManifest =
            Self::read_json(&state_dir.join("manifest.json")).await?;
        if manifest.version != 1 {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_version_unsupported: {}",
                manifest.version
            )));
        }
        if manifest.state_id != state_id {
            return Err(DomainError::InvalidData(format!(
                "agent.persistent_state_manifest_mismatch: manifest state `{}` does not match requested state `{state_id}`",
                manifest.state_id
            )));
        }
        Ok(manifest)
    }
}

pub(super) fn persistent_roots(manifest: &WorkspaceManifest) -> Result<Vec<String>, DomainError> {
    let mut roots = Vec::new();
    for root in &manifest.roots {
        if root.lifecycle != tt_domain::models::agent::WorkspaceRootLifecycle::Persistent {
            continue;
        }
        if root.scope != WorkspaceRootScope::Chat
            || root.mount != WorkspaceRootMount::ProjectedOverlay
            || root.commit != WorkspaceRootCommit::OnRunCompleted
        {
            return Err(DomainError::InvalidData(format!(
                "Unsupported persistent workspace root `{}`",
                root.path
            )));
        }
        roots.push(validate_workspace_root_path(&root.path)?);
    }
    Ok(roots)
}

fn persistent_manifest_has_files_for_root(manifest: &PersistentStateManifest, root: &str) -> bool {
    let prefix = format!("{root}/");
    manifest
        .files
        .iter()
        .any(|file| file.path.starts_with(&prefix))
}
