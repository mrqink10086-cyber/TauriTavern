use serde::Serialize;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use crate::services::agent_profile_service::AgentProfileResolveInput;
use crate::services::agent_tools::STATE_DOCUMENT_PATH;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentChatRef, WorkspacePath};
use tt_domain::models::state::{StateDeclaration, StateDocument};
use tt_domain::models::state_injection::{StateInjectionBlock, render_injection};

/// The state slice one Profile may see, and which state it came from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInjectionDto {
    /// The published version the slice came from. Absent means the chat has no
    /// committed state yet, which is a fact about the chat and not a failure.
    pub state_id: Option<String>,
    pub blocks: Vec<StateInjectionBlock>,
}

impl AgentRuntimeService {
    /// Render the state slice this Profile may see.
    ///
    /// The slice is read from the chat's **latest committed** state, not from a
    /// run's working copy: injection happens while the prompt is being
    /// assembled, and a run's own writes are published only when it completes.
    ///
    /// The declaration arrives from the host, which owns the binding, for the
    /// same reason the panel takes it: a field's own default decides injection
    /// when the Profile is silent, so the slice cannot be computed from the
    /// Profile alone.
    pub async fn resolve_state_injection(
        &self,
        chat_ref: &AgentChatRef,
        stable_chat_id: &str,
        profile_id: Option<&str>,
        declaration: &StateDeclaration,
    ) -> Result<StateInjectionDto, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(stable_chat_id)?;
        let profile = self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id,
                tool_catalog: self.tool_registry.catalog(),
            })
            .await?;

        // Nothing can be injected when neither side asks for it: a Profile with
        // no entries and a declaration with no fields have no answer to give.
        if profile.state_access.is_empty() && declaration.is_empty() {
            return Ok(StateInjectionDto {
                state_id: None,
                blocks: Vec::new(),
            });
        }

        let Some(state_id) = self.resolve_latest_persisted_state_id(chat_ref).await? else {
            return Ok(StateInjectionDto {
                state_id: None,
                blocks: Vec::new(),
            });
        };
        let workspace_id = workspace_id_for_stable_chat_id(chat_ref, &stable_chat_id)?;
        let Some(document) = self
            .read_persisted_state_document(&workspace_id, &state_id)
            .await?
        else {
            return Ok(StateInjectionDto {
                state_id: Some(state_id),
                blocks: Vec::new(),
            });
        };

        Ok(StateInjectionDto {
            blocks: render_injection(&document, &profile.state_access, declaration),
            state_id: Some(state_id),
        })
    }

    /// The newest committed persistent state in the chat.
    ///
    /// Every message is in scope, including the last one: state belongs to the
    /// floor that produced it, so a reader that skipped the newest floor would
    /// show a state the chat has already moved past.
    pub(super) async fn resolve_latest_persisted_state_id(
        &self,
        chat_ref: &AgentChatRef,
    ) -> Result<Option<String>, ApplicationError> {
        let Some(last_message) = self.find_last_chat_message(chat_ref).await? else {
            return Ok(None);
        };
        let message_count = last_message.index.saturating_add(1);
        self.resolve_persist_base_state_id(chat_ref, message_count, message_count)
            .await
    }

    pub(super) async fn read_persisted_state_document(
        &self,
        workspace_id: &str,
        state_id: &str,
    ) -> Result<Option<StateDocument>, ApplicationError> {
        let path = WorkspacePath::parse(STATE_DOCUMENT_PATH)?;
        let file = match self
            .workspace_repository
            .read_persistent_state_file(workspace_id, state_id, &path)
            .await
        {
            Ok(file) => file,
            // A committed version without a state document is a floor that
            // produced no state: nothing to render, and nothing to report.
            Err(DomainError::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        // A published document is read back through the same checks the writer
        // applied: stored keys bypass parsing on the way in.
        let document: StateDocument = serde_json::from_str(&file.text).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.invalid_document: published `{}` of state `{state_id}` is not a valid state document: {error}",
                path.as_str()
            ))
        })?;
        document.validate().map_err(|message| {
            ApplicationError::ValidationError(format!(
                "state.invalid_document: published `{}` of state `{state_id}` is not usable: {message}",
                path.as_str()
            ))
        })?;
        Ok(Some(document))
    }
}
