use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::agent_dto::{AgentStateEditDto, AgentStateProseEditDto};
use tt_application::services::agent_runtime_service::{StateEditDto, StateProseEditDto};

/// Write state from the interface.
///
/// The person using the app is the writer here, not the model: the per-field
/// writable authorization that bounds a model's own writes does not apply, and
/// the declaration does — the same shape guard every writer passes. The result
/// names the published version, which the caller binds to the floor it edited so
/// the next run inherits it.
#[tauri::command]
pub async fn update_state_values(
    dto: AgentStateEditDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateEditDto, CommandError> {
    log_command("update_state_values");

    let request = dto.request();

    app_state
        .services
        .agent_runtime_service
        .apply_user_state_update(
            &dto.chat_ref,
            &dto.stable_chat_id,
            &dto.declaration,
            dto.machine.as_ref(),
            dto.tokenizer_model.as_deref(),
            request,
            dto.recalculate,
        )
        .await
        .map_err(map_command_error("Failed to write state"))
}

/// Write one prose block from the interface.
///
/// The same writer, a different kind of thing: a diary entry or an inner voice
/// is a file somebody owns, so it has no key, no access switches and no value
/// limit. What it keeps from the state path is the version — the text is
/// published as a version the caller binds to a floor, so it travels with the
/// story — and the confinement: only a file this chat's own declaration names
/// can be written.
#[tauri::command]
pub async fn update_state_prose(
    dto: AgentStateProseEditDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateProseEditDto, CommandError> {
    log_command("update_state_prose");

    app_state
        .services
        .agent_runtime_service
        .apply_user_prose_update(
            &dto.chat_ref,
            &dto.stable_chat_id,
            &dto.declaration,
            &dto.path,
            &dto.text,
        )
        .await
        .map_err(map_command_error("Failed to write prose"))
}
