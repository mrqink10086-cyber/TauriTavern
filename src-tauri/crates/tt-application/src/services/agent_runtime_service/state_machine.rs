//! Advancing the chat's state machine when a run finishes.
//!
//! The machine has two ways to move. The model can ask, through the
//! `state.transition` tool; and the rules can fire on their own. This module
//! owns the second one — one deterministic pass over the document the run
//! produced, with no model involvement at all.
//!
//! A chat with no bound machine skips this entirely, which is what keeps the
//! layer optional. A pass that fails is reported to the run journal and does
//! not undo the reply the run already committed: state is allowed to lag behind
//! the story, but a failure is never allowed to be silent.

use std::collections::BTreeSet;

use serde_json::json;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use crate::services::state_runtime::{
    declaration_from_snapshot, evaluate_machine, field_writes_as_request, machine_from_snapshot,
    model_from_snapshot, read_machine_state, read_run_prompt_snapshot, read_state_document,
    resolve_state_value_measure, write_machine_pass,
};
use tt_domain::models::agent::AgentRunEventLevel;
use tt_domain::models::state::{
    StateUpdateError, apply_update, resolve_request_measured,
};
use tt_domain::models::state_machine::{
    ComparatorRegistry, MachineEvaluation, MachineState,
};

impl AgentRuntimeService {
    /// Move the chat's bound machine once, on the state this run produced.
    ///
    /// Returns `None` when the chat has no machine bound. Field writes go
    /// through the state document's own validation, so the machine cannot put a
    /// key in the document that the declaration does not define.
    ///
    /// This pass runs on the run's own workspace, before publication, so the
    /// new position and the values it moved are published together with the
    /// floor that caused them.
    pub(super) async fn advance_state_machine(
        &self,
        run_id: &str,
    ) -> Result<Option<MachineEvaluation>, ApplicationError> {
        let workspace_repository = self.workspace_repository.as_ref();
        let prompt_snapshot = read_run_prompt_snapshot(workspace_repository, run_id).await?;
        let Some(spec) = machine_from_snapshot(&prompt_snapshot)? else {
            return Ok(None);
        };

        let document = read_state_document(
            workspace_repository,
            run_id,
            &declaration_from_snapshot(&prompt_snapshot)?,
        )
        .await?;
        let machine_state = read_machine_state(workspace_repository, run_id, &spec).await?;

        // A completion pass fires only what the state itself satisfies: a move
        // the model asked for already happened through `state.transition`.
        let evaluation = evaluate_machine(
            Some(self.script_engine.as_ref()),
            &ComparatorRegistry::default(),
            &spec,
            &machine_state,
            &document.condition_fields(),
            &BTreeSet::new(),
        )
        .await?;

        let mut updated_document = None;
        if !evaluation.writes.is_empty() {
            let declaration = declaration_from_snapshot(&prompt_snapshot)?;
            let measure = resolve_state_value_measure(
                &declaration,
                model_from_snapshot(&prompt_snapshot).as_deref(),
                self.state_tokenizer(),
            )
            .await?;
            let count = |value: &str| measure.count(value);
            let request = field_writes_as_request(&evaluation.writes);
            // Field-level access is deliberately not checked here. `StateAccess`
            // answers "what may this Profile's model write"; this pass is the
            // chat's own configuration acting on its own, with no model in the
            // loop, and refusing it would make a rule the user wrote impossible
            // to run. The declaration check still applies: it is what keeps
            // storage shape honest.
            let resolved = resolve_request_measured(&declaration, &request, &count)
                .map_err(|errors| rejected_writes(&errors))?;
            updated_document =
                Some(apply_update(&document, &resolved).map_err(|errors| rejected_writes(&errors))?);
        }

        let next_machine_state = MachineState {
            active: evaluation.active.clone(),
        };
        write_machine_pass(
            workspace_repository,
            run_id,
            &document,
            updated_document.as_ref(),
            &next_machine_state,
        )
        .await?;

        Ok(Some(evaluation))
    }

    /// Advance the machine, recording the outcome instead of failing the run.
    ///
    /// State is allowed to lag behind the story, so a broken machine must not
    /// take the committed reply with it. It must not be invisible either: the
    /// journal records the failure, which is what lets the panel report that
    /// this floor's state did not advance.
    pub(super) async fn advance_state_machine_before_commit(
        &self,
        run_id: &str,
    ) -> Result<StateMachineAdvance, ApplicationError> {
        let outcome = match self.advance_state_machine(run_id).await {
            Ok(outcome) => outcome,
            Err(error) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "state_machine_advance_failed",
                    json!({ "message": error.to_string() }),
                )
                .await?;
                return Ok(StateMachineAdvance::Failed);
            }
        };
        let Some(evaluation) = outcome else {
            return Ok(StateMachineAdvance::NoMachine);
        };

        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "state_machine_advanced",
            json!({
                "active": evaluation.active.iter().cloned().collect::<Vec<_>>(),
                "applied": evaluation.applied.len(),
                "skipped": evaluation.skipped.len(),
                "writtenKeys": evaluation.writes.iter().map(|write| write.key.clone()).collect::<Vec<_>>(),
                "events": evaluation.events,
            }),
        )
        .await?;
        Ok(StateMachineAdvance::Advanced)
    }
}

/// What one completion pass did.
///
/// The caller records it so a resumed run does not advance the same floor twice:
/// only `Failed` leaves the pass to be retried. A chat with no machine bound has
/// nothing to retry either, so it counts as done.
pub(super) enum StateMachineAdvance {
    NoMachine,
    Advanced,
    Failed,
}

/// A machine whose own action writes a key the declaration does not define is a
/// configuration problem: the run stops advancing state instead of publishing a
/// document the rest of the layer cannot read.
fn rejected_writes(errors: &[StateUpdateError]) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "state.machine_writes_rejected: the machine's own action writes were refused, so the machine did not advance:\n{}",
        errors
            .iter()
            .map(|error| format!("- {}", error.message))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
