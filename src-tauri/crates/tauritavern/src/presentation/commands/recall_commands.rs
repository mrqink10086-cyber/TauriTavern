use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::services::recall_service::StateFloorAssignment;

/// Where one published state version landed, as the chat metadata knows it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecallFloorBindingDto {
    pub state_id: String,
    pub floor: i64,
}

/// The floors to record for one chat.
///
/// One command covers both "the message that was just saved" and "walk this
/// chat and fill in what is missing": they are the same operation, and a
/// repeated binding is a no-op in storage.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecallBindStateFloorsDto {
    pub stable_chat_id: String,
    #[serde(default)]
    pub bindings: Vec<RecallFloorBindingDto>,
}

/// Record which floor each published state version landed on.
#[tauri::command]
pub async fn recall_bind_state_floors(
    dto: RecallBindStateFloorsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<usize, CommandError> {
    log_command("recall_bind_state_floors");
    let assignments = dto
        .bindings
        .iter()
        .map(|binding| StateFloorAssignment {
            state_id: binding.state_id.clone(),
            floor: binding.floor,
        })
        .collect::<Vec<_>>();
    app_state
        .services
        .recall_service
        .bind_state_floors(&dto.stable_chat_id, &assignments)
        .await
        .map_err(map_command_error("Failed to bind recall state floors"))
}
