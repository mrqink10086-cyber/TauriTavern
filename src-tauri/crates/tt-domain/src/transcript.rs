//! In-run transcript compaction ("脱壳").
//!
//! A run keeps its whole conversation in memory and resends it on every model
//! request. Tool turns are the expensive part: each one carries a protocol
//! shell — `call_id` twice (once on the call, once on the result), the JSON
//! envelope, per-message framing, and the repeated tool name — and that shell
//! is re-sent on every round, so it compounds with the number of rounds.
//!
//! Compaction folds an aged tool turn — `assistant(ToolCall…)` plus the
//! `tool(ToolResult…)` messages that answer it — into a single plain text
//! message. Only the shell is removed: **tool result bodies are kept
//! verbatim**, because what a tool read is the material the reply is built
//! from, and trimming it would silently drop information.
//!
//! Two properties keep it safe:
//!
//! * The newest turns are never folded. Providers answer tool results against
//!   the most recent tool calls, so the tail always keeps its native shape.
//! * Folding is idempotent and stable. A folded turn no longer contains any
//!   `ToolCall` / `ToolResult` part, so it is invisible to the next pass —
//!   each round folds at most the one turn that just aged past the window.
//!
//! The function is pure: no IO, no model call, and the audit artifacts
//! (`model-responses/`, `tool-args/`, `tool-results/`) are untouched.

use std::collections::HashSet;

use serde_json::Value;

use crate::models::agent::{AgentModelContentPart, AgentModelMessage, AgentModelRole};
use crate::models::tool::{ToolArguments, ToolInvocation};

/// Folding the newest turn would leave unanswered tool calls, which providers
/// reject, so the window can never go below one.
const MINIMUM_UNFOLDED_TOOL_TURNS: usize = 1;

/// Tools whose *arguments* collapse to key names once a turn ages out. The
/// values they carried are available elsewhere — `state.update` values are
/// re-injected from the current state document — so the old argument body is
/// pure duplication.
const ARGUMENT_KEYS_ONLY_TOOLS: &[&str] = &["state.update"];

/// Header of a folded turn. Doubles as the marker that makes the block
/// recognizable to a reader of the transcript.
const FOLDED_TURN_MARKER: &str = "[folded tool turn]";

/// What one compaction pass did. Reported to the run journal so a change in
/// model behaviour can be traced back to a transcript that was rewritten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TranscriptCompaction {
    /// Tool turns folded into plain text.
    pub folded_turns: usize,
    /// Messages removed from the transcript by folding.
    pub folded_messages: usize,
}

/// Fold aged tool turns of `messages` into plain text, keeping the newest
/// `unfolded_tool_turns` turns in their native protocol shape.
pub fn compact_tool_transcript(
    messages: &mut Vec<AgentModelMessage>,
    unfolded_tool_turns: usize,
) -> TranscriptCompaction {
    let mut compaction = TranscriptCompaction::default();
    let turns = tool_turn_spans(messages);
    let keep = unfolded_tool_turns.max(MINIMUM_UNFOLDED_TOOL_TURNS);
    let foldable = turns.len().saturating_sub(keep);
    if foldable == 0 {
        return compaction;
    }

    // Spans are recorded up front, so every fold shrinks the transcript in
    // front of the spans that follow it; `removed_before` maps the recorded
    // indices onto the live ones.
    let mut removed_before = 0_usize;
    for turn in turns.iter().take(foldable) {
        let start = turn.start - removed_before;
        let end = turn.end - removed_before;
        if !is_foldable(&messages[start..=end]) {
            continue;
        }
        let folded = AgentModelMessage {
            role: AgentModelRole::User,
            parts: vec![AgentModelContentPart::Text {
                text: render_tool_turn(&messages[start..=end]),
            }],
            provider_metadata: Value::Null,
        };
        compaction.folded_turns += 1;
        compaction.folded_messages += end - start + 1;
        messages.splice(start..=end, std::iter::once(folded));
        removed_before += end - start;
    }

    compaction
}

/// Inclusive index range of one tool turn: the assistant message carrying the
/// calls, through the last tool message answering them.
struct ToolTurnSpan {
    start: usize,
    end: usize,
}

fn tool_turn_spans(messages: &[AgentModelMessage]) -> Vec<ToolTurnSpan> {
    let mut spans = Vec::new();
    let mut index = 0_usize;
    while index < messages.len() {
        let is_call = messages[index].role == AgentModelRole::Assistant
            && messages[index]
                .parts
                .iter()
                .any(|part| matches!(part, AgentModelContentPart::ToolCall { .. }));
        if !is_call {
            index += 1;
            continue;
        }
        let mut end = index;
        while messages
            .get(end + 1)
            .is_some_and(|message| message.role == AgentModelRole::Tool)
        {
            end += 1;
        }
        if end > index {
            spans.push(ToolTurnSpan { start: index, end });
        }
        index = end + 1;
    }
    spans
}

/// A turn folds only when every part it holds has a text rendering. Anything
/// else (media, resource refs, provider-native payloads) has no lossless text
/// form, so that turn stays native rather than losing content to the fold.
fn is_foldable(turn: &[AgentModelMessage]) -> bool {
    turn.iter().all(|message| {
        message.parts.iter().all(|part| {
            matches!(
                part,
                AgentModelContentPart::Text { .. }
                    | AgentModelContentPart::Reasoning { .. }
                    | AgentModelContentPart::ToolCall { .. }
                    | AgentModelContentPart::ToolResult { .. }
            )
        })
    })
}

fn render_tool_turn(turn: &[AgentModelMessage]) -> String {
    // A call that answered with an error left nothing behind anywhere else — the
    // state document never received its values — so its arguments stay whole.
    let failed_calls: HashSet<&str> = turn
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } if result.is_error => {
                Some(result.call_id.as_str())
            }
            _ => None,
        })
        .collect();

    let mut lines = vec![FOLDED_TURN_MARKER.to_string()];
    for message in turn {
        let speaker = speaker_label(message.role);
        for part in &message.parts {
            match part {
                // Reasoning is this turn's own scratchpad for calls that are
                // already resolved; carrying it forward is dead weight.
                AgentModelContentPart::Reasoning { .. } => {}
                AgentModelContentPart::Text { text } => {
                    if !text.trim().is_empty() {
                        lines.push(format!("[{speaker}] {text}"));
                    }
                }
                AgentModelContentPart::ToolCall { call } => {
                    let collapse = !failed_calls.contains(call.call_id.as_str());
                    lines.push(format!("[call] {}", render_invocation(call, collapse)));
                }
                AgentModelContentPart::ToolResult { result } => {
                    let marker = if result.is_error { "[error]" } else { "[result]" };
                    lines.push(format!("{marker} {}", result.content));
                }
                // Unreachable in practice: `is_foldable` refuses such turns.
                _ => {}
            }
        }
    }
    lines.join("\n")
}

fn render_invocation(call: &ToolInvocation, collapse_arguments_to_keys: bool) -> String {
    let arguments = match &call.arguments {
        ToolArguments::Object(arguments)
            if collapse_arguments_to_keys
                && ARGUMENT_KEYS_ONLY_TOOLS.contains(&call.tool_id.native_name()) =>
        {
            let keys = arguments
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{keys}}}")
        }
        ToolArguments::Object(arguments) => Value::Object(arguments.clone()).to_string(),
        ToolArguments::Invalid(raw) => raw.clone(),
    };
    format!("{} {arguments}", call.tool_id.native_name())
}

fn speaker_label(role: AgentModelRole) -> &'static str {
    match role {
        AgentModelRole::System => "system",
        AgentModelRole::Developer => "developer",
        AgentModelRole::User => "user",
        AgentModelRole::Assistant => "assistant",
        AgentModelRole::Tool => "tool",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ARGUMENT_KEYS_ONLY_TOOLS, compact_tool_transcript};
    use crate::models::agent::profile::DEFAULT_AGENT_TOOL_UNFOLDED_TURNS;
    use crate::models::agent::{AgentModelContentPart, AgentModelMessage, AgentModelRole};
    use crate::models::tool::{ToolArguments, ToolId, ToolInvocation, ToolProviderId};

    fn text(role: AgentModelRole, text: &str) -> AgentModelMessage {
        AgentModelMessage {
            role,
            parts: vec![AgentModelContentPart::Text {
                text: text.to_string(),
            }],
            provider_metadata: serde_json::Value::Null,
        }
    }

    fn builtin_tool(native_name: &str) -> ToolId {
        ToolId::new(&ToolProviderId::builtin(), native_name).expect("tool id")
    }

    fn call(tool: &str, call_id: &str, arguments: serde_json::Value) -> AgentModelContentPart {
        let object = match arguments {
            serde_json::Value::Object(map) => map,
            other => panic!("arguments must be an object, got {other}"),
        };
        AgentModelContentPart::ToolCall {
            call: ToolInvocation {
                call_id: call_id.to_string(),
                tool_id: builtin_tool(tool),
                arguments: ToolArguments::Object(object),
                provider_metadata: serde_json::Value::Null,
            },
        }
    }

    fn result(tool: &str, call_id: &str, content: &str) -> AgentModelMessage {
        AgentModelMessage {
            role: AgentModelRole::Tool,
            parts: vec![AgentModelContentPart::ToolResult {
                result: crate::models::agent::AgentToolResult {
                    call_id: call_id.to_string(),
                    tool_id: builtin_tool(tool),
                    content: content.to_string(),
                    structured: serde_json::Value::Null,
                    is_error: false,
                    error_code: None,
                    resource_refs: Vec::new(),
                },
            }],
            provider_metadata: serde_json::Value::Null,
        }
    }

    fn error_result(tool: &str, call_id: &str, content: &str) -> AgentModelMessage {
        let mut message = result(tool, call_id, content);
        if let Some(AgentModelContentPart::ToolResult { result }) = message.parts.first_mut() {
            result.is_error = true;
        }
        message
    }

    /// One native tool turn: an assistant call message plus its results.
    fn tool_turn(index: usize, tool: &str) -> Vec<AgentModelMessage> {
        let call_id = format!("call-{index}");
        vec![
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![
                    AgentModelContentPart::Text {
                        text: format!("looking at turn {index}"),
                    },
                    call(tool, &call_id, json!({ "index": index })),
                ],
                provider_metadata: serde_json::Value::Null,
            },
            result(tool, &call_id, &format!("body of turn {index}")),
        ]
    }

    #[test]
    fn keeps_the_newest_turn_native() {
        let mut messages = vec![text(AgentModelRole::User, "start")];
        messages.extend(tool_turn(1, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, 2);

        assert_eq!(compaction.folded_turns, 0);
        assert_eq!(messages.len(), 3);
        assert!(messages[1]
            .parts
            .iter()
            .any(|part| matches!(part, AgentModelContentPart::ToolCall { .. })));
    }

    #[test]
    fn folds_turns_outside_the_window_and_keeps_result_bodies() {
        let mut messages = vec![text(AgentModelRole::User, "start")];
        messages.extend(tool_turn(1, "workspace.read_file"));
        messages.extend(tool_turn(2, "workspace.read_file"));
        messages.extend(tool_turn(3, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, 2);

        assert_eq!(compaction.folded_turns, 1);
        assert_eq!(compaction.folded_messages, 2);
        // One user message, one folded turn, then two native turns.
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].role, AgentModelRole::User);
        let folded = match &messages[1].parts[..] {
            [AgentModelContentPart::Text { text }] => text.as_str(),
            other => panic!("expected one text part, got {other:?}"),
        };
        assert!(folded.starts_with("[folded tool turn]"));
        assert!(folded.contains("[assistant] looking at turn 1"));
        assert!(folded.contains("[call] workspace.read_file {\"index\":1}"));
        // The read body is the material the reply is built from: never trimmed.
        assert!(folded.contains("[result] body of turn 1"));
        assert!(!folded.contains("call-1"));
    }

    #[test]
    fn folding_is_idempotent() {
        let mut messages = Vec::new();
        messages.extend(tool_turn(1, "workspace.read_file"));
        messages.extend(tool_turn(2, "state.update"));

        let first = compact_tool_transcript(&mut messages, 1);
        let once = messages.clone();
        let second = compact_tool_transcript(&mut messages, 1);

        assert_eq!(first.folded_turns, 1);
        assert_eq!(second.folded_turns, 0);
        assert_eq!(messages, once);
    }

    #[test]
    fn folds_only_one_new_turn_per_pass() {
        // Round 4: turn 1 is already folded, so only turn 2 crosses the line.
        let mut messages = vec![text(AgentModelRole::User, "[folded tool turn]")];
        messages.extend(tool_turn(2, "workspace.read_file"));
        messages.extend(tool_turn(3, "workspace.read_file"));
        messages.extend(tool_turn(4, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, 2);

        assert_eq!(compaction.folded_turns, 1);
        assert!(matches!(
            &messages[1].parts[..],
            [AgentModelContentPart::Text { .. }]
        ));
        assert!(matches!(
            messages[2].parts.last(),
            Some(AgentModelContentPart::ToolCall { .. })
        ));
    }

    #[test]
    fn window_never_drops_below_one_native_turn() {
        let mut messages = Vec::new();
        messages.extend(tool_turn(1, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, 0);

        assert_eq!(compaction.folded_turns, 0);
        assert!(matches!(
            messages[0].parts.last(),
            Some(AgentModelContentPart::ToolCall { .. })
        ));
    }

    #[test]
    fn write_tool_arguments_collapse_to_key_names() {
        assert!(ARGUMENT_KEYS_ONLY_TOOLS.contains(&"state.update"));

        let call_id = "call-write".to_string();
        let mut messages = vec![
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![call(
                    "state.update",
                    &call_id,
                    json!({ "updates": [{ "key": "环境/日期", "value": "三月三日" }] }),
                )],
                provider_metadata: serde_json::Value::Null,
            },
            result("state.update", &call_id, "ok"),
        ];
        messages.extend(tool_turn(2, "workspace.read_file"));

        compact_tool_transcript(&mut messages, 1);

        let folded = match &messages[0].parts[..] {
            [AgentModelContentPart::Text { text }] => text.clone(),
            other => panic!("expected one text part, got {other:?}"),
        };
        assert!(folded.contains("[call] state.update {updates}"));
        assert!(!folded.contains("三月三日"));
    }

    #[test]
    fn a_failed_write_keeps_the_arguments_it_tried() {
        // The values never reached the document, so the folded turn is the only
        // record left of what the model attempted.
        let call_id = "call-failed".to_string();
        let mut messages = vec![
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![call(
                    "state.update",
                    &call_id,
                    json!({ "updates": [{ "key": "环境/日期", "value": "三月三日" }] }),
                )],
                provider_metadata: serde_json::Value::Null,
            },
            error_result("state.update", &call_id, "state.undeclared_key"),
        ];
        messages.extend(tool_turn(2, "workspace.read_file"));

        compact_tool_transcript(&mut messages, 1);

        let folded = match &messages[0].parts[..] {
            [AgentModelContentPart::Text { text }] => text.clone(),
            other => panic!("expected one text part, got {other:?}"),
        };
        assert!(folded.contains("三月三日"));
    }

    #[test]
    fn leaves_turns_with_unsupported_parts_native() {
        let call_id = "call-media".to_string();
        let mut messages = vec![
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![
                    call("workspace.read_file", &call_id, json!({ "path": "a.md" })),
                    AgentModelContentPart::Media {
                        mime_type: "image/png".to_string(),
                        value: serde_json::Value::String("data".to_string()),
                    },
                ],
                provider_metadata: serde_json::Value::Null,
            },
            result("workspace.read_file", &call_id, "body"),
        ];
        messages.extend(tool_turn(2, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, 1);

        // The media part survives untouched because its turn is not folded.
        assert_eq!(compaction.folded_turns, 0);
        assert!(messages[0]
            .parts
            .iter()
            .any(|part| matches!(part, AgentModelContentPart::Media { .. })));
    }

    #[test]
    fn default_window_leaves_two_turns_native() {
        let mut messages = Vec::new();
        messages.extend(tool_turn(1, "workspace.read_file"));
        messages.extend(tool_turn(2, "workspace.read_file"));
        messages.extend(tool_turn(3, "workspace.read_file"));

        let compaction = compact_tool_transcript(&mut messages, DEFAULT_AGENT_TOOL_UNFOLDED_TURNS);

        assert_eq!(compaction.folded_turns, 1);
    }

    #[test]
    fn invalid_arguments_are_kept_verbatim() {
        let call_id = "call-invalid".to_string();
        let mut messages = vec![
            AgentModelMessage {
                role: AgentModelRole::Assistant,
                parts: vec![AgentModelContentPart::ToolCall {
                    call: ToolInvocation {
                        call_id: call_id.clone(),
                        tool_id: builtin_tool("workspace.read_file"),
                        arguments: ToolArguments::Invalid("not json at all".to_string()),
                        provider_metadata: serde_json::Value::Null,
                    },
                }],
                provider_metadata: serde_json::Value::Null,
            },
            result("workspace.read_file", &call_id, "body"),
        ];
        messages.extend(tool_turn(2, "workspace.read_file"));

        compact_tool_transcript(&mut messages, 1);

        let folded = match &messages[0].parts[..] {
            [AgentModelContentPart::Text { text }] => text.clone(),
            other => panic!("expected one text part, got {other:?}"),
        };
        assert!(folded.contains("[call] workspace.read_file not json at all"));
    }

    #[test]
    fn empty_transcript_is_untouched() {
        let mut messages: Vec<AgentModelMessage> = Vec::new();
        let compaction = compact_tool_transcript(&mut messages, 2);
        assert_eq!(compaction.folded_turns, 0);
        assert!(messages.is_empty());
    }
}
