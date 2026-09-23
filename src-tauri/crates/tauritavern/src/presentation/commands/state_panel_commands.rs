use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::agent_dto::AgentStatePanelDto;
use tt_application::services::agent_runtime_service::StatePanelDto;

/// The panels this chat shows.
///
/// Read-only and side-effect free: the panel asks for its data instead of
/// parsing anything, and the same call is what a configuration preview reads.
/// The result names the version it came from, so "no state yet" and "state
/// exists but nothing is placed on a panel" stay distinguishable.
#[tauri::command]
pub async fn get_state_panel(
    dto: AgentStatePanelDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StatePanelDto, CommandError> {
    log_command("get_state_panel");

    app_state
        .services
        .agent_runtime_service
        .resolve_state_panel(&dto.chat_ref, &dto.stable_chat_id, &dto.declaration)
        .await
        .map_err(map_command_error("Failed to resolve state panel"))
}
