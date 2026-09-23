//! State written by a person, not by the model.
//!
//! The model writes state through a tool; a person writes it by clicking. Both
//! end in the same place — a published version of the chat's state, bound to the
//! floor it belongs to — and both pass the declaration, which is what keeps the
//! key space real. What differs is the authorization: `StateAccess` answers
//! "what may this Profile's *model* change on its own", and a click is not the
//! model acting on its own, so it does not apply here. Compare the run's own
//! completion pass, which is skipped by `StateAccess` for the same reason.
//!
//! The other difference is the trigger. The model asks for a key to change; a
//! person changes the key and expects the derived values to follow. That is what
//! the recalculation pass is: the chat's own hook runs once more with
//! `reason: "recalculate"`, and its writes join the same version. The engine
//! still knows no formula — a calendar, a total or an equipped bonus is the
//! user's script, and this path only carries the values through validation.

use serde::Serialize;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use crate::services::agent_tools::STATE_DOCUMENT_PATH;
use crate::services::state_runtime::{
    STATE_MACHINE_PATH, field_writes_as_request, resolve_state_value_measure,
    run_recalculation_hook,
};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentChatRef, WorkspacePath};
use tt_domain::models::state::{
    ResolvedStateUpdateRequest, StateDeclaration, StateDocument, StateUpdateError,
    StateUpdateRequest, apply_update, resolve_request_measured,
};
use tt_domain::models::state_machine::{MachineState, StateMachineSpec, initial_state};
use tt_ports::repositories::workspace_repository::PersistentFileWrite;

/// What one user edit did to a chat's state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEditDto {
    /// The version the chat now stands on. The caller binds it to the floor it
    /// edited, so the next run inherits the edit and the panel shows it.
    pub state_id: String,
    /// The version the edit started from.
    pub base_state_id: Option<String>,
    /// False when the edit asked for what the state already said: the version it
    /// started from was reused, and nothing was published.
    pub changed: bool,
    /// How many files the new version differs from its base by. One edit touches
    /// one document, so this is 0 or 1 — it rides along because the floor's
    /// metadata records it.
    pub change_count: usize,
    /// The keys the edit itself touched: set, cleared or removed.
    pub written_keys: Vec<String>,
    /// The keys the recalculation pass wrote, in the order the hook produced
    /// them. Empty when nothing recomputed.
    pub recalculated_keys: Vec<String>,
}

/// What one prose edit did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateProseEditDto {
    /// The version the chat now stands on.
    pub state_id: String,
    /// The version the edit started from.
    pub base_state_id: Option<String>,
    /// False when the file already said this: the version was reused.
    pub changed: bool,
    /// The file that now carries the text.
    pub path: String,
}

impl AgentRuntimeService {
    /// Write one panel's prose block.
    ///
    /// A diary entry or a character's inner voice is text somebody writes, which
    /// is exactly what a state field is not: no key, no access switches, no
    /// value limit, nothing to compare. So none of that machinery applies here —
    /// but two things still do. The text lands in a published version bound to a
    /// floor, so it travels with the story like the state does, and it is
    /// confined to the files this chat's own declaration names, so the panel's
    /// write path cannot become a way to write any file in the workspace.
    ///
    /// Whether the model ever reads it is not decided here. Prose is never
    /// injected; a Profile that can see `persist` can read it, and a prompt is
    /// what asks it to.
    pub async fn apply_user_prose_update(
        &self,
        chat_ref: &AgentChatRef,
        stable_chat_id: &str,
        declaration: &StateDeclaration,
        path: &str,
        text: &str,
    ) -> Result<StateProseEditDto, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(stable_chat_id)?;

        if !declaration.panels.declares_prose(path) {
            return Err(ApplicationError::ValidationError(format!(
                "state.undeclared_prose: `{path}` is not a prose block this chat's declaration names"
            )));
        }

        let path = WorkspacePath::parse(path.trim())?;
        let workspace_id = workspace_id_for_stable_chat_id(chat_ref, &stable_chat_id)?;
        let base_state_id = self.resolve_latest_persisted_state_id(chat_ref).await?;

        let changes = self
            .workspace_repository
            .publish_persistent_files(
                &workspace_id,
                base_state_id.as_deref(),
                &[PersistentFileWrite {
                    path: path.clone(),
                    text: text.to_string(),
                }],
            )
            .await?;

        Ok(StateProseEditDto {
            state_id: changes.state_id,
            base_state_id: changes.base_state_id,
            changed: !changes.changes.is_empty(),
            path: path.as_str().to_string(),
        })
    }

    /// Apply one user edit to a chat's state.
    ///
    /// `machine` is the chat's bound machine, when it has one. It arrives as an
    /// argument for the same reason the declaration does: the host owns the
    /// binding, and this service does not read configuration behind its back.
    #[expect(
        clippy::too_many_arguments,
        reason = "one edit carries the chat, the scene, its machine and the model a token ceiling follows"
    )]
    pub async fn apply_user_state_update(
        &self,
        chat_ref: &AgentChatRef,
        stable_chat_id: &str,
        declaration: &StateDeclaration,
        machine: Option<&StateMachineSpec>,
        tokenizer_model: Option<&str>,
        request: StateUpdateRequest,
        recalculate: bool,
    ) -> Result<StateEditDto, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(stable_chat_id)?;
        if request.fields.is_empty() && request.remove.is_empty() {
            return Err(ApplicationError::ValidationError(
                "state.empty_update: send at least one field to set or one key to remove"
                    .to_string(),
            ));
        }

        // A click has no run behind it, so the model a scene follows by default
        // arrives with the request: the panel knows what this chat is talking to.
        let measure =
            resolve_state_value_measure(declaration, tokenizer_model, self.state_tokenizer()).await?;
        let count = |value: &str| measure.count(value);

        let resolved = resolve_request_measured(declaration, &request, &count).map_err(rejected)?;
        let written_keys = touched_keys(&resolved);

        let workspace_id = workspace_id_for_stable_chat_id(chat_ref, &stable_chat_id)?;
        let base_state_id = self.resolve_latest_persisted_state_id(chat_ref).await?;
        let document = match base_state_id.as_deref() {
            Some(state_id) => self
                .read_persisted_state_document(&workspace_id, state_id)
                .await?
                .unwrap_or_default(),
            None => StateDocument::default(),
        };

        // The edit's own writes land first: the recalculation reads the state as
        // the person just left it, which is the only reading that can produce the
        // value they expect to see.
        let document = apply_update(&document, &resolved).map_err(rejected)?;

        let (document, recalculated_keys) = if recalculate {
            match machine {
                Some(spec) => {
                    let active = self
                        .read_persisted_machine_state(&workspace_id, base_state_id.as_deref(), spec)
                        .await?;
                    let writes = run_recalculation_hook(
                        Some(self.script_engine.as_ref()),
                        spec,
                        &active.active,
                        &document.condition_fields(),
                    )
                    .await?;
                    if writes.is_empty() {
                        (document, Vec::new())
                    } else {
                        // A hook writing a key the declaration does not define is
                        // a configuration problem: it stops the edit rather than
                        // publishing half of what was asked for.
                        let request = field_writes_as_request(&writes);
                        let resolved =
                            resolve_request_measured(declaration, &request, &count).map_err(rejected)?;
                        let keys = touched_keys(&resolved);
                        let document = apply_update(&document, &resolved).map_err(rejected)?;
                        (document, keys)
                    }
                }
                None => (document, Vec::new()),
            }
        } else {
            (document, Vec::new())
        };

        let text = serde_json::to_string_pretty(&document).map_err(|error| {
            ApplicationError::InternalError(format!("state.document_serialize_failed: {error}"))
        })?;
        let path = WorkspacePath::parse(STATE_DOCUMENT_PATH)?;
        let changes = self
            .workspace_repository
            .publish_persistent_files(
                &workspace_id,
                base_state_id.as_deref(),
                &[PersistentFileWrite { path, text }],
            )
            .await?;

        Ok(StateEditDto {
            state_id: changes.state_id,
            base_state_id: changes.base_state_id,
            changed: !changes.changes.is_empty(),
            change_count: changes.changes.len(),
            written_keys,
            recalculated_keys,
        })
    }

    /// Where the chat's machine stands, read from the published version.
    ///
    /// A run reads its position from the run workspace; a click has no run, so it
    /// reads the same file out of the version the click derives from. A version
    /// that never carried a position means the machine has not moved in this
    /// chat, which is the same answer a first run gets.
    async fn read_persisted_machine_state(
        &self,
        workspace_id: &str,
        state_id: Option<&str>,
        spec: &StateMachineSpec,
    ) -> Result<MachineState, ApplicationError> {
        let Some(state_id) = state_id else {
            return Ok(initial_state(spec));
        };
        let path = WorkspacePath::parse(STATE_MACHINE_PATH)?;
        match self
            .workspace_repository
            .read_persistent_state_file(workspace_id, state_id, &path)
            .await
        {
            Ok(file) => serde_json::from_str(&file.text).map_err(|error| {
                ApplicationError::ValidationError(format!(
                    "state.invalid_machine_state: published `{}` of state `{state_id}` is not a valid machine state: {error}",
                    path.as_str()
                ))
            }),
            Err(DomainError::NotFound(_)) => Ok(initial_state(spec)),
            Err(error) => Err(error.into()),
        }
    }
}

/// Every key a resolved request touches, in the declaration's spelling.
fn touched_keys(request: &ResolvedStateUpdateRequest) -> Vec<String> {
    request
        .fields
        .iter()
        .map(|(key, _)| key.as_str().to_string())
        .chain(request.remove.iter().map(|key| key.as_str().to_string()))
        .collect()
}

/// One line per problem, so a caller can show all of them at once.
fn rejected(errors: Vec<StateUpdateError>) -> ApplicationError {
    let lines = errors
        .iter()
        .map(|error| match &error.key {
            Some(key) => format!("- {} ({key}): {}", error.code, error.message),
            None => format!("- {}: {}", error.code, error.message),
        })
        .collect::<Vec<_>>()
        .join("\n");
    ApplicationError::ValidationError(lines)
}
