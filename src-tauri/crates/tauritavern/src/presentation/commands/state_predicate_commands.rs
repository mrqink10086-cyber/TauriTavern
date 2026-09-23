use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::agent_dto::AgentStatePredicateEntriesDto;
use tt_application::services::agent_runtime_service::StatePredicateEntryDto;
use tt_domain::models::state::StateDeclaration;
use tt_domain::models::state_predicate::{
    PredicateError, PredicateEvaluation, StatePredicateSet,
};

/// Result of a predicate set check.
#[derive(Serialize)]
pub struct StatePredicateValidation {
    pub errors: Vec<PredicateError>,
}

/// A set to run once, against assumed field values.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePredicateEvaluateDto {
    pub set: StatePredicateSet,
    /// `key -> values`, as a state document stores them.
    #[serde(default)]
    pub fields: BTreeMap<String, Vec<String>>,
}

fn parse_set(dto: &Value) -> Result<StatePredicateSet, CommandError> {
    let raw = dto.get("set").ok_or_else(|| {
        CommandError::BadRequest("the dto must carry a `set` field".to_string())
    })?;
    serde_json::from_value::<StatePredicateSet>(raw.clone()).map_err(|error| {
        CommandError::BadRequest(format!("Failed to parse state predicates: {error}"))
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
pub async fn save_state_predicate_set(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let name = match dto.get("name").and_then(Value::as_str) {
        Some(name) => name.to_string(),
        None => {
            return Err(CommandError::BadRequest(
                "save_state_predicate_set dto must carry a string `name` field".to_string(),
            ))
        }
    };
    log_command(format!("save_state_predicate_set, name: {name}"));

    let set = parse_set(&dto)?;
    let declaration = parse_declaration(&dto)?;

    app_state
        .services
        .state_predicate_service
        .save(&name, &set, declaration.as_ref())
        .map_err(map_command_error(format!(
            "Failed to save state predicates {name}"
        )))
}

#[tauri::command]
pub async fn get_state_predicate_set(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StatePredicateSet, CommandError> {
    log_command(format!("get_state_predicate_set, name: {name}"));

    app_state
        .services
        .state_predicate_service
        .get(&name)
        .map_err(map_command_error(format!(
            "Failed to get state predicates {name}"
        )))
}

#[tauri::command]
pub async fn list_state_predicate_sets(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_state_predicate_sets");

    app_state
        .services
        .state_predicate_service
        .list_names()
        .map_err(map_command_error("Failed to list state predicate sets"))
}

#[tauri::command]
pub async fn delete_state_predicate_set(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_state_predicate_set, name: {name}"));

    app_state
        .services
        .state_predicate_service
        .delete(&name)
        .map_err(map_command_error(format!(
            "Failed to delete state predicates {name}"
        )))
}

/// Check a set without storing it.
///
/// Reports every problem at once — duplicate ids, an entry with no content, an
/// effect that reaches a label no entry carries, a condition reading a field the
/// declaration does not define — so an editor can show them in one pass.
#[tauri::command]
pub async fn validate_state_predicate_set(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StatePredicateValidation, CommandError> {
    log_command("validate_state_predicate_set");

    let set = parse_set(&dto)?;
    let declaration = parse_declaration(&dto)?;

    Ok(StatePredicateValidation {
        errors: app_state
            .services
            .state_predicate_service
            .validate(&set, declaration.as_ref()),
    })
}

/// Run a set once against assumed field values, without storing it.
///
/// This is the editor's preview: it answers the same question the assembly asks
/// — which entries hold — but against values the user typed, so a half-finished
/// set can be judged before it is ever saved.
#[tauri::command]
pub async fn evaluate_state_predicate_set(
    dto: StatePredicateEvaluateDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<PredicateEvaluation, CommandError> {
    log_command("evaluate_state_predicate_set");

    app_state
        .services
        .state_predicate_service
        .evaluate_fields(&dto.set, &dto.fields)
        .map_err(map_command_error("Failed to evaluate state predicates"))
}

/// The entries this chat's state selects from one predicate set.
///
/// Read-only and side-effect free, like the state injection read: the host calls
/// it while it assembles a prompt, and nothing about the answer is stored. The
/// skipped list travels with the selected entries so a caller can explain a
/// block that came out shorter than the configuration looks.
#[tauri::command]
pub async fn get_state_predicate_entries(
    dto: AgentStatePredicateEntriesDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StatePredicateEntryDto, CommandError> {
    log_command("get_state_predicate_entries");

    // A set that travels inside the declaration needs no lookup; one bound on its
    // own is loaded by name, so the entries always come from what was saved.
    let set = match dto.set.clone() {
        Some(set) => set,
        None => {
            if dto.name.trim().is_empty() {
                return Err(CommandError::BadRequest(
                    "state_predicate.entries_target_missing: the request names neither a predicate set nor a name to load one by"
                        .to_string(),
                ));
            }
            app_state
                .services
                .state_predicate_service
                .get(&dto.name)
                .map_err(map_command_error(format!(
                    "Failed to load state predicates {}",
                    dto.name
                )))?
        }
    };

    app_state
        .services
        .agent_runtime_service
        .resolve_state_predicate_entries(
            &dto.chat_ref,
            &dto.stable_chat_id,
            &set,
            app_state.services.state_predicate_service.comparators(),
        )
        .await
        .map_err(map_command_error("Failed to resolve state predicate entries"))
}
