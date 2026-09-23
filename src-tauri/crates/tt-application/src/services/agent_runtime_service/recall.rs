//! What a run does with the recall its chat's extensions produced.
//!
//! A recall block is written before a run starts, by an extension, and frozen
//! with the rest of the run's input: the same question is asked of the same index
//! with the same context, and every invocation of that run reads the same answer.
//! Asking again would buy the same answer for another round trip and one more
//! chance for two answers to disagree, so nothing here retrieves — a run decides
//! only whether its prompt carries what was already recalled.
//!
//! Two decisions follow from that, and they are different decisions:
//!
//! * the run's own prompt: the blocks arrive already merged into the messages
//!   the assembly produced, so dropping them is a text operation;
//! * a delegated invocation: it builds its prompt from scratch (a system prompt
//!   and the task), so carrying them is an insertion.
//!
//! Nothing is trimmed on the way through. A recall block is the extension's
//! answer to "what does this story need to remember", and a budget applied here
//! would quietly turn that answer into a partial one — the failure mode of a
//! memory system that looks like it is working.

use serde_json::{Map, Value, json};

use tt_domain::models::agent::profile::AgentRecallPolicy;

/// The run-input key that carries this Profile's recall policy.
///
/// It travels with the frozen run input for the same reason access does: a
/// delegated invocation has to know what its run was allowed to carry.
pub(crate) const RECALL_SNAPSHOT_KEY: &str = "recall";

/// The text of every recall block the frozen run input carries, in key order.
///
/// The values are taken from the snapshot rather than from the live extension
/// prompt table: the run is governed by what it started with, and a block that
/// arrived later belongs to the next turn.
pub(crate) fn recall_block_texts(snapshot: &Value, policy: &AgentRecallPolicy) -> Vec<String> {
    let Some(prompts) = snapshot
        .pointer("/frozenRunInputSnapshot/promptInputs/extensionPrompts")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    prompts
        .iter()
        .filter(|(key, _)| policy.matches_source(key))
        .filter_map(|(_, prompt)| {
            let text = prompt.get("value").and_then(Value::as_str)?.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .collect()
}

/// Remove the recall blocks from a request payload's messages.
///
/// A message that becomes empty is dropped rather than sent blank: it existed to
/// carry the block. Returns how many blocks were found and how many messages
/// went away, because a caller that asked for the blocks to be dropped and saw
/// nothing dropped wants to know.
pub(crate) fn strip_recall_blocks(
    payload: &mut Map<String, Value>,
    blocks: &[String],
) -> (usize, usize) {
    if blocks.is_empty() {
        return (0, 0);
    }

    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return (0, 0);
    };

    let mut removed = 0_usize;
    let carried = messages.len();
    let mut kept = Vec::with_capacity(carried);
    for mut message in messages.drain(..) {
        let found = strip_blocks_from_message(&mut message, blocks);
        removed += found;
        if found > 0 && message_content_is_blank(&message) {
            continue;
        }
        kept.push(message);
    }
    let dropped = carried.saturating_sub(kept.len());
    *messages = kept;

    (removed, dropped)
}

/// Carry the parent's recall blocks into a delegated invocation's prompt.
///
/// They arrive as their own system message, between the sub-agent's own system
/// prompt and the task: the task stays the last thing said, and the memories stay
/// what they are — someone else's recollection, in the voice the recall
/// extension wrote them in.
pub(crate) fn inherit_recall_message(blocks: &[String]) -> Option<Value> {
    if blocks.is_empty() {
        return None;
    }

    Some(json!({
        "role": "system",
        "content": blocks.join("\n\n"),
    }))
}

/// Place material a sub-agent inherited after the prompt's own system message.
///
/// Appended rather than prepended: a prompt that opens with someone else's
/// recollection would have the sub-agent reading memories before it has been told
/// what it is.
pub(crate) fn insert_system_message_after_prompt(
    payload: &mut Map<String, Value>,
    message: Value,
) -> bool {
    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };

    let position = match messages.first() {
        Some(first) if first.get("role").and_then(Value::as_str) == Some("system") => 1,
        _ => 0,
    };
    messages.insert(position, message);
    true
}

fn strip_blocks_from_message(message: &mut Value, blocks: &[String]) -> usize {
    let Some(object) = message.as_object_mut() else {
        return 0;
    };

    match object.get_mut("content") {
        Some(Value::String(content)) => strip_blocks_from_text(content, blocks),
        Some(Value::Array(parts)) => parts
            .iter_mut()
            .map(|part| match part.get_mut("text") {
                Some(Value::String(text)) => strip_blocks_from_text(text, blocks),
                _ => 0,
            })
            .sum(),
        _ => 0,
    }
}

fn strip_blocks_from_text(text: &mut String, blocks: &[String]) -> usize {
    let mut removed = 0_usize;
    for block in blocks {
        while let Some(position) = text.find(block.as_str()) {
            text.replace_range(position..position + block.len(), "");
            removed += 1;
        }
    }

    if removed > 0 {
        *text = text.trim().to_string();
    }
    removed
}

fn message_content_is_blank(message: &Value) -> bool {
    match message.get("content") {
        Some(Value::String(content)) => content.trim().is_empty(),
        Some(Value::Array(parts)) => parts.iter().all(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_none_or(|text| text.trim().is_empty())
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        inherit_recall_message, insert_system_message_after_prompt, recall_block_texts,
        strip_recall_blocks,
    };
    use tt_domain::models::agent::profile::{AgentRecallInheritance, AgentRecallPolicy};

    fn policy(sources: &[&str]) -> AgentRecallPolicy {
        AgentRecallPolicy {
            inject: true,
            sources: sources.iter().map(|source| source.to_string()).collect(),
            subagent: AgentRecallInheritance::Skip,
        }
    }

    fn snapshot_with(prompts: serde_json::Value) -> serde_json::Value {
        json!({ "frozenRunInputSnapshot": { "promptInputs": { "extensionPrompts": prompts } } })
    }

    #[test]
    fn a_prefix_source_names_every_key_the_extension_writes() {
        let snapshot = snapshot_with(json!({
            "3_vectfox": { "value": "first block" },
            "3_vectfox_pos1": { "value": "second block" },
            "3_vectfox_eventbase": { "value": "third block" },
            "customWIOutlet_Lore": { "value": "not recall" },
        }));

        let blocks = recall_block_texts(&snapshot, &policy(&["3_vectfox*"]));

        assert_eq!(
            blocks,
            vec!["first block", "third block", "second block"],
            "only the recall extension's own keys are its blocks, in key order"
        );
    }

    #[test]
    fn an_empty_block_is_not_a_block() {
        let snapshot = snapshot_with(json!({ "3_vectfox": { "value": "   " } }));

        assert!(recall_block_texts(&snapshot, &policy(&["3_vectfox"])).is_empty());
    }

    #[test]
    fn dropping_a_block_leaves_the_message_it_shared() {
        let mut payload = json!({
            "messages": [
                { "role": "system", "content": "You are Aria." },
                { "role": "system", "content": "World notes\nRecalled: the tavern burned" },
                { "role": "user", "content": "Recalled: the tavern burned" },
                { "role": "user", "content": "What now?" },
            ]
        });
        let payload = payload.as_object_mut().expect("payload");
        let blocks = vec!["Recalled: the tavern burned".to_string()];

        let (occurrences, messages) = strip_recall_blocks(payload, &blocks);

        assert_eq!(occurrences, 2);
        assert_eq!(messages, 1, "the message that was only the block goes away");
        let kept = payload["messages"].as_array().expect("messages");
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[1]["content"], json!("World notes"));
        assert_eq!(kept[2]["content"], json!("What now?"));
    }

    #[test]
    fn a_blank_payload_is_left_alone() {
        let mut payload = json!({ "model": "gpt-4o" });
        let payload = payload.as_object_mut().expect("payload");

        assert_eq!(
            strip_recall_blocks(payload, &["anything".to_string()]),
            (0, 0)
        );
    }

    #[test]
    fn an_inherited_block_sits_after_the_sub_agents_own_system_prompt() {
        let mut payload = json!({
            "messages": [
                { "role": "system", "content": "You are a scene writer." },
                { "role": "user", "content": "Write the next scene." },
            ]
        });
        let payload = payload.as_object_mut().expect("payload");
        let message = inherit_recall_message(&["Recalled: the tavern burned".to_string()])
            .expect("a block becomes a message");

        assert!(insert_system_message_after_prompt(payload, message));

        let messages = payload["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], json!("system"));
        assert_eq!(messages[1]["content"], json!("Recalled: the tavern burned"));
        assert_eq!(messages[2]["content"], json!("Write the next scene."));
    }

    #[test]
    fn nothing_to_inherit_is_not_an_empty_message() {
        assert!(inherit_recall_message(&[]).is_none());
    }
}


