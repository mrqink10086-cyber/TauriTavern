//! Resolving a chat's predicate entries for assembly.
//!
//! The set arrives already loaded: this is the half that needs the chat — which
//! state version is current, and what it says. The entries are produced fresh on
//! every read, so there is nothing stored to invalidate and nothing to clean up
//! when the state moves on.

use serde::Serialize;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use tt_domain::models::agent::AgentChatRef;
use tt_domain::models::state_machine::ComparatorRegistry;
use tt_domain::models::state_predicate::{
    SelectedPredicate, SkippedPredicate, StatePredicateSet, evaluate_predicates,
};

/// What one chat's state selects from one predicate set.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePredicateEntryDto {
    /// The published version the entries were resolved against. Absent means the
    /// chat has no committed state yet, which is a fact about the chat and not a
    /// failure.
    pub state_id: Option<String>,
    /// The entries to place, in the order they were ordered by.
    pub entries: Vec<SelectedPredicate>,
    /// Everything that was a candidate and did not make it, with its reason. A
    /// reader that cannot say why an entry is missing is one the user has to
    /// debug by guessing.
    pub skipped: Vec<SkippedPredicate>,
}

impl AgentRuntimeService {
    /// Resolve one predicate set against the chat's newest committed state.
    ///
    /// A chat with no committed state has no predicates to select: every entry
    /// that depends on state is unavailable, and the answer says the version is
    /// absent rather than pretending the set is empty.
    pub async fn resolve_state_predicate_entries(
        &self,
        chat_ref: &AgentChatRef,
        stable_chat_id: &str,
        set: &StatePredicateSet,
        comparators: &ComparatorRegistry,
    ) -> Result<StatePredicateEntryDto, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(stable_chat_id)?;
        let Some(state_id) = self.resolve_latest_persisted_state_id(chat_ref).await? else {
            return Ok(StatePredicateEntryDto {
                state_id: None,
                entries: Vec::new(),
                skipped: Vec::new(),
            });
        };
        let workspace_id = workspace_id_for_stable_chat_id(chat_ref, &stable_chat_id)?;
        let Some(document) = self
            .read_persisted_state_document(&workspace_id, &state_id)
            .await?
        else {
            return Ok(StatePredicateEntryDto {
                state_id: Some(state_id),
                entries: Vec::new(),
                skipped: Vec::new(),
            });
        };

        // A configuration problem stops the read: an entry whose condition cannot
        // be evaluated must not be quietly left out, or the reader would see a
        // shorter block and no reason for it.
        let evaluation = evaluate_predicates(set, &document, comparators).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.predicate_invalid_read: {}\n{}",
                error.code, error.message
            ))
        })?;

        Ok(StatePredicateEntryDto {
            state_id: Some(state_id),
            entries: evaluation.selected,
            skipped: evaluation.skipped,
        })
    }
}
