use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_domain::models::state::StateDeclaration;

#[tauri::command]
pub async fn save_state_declaration(
    dto: Value,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    let name = match dto.get("name").and_then(Value::as_str) {
        Some(name) => name.to_string(),
        None => {
            return Err(CommandError::BadRequest(
                "save_state_declaration dto must carry a string `name` field".to_string(),
            ))
        }
    };
    log_command(format!("save_state_declaration, name: {name}"));

    let declaration = match dto.get("declaration") {
        Some(raw) => match serde_json::from_value::<StateDeclaration>(raw.clone()) {
            Ok(declaration) => declaration,
            Err(error) => {
                return Err(CommandError::BadRequest(format!(
                    "Failed to parse state declaration for {name}: {error}"
                )))
            }
        },
        None => {
            return Err(CommandError::BadRequest(format!(
                "save_state_declaration dto for {name} is missing the `declaration` field"
            )))
        }
    };

    // A scene counting its values in tokens can only be checked here: the
    // declaration service has no vocabulary, and an initial value is seeded into
    // the document and injected from there, so it never passes a write that
    // would have caught it.
    let tokenizer_model = dto
        .get("tokenizerModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());

    app_state
        .services
        .agent_runtime_service
        .validate_state_initial_values(&declaration, tokenizer_model)
        .await
        .map_err(map_command_error(format!(
            "Failed to validate state declaration {name}"
        )))?;

    app_state
        .services
        .state_declaration_service
        .save(&name, &declaration)
        .map_err(map_command_error(format!(
            "Failed to save state declaration {name}"
        )))
}

#[tauri::command]
pub async fn get_state_declaration(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateDeclaration, CommandError> {
    log_command(format!("get_state_declaration, name: {name}"));

    app_state
        .services
        .state_declaration_service
        .get(&name)
        .map_err(map_command_error(format!(
            "Failed to get state declaration {name}"
        )))
}

#[tauri::command]
pub async fn list_state_declarations(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("list_state_declarations");

    app_state
        .services
        .state_declaration_service
        .list_names()
        .map_err(map_command_error("Failed to list state declarations"))
}

#[tauri::command]
pub async fn delete_state_declaration(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_state_declaration, name: {name}"));

    app_state
        .services
        .state_declaration_service
        .delete(&name)
        .map_err(map_command_error(format!(
            "Failed to delete state declaration {name}"
        )))
}
