//! What a delegated invocation carries of the run's World Info.
//!
//! The scan happens once, before the run starts, against the chat: which entries
//! a book activates is a fact about the chat and the moment, not about an Agent.
//! It is recorded with the run's input, content and all, and every invocation
//! reads that record — so a SubAgent is given text the book actually contains,
//! word for word, rather than a Skill it has to decide to open or a summary of
//! something it never saw.
//!
//! Deciding is therefore all that is left here, and it is one decision per
//! Profile: whether a delegated invocation carries the run's entries, and which
//! of them it does not. Nothing is summarised, truncated, or re-scanned.

use serde_json::{Map, Value, json};

use tt_domain::models::agent::profile::AgentWorldInfoPolicy;

use super::recall::insert_system_message_after_prompt;

/// The run-input key that carries what the scan activated.
pub(crate) const WORLD_INFO_ACTIVATION_KEY: &str = "worldInfoActivation";

/// One entry as the scan recorded it.
///
/// `uid` stays a string: the frontend records numbers for ordinary books and
/// falls back to whatever the entry carries, so matching a rule to an entry is a
/// comparison of their written forms rather than of two number types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivatedWorldInfoEntry {
    pub book: String,
    pub uid: String,
    pub display_name: String,
    pub content: String,
}

/// Every entry the run's scan activated, in the order it recorded them.
pub(crate) fn activated_entries(snapshot: &Value) -> Vec<ActivatedWorldInfoEntry> {
    let Some(entries) = snapshot
        .get(WORLD_INFO_ACTIVATION_KEY)
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|entry| {
            let content = entry.get("content").and_then(Value::as_str)?.trim();
            if content.is_empty() {
                return None;
            }

            Some(ActivatedWorldInfoEntry {
                book: string_field(entry, "world"),
                uid: uid_field(entry),
                display_name: string_field(entry, "displayName"),
                content: content.to_string(),
            })
        })
        .collect()
}

/// The entries this Profile's SubAgent use carries.
///
/// The switch answers for entries with no row of their own; a row is an exception
/// for one entry, in both directions — the same table the Profile's own prompt is
/// filtered by, read against this switch instead of the chat-wide one.
pub(crate) fn entries_for_subagent(
    policy: &AgentWorldInfoPolicy,
    entries: Vec<ActivatedWorldInfoEntry>,
) -> Vec<ActivatedWorldInfoEntry> {
    let default = policy.subagent_inherits;

    entries
        .into_iter()
        .filter(|entry| {
            policy
                .entries
                .iter()
                .find(|rule| {
                    rule.book.trim() == entry.book && rule.uid.to_string() == entry.uid
                })
                .map(|rule| rule.inject)
                .unwrap_or(default)
        })
        .collect()
}

/// Render the carried entries as the prompt text for a delegated invocation.
///
/// One message rather than one per entry: what a SubAgent needs is the material,
/// and separating it into a message per entry would suggest a structure the book
/// did not have. Returns `None` when there is nothing to carry, so the caller can
/// leave the prompt untouched.
pub(crate) fn world_info_message(entries: &[ActivatedWorldInfoEntry]) -> Option<Value> {
    if entries.is_empty() {
        return None;
    }

    let content = entries
        .iter()
        .map(|entry| entry.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(json!({ "role": "system", "content": content }))
}

/// Put the carried entries after the invocation's own system prompt.
pub(crate) fn insert_world_info_message(
    payload: &mut Map<String, Value>,
    message: Value,
) -> bool {
    insert_system_message_after_prompt(payload, message)
}

fn string_field(entry: &Value, field: &str) -> String {
    entry
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn uid_field(entry: &Value) -> String {
    match entry.get("uid") {
        Some(Value::String(uid)) => uid.clone(),
        Some(Value::Number(uid)) => uid.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WORLD_INFO_ACTIVATION_KEY, activated_entries, entries_for_subagent, world_info_message,
    };
    use serde_json::json;
    use tt_domain::models::agent::profile::{AgentWorldInfoEntryRule, AgentWorldInfoPolicy};

    fn snapshot(entries: serde_json::Value) -> serde_json::Value {
        json!({ WORLD_INFO_ACTIVATION_KEY: { "entries": entries } })
    }

    fn policy(subagent_inherits: bool, rules: Vec<(u64, bool)>) -> AgentWorldInfoPolicy {
        AgentWorldInfoPolicy {
            entries: rules
                .into_iter()
                .map(|(uid, inject)| AgentWorldInfoEntryRule {
                    book: "Char Lore".to_string(),
                    uid,
                    inject,
                })
                .collect(),
            subagent_inherits,
        }
    }

    #[test]
    fn the_recorded_entries_are_read_with_their_text() {
        let entries = activated_entries(&snapshot(json!([
            { "world": "Char Lore", "uid": 7, "displayName": "House rules", "content": "Always answer in the character's voice." },
            { "world": "Char Lore", "uid": 8, "content": "   " },
        ])));

        assert_eq!(entries.len(), 1, "an entry with no text is not an entry");
        assert_eq!(entries[0].book, "Char Lore");
        assert_eq!(entries[0].uid, "7");
        assert_eq!(entries[0].content, "Always answer in the character's voice.");
    }

    #[test]
    fn an_entry_the_book_recorded_as_a_string_id_still_matches_its_rule() {
        let entries = activated_entries(&snapshot(json!([
            { "world": "Char Lore", "uid": "7", "content": "text" },
        ])));

        let carried = entries_for_subagent(&policy(false, vec![(7, true)]), entries);

        assert_eq!(carried.len(), 1, "the rule names the entry, not its type");
    }

    #[test]
    fn a_subagent_carries_nothing_until_its_profile_says_so() {
        let entries = activated_entries(&snapshot(json!([
            { "world": "Char Lore", "uid": 7, "content": "one" },
            { "world": "Char Lore", "uid": 8, "content": "two" },
        ])));

        assert!(
            entries_for_subagent(&policy(false, vec![]), entries.clone()).is_empty(),
            "the standing answer is what a SubAgent gets today: nothing"
        );
        assert_eq!(
            entries_for_subagent(&policy(false, vec![(7, true)]), entries.clone()).len(),
            1,
            "a row lets one entry through while the switch says no"
        );
        assert_eq!(
            entries_for_subagent(&policy(true, vec![]), entries.clone()).len(),
            2,
            "the switch on carries them all"
        );
        assert_eq!(
            entries_for_subagent(&policy(true, vec![(8, false)]), entries).len(),
            1,
            "a row keeps one out while the switch says yes"
        );
    }

    #[test]
    fn a_rule_for_another_book_does_not_decide_this_one() {
        let entries = activated_entries(&snapshot(json!([
            { "world": "Other Book", "uid": 7, "content": "text" },
        ])));

        let carried = entries_for_subagent(&policy(true, vec![(7, false)]), entries);

        assert_eq!(carried.len(), 1, "the rule names a book as well as an entry");
    }

    #[test]
    fn nothing_to_carry_is_not_an_empty_message() {
        assert!(world_info_message(&[]).is_none());
    }
}
