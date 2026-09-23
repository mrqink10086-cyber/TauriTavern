use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::agent_dto::AgentStateInjectionDto;
use tt_application::services::agent_runtime_service::StateInjectionDto;

/// The state slice this Profile may see in this chat.
///
/// Read-only and side-effect free: the host calls it while it assembles a
/// prompt, and the panel reads the same shape when it renders state. The result
/// says which published version the slice came from, so "no state yet" and
/// "state exists but nothing is injectable" stay distinguishable.
#[tauri::command]
pub async fn get_state_injection(
    dto: AgentStateInjectionDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<StateInjectionDto, CommandError> {
    log_command("get_state_injection");

    app_state
        .services
        .agent_runtime_service
        .resolve_state_injection(
            &dto.chat_ref,
            &dto.stable_chat_id,
            dto.profile_id.as_deref(),
            &dto.declaration,
        )
        .await
        .map_err(map_command_error("Failed to resolve state injection"))
}
