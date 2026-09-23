//! The `state.transition` tool: the model's own move request.
//!
//! The machine is deterministic — at run completion its rules fire on the
//! conditions the state document happens to satisfy. This tool is the other
//! half: the model asking to move now, which is what makes "the model asked for
//! it" different from "the rules allowed it". A request still has to name a
//! transition the spec defines, and the writes it carries go back through the
//! state document's own validation, so a move cannot put a key in the document
//! the declaration never defined.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use crate::services::agent_tools::common::{ensure_only_args, tool_error};
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use crate::services::agent_tools::structured::structured_value;
use crate::services::agent_tools::workspace::workspace_access_policy;
use crate::services::state_runtime::{
    access_from_snapshot, declaration_from_snapshot, evaluate_machine, field_writes_as_request,
    machine_from_snapshot, model_from_snapshot, read_machine_state, read_state_document,
    resolve_state_value_measure, state_machine_path, write_machine_pass,
};
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::state::{
    StateKey, StateUpdateError, apply_update, resolve_request_measured,
};
use tt_domain::models::state_access::check_writable;
use tt_domain::models::state_machine::{
    AppliedTransition, ComparatorRegistry, MachineError, MachineState, SkippedTransition,
    TransitionRequest, resolve_requests,
};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::skill_script::SkillScriptEngine;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateTransitionStructured<'a> {
    path: &'a str,
    active: Vec<String>,
    applied: &'a [AppliedTransition],
    skipped: &'a [SkippedTransition],
    events: &'a [String],
    written_keys: &'a [StateKey],
}

pub(in crate::services::agent_tools) async fn transition(
    workspace_repository: &dyn WorkspaceRepository,
    script_engine: Option<&dyn SkillScriptEngine>,
    run_id: &str,
    prompt_snapshot: &Value,
    tokenizers: Option<&Arc<dyn TokenizerRepository>>,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    // A chat with no bound machine has nothing to move. That is a configuration
    // fact the model should hear about, not a failure that stops the run.
    let Some(spec) = machine_from_snapshot(prompt_snapshot)? else {
        return Ok((
            tool_error(
                call,
                "state.machine_not_configured",
                "No state machine is bound to this chat, so there is nothing to move.",
            ),
            AgentToolEffect::None,
        ));
    };

    let requests = match parse_requests(args) {
        Ok(requests) => requests,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if requests.is_empty() {
        return Ok((
            tool_error(
                call,
                "state.empty_transition",
                "send at least one transition request",
            ),
            AgentToolEffect::None,
        ));
    }

    let machine_path = state_machine_path()?;
    let policy = workspace_access_policy(workspace_repository, run_id).await?;
    if !policy.is_writable(&machine_path) {
        return Ok((
            tool_error(
                call,
                "state.not_writable",
                &format!(
                    "`{}` is outside this Agent's writable workspace roots, so this Agent cannot move the machine.",
                    machine_path.as_str()
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    let document = read_state_document(
        workspace_repository,
        run_id,
        &declaration_from_snapshot(prompt_snapshot)?,
    )
    .await?;
    let machine_state = read_machine_state(workspace_repository, run_id, &spec).await?;

    // A request that names no transition the machine defines is a model
    // mistake: report it and change nothing, so the move can be rephrased in
    // one retry.
    let forced = match resolve_requests(&spec, &machine_state, &requests) {
        Ok(indices) => indices.into_iter().collect::<BTreeSet<_>>(),
        Err(errors) => {
            return Ok((
                rejected(call, &errors),
                AgentToolEffect::None,
            ));
        }
    };

    let evaluation = evaluate_machine(
        script_engine,
        &ComparatorRegistry::default(),
        &spec,
        &machine_state,
        &document.condition_fields(),
        &forced,
    )
    .await?;

    // The writes go through the document's own validation before the position
    // moves: a rejected write leaves the machine exactly where it was, so the
    // next attempt starts from a state the model can reason about.
    let mut updated_document = None;
    let mut written_keys = Vec::new();
    if !evaluation.writes.is_empty() {
        let declaration = declaration_from_snapshot(prompt_snapshot)?;
        let measure = match resolve_state_value_measure(
            &declaration,
            model_from_snapshot(prompt_snapshot).as_deref(),
            tokenizers,
        )
        .await
        {
            Ok(measure) => measure,
            Err(error) => {
                return Ok((
                    tool_error(call, "state.tokenizer_unavailable", &error.to_string()),
                    AgentToolEffect::None,
                ));
            }
        };
        let count = |value: &str| measure.count(value);
        let request = field_writes_as_request(&evaluation.writes);
        let resolved = match resolve_request_measured(&declaration, &request, &count) {
            Ok(resolved) => resolved,
            Err(errors) => return Ok((rejected_writes(call, &errors), AgentToolEffect::None)),
        };
        let access = access_from_snapshot(prompt_snapshot)?;
        if let Err(errors) = check_writable(&access, &declaration, &resolved) {
            return Ok((rejected_writes(call, &errors), AgentToolEffect::None));
        }
        let updated = match apply_update(&document, &resolved) {
            Ok(updated) => updated,
            Err(errors) => return Ok((rejected_writes(call, &errors), AgentToolEffect::None)),
        };
        written_keys = resolved.fields.iter().map(|(key, _)| key.clone()).collect();
        updated_document = Some(updated);
    }

    let next_machine_state = MachineState {
        active: evaluation.active.clone(),
    };
    let files = write_machine_pass(
        workspace_repository,
        run_id,
        &document,
        updated_document.as_ref(),
        &next_machine_state,
    )
    .await?;

    let mut content = format!(
        "State machine advanced: now at [{}]. {} transition applied, {} skipped.",
        evaluation
            .active
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        evaluation.applied.len(),
        evaluation.skipped.len(),
    );
    if !evaluation.events.is_empty() {
        content.push_str(&format!(" Events: {}.", evaluation.events.join(", ")));
    }
    if !written_keys.is_empty() {
        content.push_str(&format!(" {} field(s) written.", written_keys.len()));
    }

    let result = AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content,
        structured: structured_value(StateTransitionStructured {
            path: machine_path.as_str(),
            active: evaluation.active.iter().cloned().collect(),
            applied: &evaluation.applied,
            skipped: &evaluation.skipped,
            events: &evaluation.events,
            written_keys: &written_keys,
        }),
        is_error: false,
        error_code: None,
        resource_refs: files
            .iter()
            .map(|file| file.path.as_str().to_string())
            .collect(),
    };

    Ok((
        result,
        AgentToolEffect::WorkspaceFilesWritten {
            files,
            // State files are never the chat message, so nothing here is
            // offered to the automatic commit.
            last_text_mutation: None,
        },
    ))
}

/// Parse the tool arguments into the domain's own request shape.
///
/// Types are checked strictly against the descriptor: silently repairing a
/// malformed request here would hide exactly the failure the schema is supposed
/// to prevent, and the model would never learn the shape it must send.
fn parse_requests(args: &Map<String, Value>) -> Result<Vec<TransitionRequest>, String> {
    ensure_only_args(args, &["transitions"])?;

    let Some(value) = args.get("transitions") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err("transitions must be an array".to_string());
    };

    let mut requests = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(format!("transitions[{index}] must be an object"));
        };
        for key in object.keys() {
            if key != "to" && key != "from" {
                return Err(format!(
                    "transitions[{index}] has an unknown key `{key}`; only `to` and `from` are allowed"
                ));
            }
        }
        let Some(to) = object.get("to") else {
            return Err(format!("transitions[{index}].to is required"));
        };
        let to = parse_ids(to, &format!("transitions[{index}].to"))?;
        let from = match object.get("from") {
            None | Some(Value::Null) => None,
            Some(value) => Some(parse_ids(value, &format!("transitions[{index}].from"))?),
        };
        requests.push(TransitionRequest { to, from });
    }

    Ok(requests)
}

fn parse_ids(value: &Value, at: &str) -> Result<Vec<String>, String> {
    let Some(items) = value.as_array() else {
        return Err(format!("{at} must be an array of state ids"));
    };
    let mut ids = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(id) = item.as_str() else {
            return Err(format!("{at}[{index}] must be a string"));
        };
        ids.push(id.to_string());
    }
    Ok(ids)
}

/// Report every problem in one result so the model can fix the whole request in
/// a single retry, and say plainly that nothing moved.
fn rejected(call: &ToolInvocation, errors: &[MachineError]) -> AgentToolResult {
    let mut message = String::from(
        "The transition request was rejected and nothing moved. Fix every problem below, then call state.transition again:\n",
    );
    for error in errors {
        message.push_str("- ");
        message.push_str(&error.message);
        message.push('\n');
    }
    tool_error(call, "state.transition_rejected", message.trim_end())
}

/// The transition itself was legal; the fields it writes were not.
///
/// Reported separately from a rejected request because the fix is different:
/// the request was right, the machine's own action writes a key the declaration
/// does not define, or this Profile may not write it.
fn rejected_writes(call: &ToolInvocation, errors: &[StateUpdateError]) -> AgentToolResult {
    let mut message = String::from(
        "The transition was not applied: the state fields it writes were rejected, and nothing moved. Fix every problem below, then call state.transition again:\n",
    );
    for error in errors {
        message.push_str("- ");
        message.push_str(&error.message);
        message.push('\n');
    }
    tool_error(call, "state.transition_writes_rejected", message.trim_end())
}
