use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::similharity_dto::{SimilharityRequestDto, SimilharityResponseDto};

/// Serves the Similharity compatibility bridge (`/api/plugins/similharity/*`).
///
/// `endpoint` is the path below the bridge prefix and `method` the HTTP verb the
/// extension used, because a few endpoints share a path and differ by verb.
#[tauri::command]
pub async fn similharity_handle(
    method: String,
    endpoint: String,
    request: SimilharityRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SimilharityResponseDto, CommandError> {
    log_command(format!(
        "similharity_handle {} {}",
        method.trim().to_ascii_uppercase(),
        endpoint.trim()
    ));
    app_state
        .services
        .similharity_service
        .handle_request(&method, &endpoint, request)
        .await
        .map_err(map_command_error("Similharity bridge request failed"))
}
