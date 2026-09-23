//! Turning state into prompt text: which fields travel, where they land.
//!
//! The model never pulls state; whatever this module renders is what it knows
//! exists. "The good way to stop an AI from doing something is not to tell it
//! about it" — so the filter here is the access grant, and it defaults to off.
//!
//! Placement is decided by how often a field changes, not by how important it
//! is. Static content can sit in the prompt prefix and stay in the cache; content
//! that changes every turn has to sit near the end, or it breaks the whole cached
//! prefix on every round.

use serde::{Deserialize, Serialize};

use super::state::{StateDeclaration, StateDocument, StateKey};
use super::state_access::{FieldAccess, StateAccessPolicy, resolve_access};

/// How far from the end of the chat an at-depth block is placed when the entry
/// does not say. Deep enough to read as "current situation", shallow enough to
/// stay out of the cached prefix.
pub const DEFAULT_INJECT_DEPTH: usize = 4;

/// Where an injected state slice is placed in the assembled prompt.
///
/// The vocabulary is the one world book entries already use, so placement is
/// configured the same way and the host places blocks with the same machinery.
/// The slots that are not listed here are not supported yet, and a policy that
/// names one is refused when it is parsed rather than silently dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateInjectionSlot {
    /// Before the main prompt: for state that rarely changes.
    Before,
    /// After the main prompt.
    After,
    /// A fixed number of messages from the end of the chat: for state that
    /// changes every turn.
    #[default]
    AtDepth,
}

/// One place in the prompt and the text that goes there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInjectionBlock {
    pub slot: StateInjectionSlot,
    /// Messages from the end of the chat. Only meaningful for `AtDepth`.
    pub depth: usize,
    /// The state keys this block carries, in the order they were rendered.
    ///
    /// Carried separately from the text so a run journal or a panel can say what
    /// the model was told without parsing the block back.
    pub keys: Vec<String>,
    pub text: String,
}

/// Render every field that resolves to injectable, grouped by where it goes.
///
/// Injection is off unless it is asked for, wherever the asking happens: a
/// Profile row that turns it on, or a declared field whose own default has it on.
/// What a browser of the field list sees is therefore the truth — a field with
/// injection off tells the model nothing, and one with it on travels every turn.
///
/// A field whose value was cleared carries nothing and is left out: an empty
/// line would tell the model a field exists and has no value, which is what the
/// declaration is for, not the injection.
pub fn render_injection(
    document: &StateDocument,
    policy: &StateAccessPolicy,
    declaration: &StateDeclaration,
) -> Vec<StateInjectionBlock> {
    if policy.is_empty() && declaration.is_empty() {
        return Vec::new();
    }

    // Rendered order is the key order, not the order the fields happened to be
    // written in: the same state must always produce the same prompt text.
    let mut fields = document.fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.key.cmp(&right.key));

    let mut blocks: Vec<StateInjectionBlock> = Vec::new();
    for field in fields {
        if field.values.is_empty() {
            continue;
        }
        let Some(placement) = placement_for(policy, declaration, &field.key) else {
            continue;
        };
        let line = format!(
            "{}: {}",
            render_name(document, declaration, &field.key),
            joined_values(&field.values),
        );
        match blocks
            .iter_mut()
            .find(|block| block.slot == placement.slot && block.depth == placement.depth)
        {
            Some(block) => {
                block.text.push('\n');
                block.text.push_str(&line);
                block.keys.push(field.key.as_str().to_string());
            }
            None => blocks.push(StateInjectionBlock {
                slot: placement.slot,
                depth: placement.depth,
                keys: vec![field.key.as_str().to_string()],
                text: line,
            }),
        }
    }

    blocks
}

/// The values as one readable run, with a repeated value said once.
///
/// A model that writes `["白天", "白天"]` means one thing; saying it twice only
/// spends prompt. First occurrence wins, so the text stays a function of the
/// document.
fn joined_values(values: &[String]) -> String {
    let mut seen: Vec<&str> = Vec::with_capacity(values.len());
    for value in values {
        if !seen.contains(&value.as_str()) {
            seen.push(value);
        }
    }
    seen.join(", ")
}

/// What a field is called in the prompt: its display name when that is
/// unambiguous, its key path otherwise.
///
/// The rule is deliberately narrow. A display name is used only when the
/// declaration names this exact key as a literal and no other injected field
/// shares that name — `环境/日期` reads as `日期: 2026/09/10`. A key that a
/// wildcard covered keeps its path, because the path is where the entity name
/// lives: `角色/甲/好感度` says *whose* affection it is, and no label can.
fn render_name(document: &StateDocument, declaration: &StateDeclaration, key: &StateKey) -> String {
    let mut literal_label: Option<&str> = None;
    for field in &declaration.fields {
        if !field.pattern.is_literal() || !field.pattern.matches_exact(key.as_str()) {
            continue;
        }
        match literal_label {
            // Two declarations answering to the same name say nothing about
            // which one this is, so the path stays.
            Some(existing) if existing != field.label => return key.as_str().to_string(),
            Some(_) => {}
            None => literal_label = Some(field.label.as_str()),
        }
    }

    let Some(label) = literal_label else {
        return key.as_str().to_string();
    };
    let label = label.trim();
    if label.is_empty() {
        return key.as_str().to_string();
    }

    // A name two fields would share is not a name for either of them.
    let shared = document
        .fields
        .iter()
        .filter(|other| other.key != *key)
        .filter(|other| declared_literal_label(declaration, &other.key).as_deref() == Some(label))
        .count();
    if shared > 0 {
        return key.as_str().to_string();
    }

    label.to_string()
}

/// The label a key was declared under, when the declaration names it literally.
fn declared_literal_label(declaration: &StateDeclaration, key: &StateKey) -> Option<String> {
    declaration
        .fields
        .iter()
        .find(|field| field.pattern.is_literal() && field.pattern.matches_exact(key.as_str()))
        .map(|field| field.label.trim().to_string())
        .filter(|label| !label.is_empty())
}

struct Placement {
    slot: StateInjectionSlot,
    depth: usize,
}

fn placement_for(
    policy: &StateAccessPolicy,
    declaration: &StateDeclaration,
    key: &StateKey,
) -> Option<Placement> {
    match resolve_access(policy, declaration, key) {
        FieldAccess::Configured(access) if access.inject => Some(Placement {
            slot: access.inject_slot,
            depth: access.inject_depth,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{StateInjectionBlock, StateInjectionSlot, render_injection};
    use crate::models::state::{
        DeclaredStateField, StateDeclaration, StateDocument, StateField, StateKey,
    };
    use crate::models::state_access::{StateAccessEntry, StateAccessPolicy, StateFieldAccess};
    use crate::models::state_key::StateKeyPattern;

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    fn field(raw: &str, values: &[&str]) -> StateField {
        StateField {
            key: key(raw),
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn entry(
        pattern: &str,
        inject: bool,
        slot: StateInjectionSlot,
        depth: usize,
    ) -> StateAccessEntry {
        StateAccessEntry {
            pattern: StateKeyPattern::parse(pattern)
                .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
            inject,
            visible: false,
            writable: false,
            inject_slot: slot,
            inject_depth: depth,
        }
    }

    fn document(fields: Vec<StateField>) -> StateDocument {
        StateDocument { fields }
    }

    fn policy(entries: Vec<StateAccessEntry>) -> StateAccessPolicy {
        StateAccessPolicy { entries }
    }

    /// No declaration: these tests are about the Profile's own policy.
    fn declaration_only_nothing() -> StateDeclaration {
        StateDeclaration::default()
    }

    /// One declared row, with the switches the tests do not vary.
    fn declared(pattern: &str, label: &str) -> DeclaredStateField {
        DeclaredStateField {
            pattern: StateKeyPattern::parse(pattern).expect("test pattern must parse"),
            label: label.to_string(),
            access: StateFieldAccess::NONE,
            initial: Vec::new(),
        }
    }

    #[test]
    fn an_unconfigured_profile_injects_nothing() {
        let document = document(vec![field("环境/日期", &["2026/09/10"])]);

        assert!(
            render_injection(&document, &StateAccessPolicy::default(), &declaration_only_nothing()).is_empty(),
            "injection defaults to off, so an unconfigured Profile tells the model nothing"
        );
    }

    #[test]
    fn a_key_no_entry_covers_is_not_injected() {
        let document = document(vec![
            field("环境/日期", &["2026/09/10"]),
            field("环境/地点", &["咖啡馆"]),
        ]);
        // The depth of a `Before` entry is irrelevant; this one carries the
        // default.
        let policy = policy(vec![entry(
            "环境/日期",
            true,
            StateInjectionSlot::Before,
            4,
        )]);

        let blocks = render_injection(&document, &policy, &declaration_only_nothing());

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].keys, ["环境/日期"]);
        assert_eq!(blocks[0].text, "环境/日期: 2026/09/10");
    }

    #[test]
    fn fields_that_share_a_slot_are_rendered_as_one_block() {
        let document = document(vec![
            // Written out of order on purpose: the rendered order must be the
            // key order, so the same state always produces the same text.
            field("环境/地点", &["咖啡馆"]),
            field("环境/日期", &["2026/09/10"]),
        ]);
        let policy = policy(vec![entry(
            "环境/*",
            true,
            StateInjectionSlot::AtDepth,
            6,
        )]);

        let blocks = render_injection(&document, &policy, &declaration_only_nothing());

        assert_eq!(
            blocks,
            vec![StateInjectionBlock {
                slot: StateInjectionSlot::AtDepth,
                depth: 6,
                // Key order, not the order the document happened to hold.
                keys: vec!["环境/地点".to_string(), "环境/日期".to_string()],
                text: "环境/地点: 咖啡馆\n环境/日期: 2026/09/10".to_string(),
            }]
        );
    }

    #[test]
    fn different_slots_produce_different_blocks() {
        let document = document(vec![
            field("角色/角色甲/外貌", &["短发"]),
            field("环境/时间", &["下午"]),
        ]);
        let policy = policy(vec![
            entry("角色/*/外貌", true, StateInjectionSlot::Before, 0),
            entry("环境/时间", true, StateInjectionSlot::AtDepth, 2),
        ]);

        let blocks = render_injection(&document, &policy, &declaration_only_nothing());

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].slot, StateInjectionSlot::AtDepth);
        assert_eq!(blocks[0].depth, 2);
        assert_eq!(blocks[0].keys, ["环境/时间"]);
        assert_eq!(blocks[1].slot, StateInjectionSlot::Before);
        assert_eq!(blocks[1].keys, ["角色/角色甲/外貌"]);
    }

    #[test]
    fn a_field_that_was_cleared_has_nothing_to_inject() {
        let document = document(vec![field("环境/地点", &[])]);
        let policy = policy(vec![entry(
            "环境/地点",
            true,
            StateInjectionSlot::AtDepth,
            4,
        )]);

        assert!(render_injection(&document, &policy, &declaration_only_nothing()).is_empty());
    }

    #[test]
    fn a_declared_field_can_ask_to_be_injected_itself() {
        // The field carries the switch, so a chat whose Profile configured
        // nothing at all can still push the values the scene needs every turn.
        let document = document(vec![field("环境/日期", &["2026/09/10"])]);
        let declaration = StateDeclaration {
            fields: vec![DeclaredStateField {
                pattern: StateKeyPattern::parse("环境/日期").expect("pattern"),
                label: "日期".to_string(),
                access: StateFieldAccess {
                    inject: true,
                    visible: true,
                    writable: true,
                    inject_slot: StateInjectionSlot::AtDepth,
                    inject_depth: 2,
                },
                initial: Vec::new(),
            }],
            ..Default::default()
        };

        let blocks = render_injection(&document, &StateAccessPolicy::default(), &declaration);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].keys, ["环境/日期"]);
        assert_eq!(blocks[0].depth, 2);
    }

    #[test]
    fn the_declared_default_tells_the_model_this_field() {
        // Writing a field down is the statement that the model should know it, so
        // the value goes in every turn; the Profile is where a field is muted.
        let document = document(vec![field("环境/日期", &["2026/09/10"])]);
        let declaration = StateDeclaration {
            fields: vec![DeclaredStateField {
                pattern: StateKeyPattern::parse("环境/日期").expect("pattern"),
                label: "日期".to_string(),
                access: StateFieldAccess::DECLARED,
                initial: Vec::new(),
            }],
            ..Default::default()
        };

        let blocks = render_injection(&document, &StateAccessPolicy::default(), &declaration);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].keys, ["环境/日期"]);

        let muted = StateAccessPolicy {
            entries: vec![StateAccessEntry {
                pattern: StateKeyPattern::parse("环境/日期").expect("pattern"),
                inject: false,
                visible: false,
                writable: false,
                inject_slot: StateInjectionSlot::AtDepth,
                inject_depth: crate::models::state_injection::DEFAULT_INJECT_DEPTH,
            }],
        };

        assert!(
            render_injection(&document, &muted, &declaration).is_empty()
        );
    }

    #[test]
    fn a_value_the_document_repeats_is_said_once() {
        let document = document(vec![field("环境/时间", &["白天", "白天", "夜晚"])]);
        let policy = policy(vec![entry(
            "环境/时间",
            true,
            StateInjectionSlot::AtDepth,
            4,
        )]);

        let blocks = render_injection(&document, &policy, &declaration_only_nothing());

        assert_eq!(blocks[0].text, "环境/时间: 白天, 夜晚");
    }

    #[test]
    fn a_literally_declared_field_is_called_by_its_display_name() {
        let document = document(vec![
            field("环境/日期", &["2026/09/10"]),
            // Covered by a pattern, so it keeps its path: `角色/甲/好感度` names
            // whose affection it is, and no label can.
            field("角色/甲/好感度", &["42"]),
        ]);
        let declaration = StateDeclaration {
            fields: vec![
                declared("环境/日期", "日期"),
                declared("角色/*/好感度", "好感度"),
            ],
            ..Default::default()
        };
        let policy = policy(vec![entry(
            "环境/**",
            true,
            StateInjectionSlot::AtDepth,
            4,
        ), entry("角色/**", true, StateInjectionSlot::AtDepth, 4)]);

        let blocks = render_injection(&document, &policy, &declaration);

        assert_eq!(blocks[0].text, "日期: 2026/09/10\n角色/甲/好感度: 42");
    }

    #[test]
    fn a_display_name_two_fields_would_share_is_not_used() {
        let document = document(vec![
            field("环境/日期", &["2026/09/10"]),
            field("世界/日期", &["第三天"]),
        ]);
        let declaration = StateDeclaration {
            fields: vec![declared("环境/日期", "日期"), declared("世界/日期", "日期")],
            ..Default::default()
        };
        let policy = policy(vec![entry(
            "环境/日期",
            true,
            StateInjectionSlot::AtDepth,
            4,
        ), entry("世界/日期", true, StateInjectionSlot::AtDepth, 4)]);

        let blocks = render_injection(&document, &policy, &declaration);

        assert_eq!(blocks[0].text, "世界/日期: 第三天\n环境/日期: 2026/09/10");
    }

    #[test]
    fn the_inject_switch_is_what_puts_a_field_in_the_prompt() {
        let document = document(vec![field("环境/日期", &["2026/09/10"])]);
        // Writable and visible, but not injectable: the model may retrieve it or
        // change it, and still never has it pushed into its context.
        let policy = policy(vec![StateAccessEntry {
            pattern: StateKeyPattern::parse("环境/*").expect("pattern"),
            inject: false,
            visible: true,
            writable: true,
            inject_slot: StateInjectionSlot::Before,
            inject_depth: 0,
        }]);

        assert!(render_injection(&document, &policy, &declaration_only_nothing()).is_empty());
    }
}
