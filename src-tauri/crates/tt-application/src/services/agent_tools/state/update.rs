use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use crate::services::agent_tools::common::{ensure_only_args, tool_error};
use crate::services::agent_tools::dispatcher::AgentToolEffect;
use crate::services::agent_tools::structured::structured_value;
use crate::services::agent_tools::workspace::workspace_access_policy;
use crate::services::state_runtime::{
    access_from_snapshot, declaration_from_snapshot, model_from_snapshot, read_state_document,
    resolve_state_value_measure, state_document_path, write_state_document,
};
use tt_domain::models::agent::{AgentToolResult, WorkspaceFileWriteMode};
use tt_domain::models::state::{
    StateKey, StateUpdateError, StateUpdateRequest, apply_update, resolve_request_measured,
};
use tt_domain::models::state_access::check_writable;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateUpdateStructured<'a> {
    path: &'a str,
    fields_tracked: usize,
    set_keys: &'a [StateKey],
    cleared_keys: &'a [StateKey],
    removed_keys: &'a [StateKey],
    /// False when no declaration is bound: keys were accepted on shape alone.
    declaration_configured: bool,
}

pub(in crate::services::agent_tools) async fn update(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    prompt_snapshot: &Value,
    tokenizers: Option<&Arc<dyn TokenizerRepository>>,
    call: &ToolInvocation,
    args: &Map<String, Value>,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    // A broken declaration is a configuration problem, not a model mistake, so
    // it stops the call instead of being handed back as something to retry.
    let declaration = declaration_from_snapshot(prompt_snapshot)?;
    let declaration_configured = !declaration.is_empty();

    let request = match parse_request(args) {
        Ok(request) => request,
        Err(message) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", &message),
                AgentToolEffect::None,
            ));
        }
    };
    if request.fields.is_empty() && request.remove.is_empty() {
        return Ok((
            tool_error(
                call,
                "state.empty_update",
                "send at least one field to set or one key to remove",
            ),
            AgentToolEffect::None,
        ));
    }

    let path = state_document_path()?;
    let policy = workspace_access_policy(workspace_repository, run_id).await?;
    if !policy.is_writable(&path) {
        return Ok((
            tool_error(
                call,
                "state.not_writable",
                &format!(
                    "`{}` is outside this Agent's writable workspace roots, so this Agent cannot update state. Widen the profile's writable roots to let it.",
                    path.as_str()
                ),
            ),
            AgentToolEffect::None,
        ));
    }

    // A scene that counts its values in tokens has to be measured by a
    // vocabulary. Asking for one this host cannot count with is a scene the
    // author has to fix, so it is reported as a configuration problem instead of
    // being quietly re-measured in characters.
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

    let resolved = match resolve_request_measured(&declaration, &request, &count) {
        Ok(resolved) => resolved,
        Err(errors) => return Ok((rejected(call, &errors), AgentToolEffect::None)),
    };

    // Field-level access is checked before anything is read or written, so a
    // refused key cannot leave a half-applied document behind.
    let access = access_from_snapshot(prompt_snapshot)?;
    if let Err(errors) = check_writable(&access, &declaration, &resolved) {
        return Ok((rejected(call, &errors), AgentToolEffect::None));
    }

    let document = read_state_document(workspace_repository, run_id, &declaration).await?;
    let updated = match apply_update(&document, &resolved) {
        Ok(updated) => updated,
        Err(errors) => return Ok((rejected(call, &errors), AgentToolEffect::None)),
    };
    let file = write_state_document(workspace_repository, run_id, &updated).await?;

    let (set_keys, cleared_keys) = resolved
        .fields
        .iter()
        .partition::<Vec<_>, _>(|(_, values)| !values.is_empty());
    let set_keys = set_keys
        .into_iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let cleared_keys = cleared_keys
        .into_iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();

    // Without a declaration every well-formed key is accepted, so say so in
    // the result: the model and the run journal should be able to tell a
    // validated update from an unconstrained one.
    let mut content = format!(
        "State updated: {} set, {} cleared, {} removed; {} fields tracked.",
        set_keys.len(),
        cleared_keys.len(),
        resolved.remove.len(),
        updated.fields.len()
    );
    if !declaration_configured {
        content.push_str(
            " No state declaration is bound to this chat, so these keys were accepted on shape alone.",
        );
    }

    let result = AgentToolResult {
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        content,
        structured: structured_value(StateUpdateStructured {
            path: file.path.as_str(),
            fields_tracked: updated.fields.len(),
            set_keys: &set_keys,
            cleared_keys: &cleared_keys,
            removed_keys: &resolved.remove,
            declaration_configured,
        }),
        is_error: false,
        error_code: None,
        resource_refs: vec![file.path.as_str().to_string()],
    };

    Ok((
        result,
        AgentToolEffect::WorkspaceFileWritten {
            file,
            mode: WorkspaceFileWriteMode::Replace,
        },
    ))
}

/// Parse the tool arguments into the domain's raw request.
///
/// Types are checked strictly against the descriptor: this is the layer that
/// has to hold when the model sends something the schema does not allow, and
/// silently repairing it here would hide exactly the failure the tool schema is
/// supposed to prevent.
fn parse_request(args: &Map<String, Value>) -> Result<StateUpdateRequest, String> {
    ensure_only_args(args, &["fields", "remove"])?;

    let mut fields = Vec::new();
    if let Some(value) = args.get("fields") {
        let Some(items) = value.as_array() else {
            return Err("fields must be an array".to_string());
        };
        for (index, item) in items.iter().enumerate() {
            let Some(object) = item.as_object() else {
                return Err(format!("fields[{index}] must be an object"));
            };
            let Some(key) = object.get("key").and_then(Value::as_str) else {
                return Err(format!("fields[{index}].key must be a string"));
            };
            let Some(values) = object.get("value").and_then(Value::as_array) else {
                return Err(format!(
                    "fields[{index}].value must be an array of strings, even for a single value; send [] to clear the field"
                ));
            };
            let mut lines = Vec::with_capacity(values.len());
            for (line_index, line) in values.iter().enumerate() {
                let Some(line) = line.as_str() else {
                    return Err(format!(
                        "fields[{index}].value[{line_index}] must be a string"
                    ));
                };
                lines.push(line.to_string());
            }
            fields.push((key.to_string(), lines));
        }
    }

    let mut remove = Vec::new();
    if let Some(value) = args.get("remove") {
        let Some(items) = value.as_array() else {
            return Err("remove must be an array of strings".to_string());
        };
        for (index, item) in items.iter().enumerate() {
            let Some(key) = item.as_str() else {
                return Err(format!("remove[{index}] must be a string"));
            };
            remove.push(key.to_string());
        }
    }

    Ok(StateUpdateRequest { fields, remove })
}

/// Report every problem in one result so the model can fix the whole submission
/// in a single retry, and say plainly that nothing was written.
fn rejected(call: &ToolInvocation, errors: &[StateUpdateError]) -> AgentToolResult {
    let mut message = String::from(
        "The state update was rejected and nothing was written. Fix every problem below, then call state.update again with the whole change set:\n",
    );
    for error in errors {
        message.push_str("- ");
        message.push_str(&error.message);
        message.push('\n');
    }
    tool_error(call, "state.update_rejected", message.trim_end())
}
