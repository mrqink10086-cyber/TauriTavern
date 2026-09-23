use serde_json::json;

use super::{STATE_TRANSITION, STATE_UPDATE};
use tt_domain::models::tool::{ToolDescriptor, ToolId};

pub(in crate::services::agent_tools) fn state_update_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(STATE_UPDATE).expect("builtin tool name must be valid"),
        title: Some("Update State".to_string()),
        description: Some(
            "Record what changed in the tracked state. Send only the fields that changed: a field you leave out keeps its current value, an empty value array clears it, and `remove` deletes it. Keys must be the ones the state declaration defines."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "fields": {
                    "type": "array",
                    "description": "Fields to set or clear.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "key": {
                                "type": "string",
                                "description": "The field's key, spelled the way the state declaration writes it."
                            },
                            "value": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "One entry per line of the field's value. Send an empty array to clear the field without deleting it."
                            }
                        },
                        "required": ["key", "value"]
                    }
                },
                "remove": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Keys to delete outright. An empty value only clears; deleting needs this list."
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "idempotent": true, "sourceKind": "state" }),
    }
}

pub(in crate::services::agent_tools) fn state_transition_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(STATE_TRANSITION).expect("builtin tool name must be valid"),
        title: Some("Transition State".to_string()),
        description: Some(
            "Move the story's position in this chat's state machine. Send the positions you want to enter: a move only fires a transition the machine already defines from the current position, and a move that names no such transition is reported instead of guessed at."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "transitions": {
                    "type": "array",
                    "description": "Moves to request, in order.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "to": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "State ids to enter."
                            },
                            "from": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional. Positions that must be active for the move to be legal; omit to mean any current position."
                            }
                        },
                        "required": ["to"]
                    }
                }
            }
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "idempotent": false, "sourceKind": "state" }),
    }
}
