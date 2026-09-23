use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_domain::models::state::StateDeclaration;
use tt_domain::models::state_machine::{
    MachineError, MachineEvaluation, MachineState, StateMachineSpec, TransitionRequest,
    resolve_requests,
};

/// Result of a machine definition check.
#[derive(Serialize)]
pub struct StateMachineValidation {
    pub errors: Vec<MachineError>,
}

/// Result of running a machine once: the new positions, what fired, what was
/// refused, and the writes the caller still has to apply to the state document.
#[derive(Serialize)]
pub struct StateMachineRun {
    pub evaluation: MachineEvaluation,
    pub errors: Vec<MachineError>,
}

fn parse_spec(dto: &Value) -> Result<StateMachineSpec, CommandError> {
    let raw = dto.get("machine").ok_or_else(|| {
        CommandError::BadRequest("the dto must carry a `machine` field".to_string())
    })?;
    serde_json::from_value::<StateMachineSpec>(raw.clone()).map_err(|error| {
        CommandError::BadRequest(format!("Failed to parse state machine: {error}"))
    })
}

fn parse_declaration(dto: &Value) -> Result<Option<StateDeclaration>, CommandError> {
    match dto.get("declaration") {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => serde_json::from_value::<StateDeclaration>(raw.clone())
            .map(Some)
            .map_err(|error| {
                CommandError::BadRequest(format!("Failed to parse state declaration: {error}"))
            }),
    }
}

#[tauri::command]
pub async fn save_state_machine(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let name = match dto.get("name").and_then(Value::as_str) {
        Some(name) => name.to_string(),
        None => {
            return Err(CommandError::BadRequest(
                "save_state_machine dto must carry a string `name` field".to_string(),
            ))
        }
    };
    log_command(format!("save_state_machine, name: {name}"));

    let machine = parse_spec(&dto)?;
    let declaration = parse_declaration(&dto)?;

    app_state
        .services
        .state_machine_service
        .save(&name, &machine, declaration.as_ref())
        .map_err(map_command_error(format!(
            "Failed to save state machine {name}"
        )))
}

#[tauri::command]
pub async fn get_state_machine(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateMachineSpec, CommandError> {
    log_command(format!("get_state_machine, name: {name}"));

    app_state
        .services
        .state_machine_service
        .get(&name)
        .map_err(map_command_error(format!(
            "Failed to get state machine {name}"
        )))
}

#[tauri::command]
pub async fn list_state_machines(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_state_machines");

    app_state
        .services
        .state_machine_service
        .list_names()
        .map_err(map_command_error("Failed to list state machines"))
}

#[tauri::command]
pub async fn delete_state_machine(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_state_machine, name: {name}"));

    app_state
        .services
        .state_machine_service
        .delete(&name)
        .map_err(map_command_error(format!(
            "Failed to delete state machine {name}"
        )))
}

/// Check a definition without storing it.
///
/// Reports every problem at once — unknown states, unknown comparators or
/// actions, fields the declaration does not define, a machine that can never
/// start — so an editor can show them all in one pass.
#[tauri::command]
pub async fn validate_state_machine(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateMachineValidation, CommandError> {
    log_command("validate_state_machine");

    let machine = parse_spec(&dto)?;
    let declaration = parse_declaration(&dto)?;

    Ok(StateMachineValidation {
        errors: app_state
            .services
            .state_machine_service
            .validate(&machine, declaration.as_ref()),
    })
}

/// Run a machine once: resolve the requested moves, apply every rule that
/// fires, and report what happened.
///
/// `requests` are explicit moves (from a model or a script); they are resolved
/// against the spec first and reported as errors when the spec has no such
/// transition. Rules that fire on their own conditions do not need a request.
#[tauri::command]
pub async fn evaluate_state_machine(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateMachineRun, CommandError> {
    log_command("evaluate_state_machine");

    let machine = parse_spec(&dto)?;
    let active: BTreeSet<String> = dto
        .get("active")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| machine.initial.iter().cloned().collect());
    let fields: BTreeMap<String, Vec<String>> = dto
        .get("fields")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(key, value)| {
                    let values = value
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    (key.clone(), values)
                })
                .collect()
        })
        .unwrap_or_default();
    let requests: Vec<TransitionRequest> = match dto.get("requests") {
        None | Some(Value::Null) => Vec::new(),
        Some(raw) => serde_json::from_value(raw.clone()).map_err(|error| {
            CommandError::BadRequest(format!("Failed to parse transition requests: {error}"))
        })?,
    };

    let state = MachineState { active };
    let mut errors = Vec::new();
    let forced: BTreeSet<usize> = match resolve_requests(&machine, &state, &requests) {
        Ok(indices) => indices.into_iter().collect(),
        Err(request_errors) => {
            errors = request_errors;
            BTreeSet::new()
        }
    };

    let evaluation = app_state
        .services
        .state_machine_service
        .evaluate(&machine, &state, &fields, &forced)
        .await
        .map_err(map_command_error("Failed to evaluate state machine"))?;

    Ok(StateMachineRun { evaluation, errors })
}
