//! Per-field access to state: who may inject, retrieve, or write a field.
//!
//! Access is configured per Profile and the granularity is the declared entry —
//! a literal name, or a pattern such as `角色/*/着装`. That granularity is the
//! point: a relationship score or an act marker entering the main Agent's
//! context makes the character perform the number instead of the person, so
//! injection has to be decided per field.
//!
//! **Defaults come from the field, exceptions come from the Profile.** A
//! declared field is state the scene tracks, so writing it down is the statement
//! that the model should know it: injection is on by default. The two switches
//! that hand out extra authority stay off — visibility lets the model pull on
//! any turn, and writability lets it change the value, and neither should follow
//! from a declaration alone.
//!
//! A Profile entry that matches is an exception in both directions, including a
//! switch left off (an explicit refusal). A key no entry and no declared field
//! covers is answered by the Profile's own reading: nothing configured means "not
//! configured yet" and constrains nothing, the same reading the empty
//! declaration gets for keys.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::state::{ResolvedStateUpdateRequest, StateDeclaration, StateUpdateError};
use super::state_injection::{DEFAULT_INJECT_DEPTH, StateInjectionSlot};
use super::state_key::{PatternOverlap, StateKey, StateKeyPattern, check_pattern_overlaps};

/// What one Profile may do with one declared field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateFieldAccess {
    /// The value is pushed into this Profile's context, without a tool call.
    #[serde(default)]
    pub inject: bool,
    /// The field can be retrieved with tools, but is never injected.
    ///
    /// Visibility is the harder switch of the two: injection happens once, at a
    /// known place, inside a budget, while visibility lets the model pull on any
    /// turn. It defaults to off for that reason.
    #[serde(default)]
    pub visible: bool,
    /// The Profile may change this field.
    #[serde(default)]
    pub writable: bool,
    /// Where the injected value lands. Only meaningful when `inject` is set.
    #[serde(default)]
    pub inject_slot: StateInjectionSlot,
    /// Messages from the end of the chat, for the `AtDepth` slot.
    #[serde(default = "default_inject_depth")]
    pub inject_depth: usize,
}

impl StateFieldAccess {
    /// The state of an entry that grants nothing.
    pub const NONE: Self = Self {
        inject: false,
        visible: false,
        writable: false,
        inject_slot: StateInjectionSlot::AtDepth,
        inject_depth: DEFAULT_INJECT_DEPTH,
    };

    /// What a declared field grants unless a Profile overrides it.
    ///
    /// Naming a field in the declaration is the statement that the model should
    /// know it, so its value is injected every turn. Visibility and writability
    /// are extra authority and are not implied: a scene can be tracked without
    /// letting every Agent pull on it or rewrite it. A Profile entry grants them
    /// back field by field, and the designated state updater is exactly that
    /// entry.
    pub const DECLARED: Self = Self {
        inject: true,
        visible: false,
        writable: false,
        inject_slot: StateInjectionSlot::AtDepth,
        inject_depth: DEFAULT_INJECT_DEPTH,
    };
}

fn default_inject_depth() -> usize {
    DEFAULT_INJECT_DEPTH
}

/// One configured entry: the key it covers plus the three switches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateAccessEntry {
    /// The key this entry answers to, written the way the declaration writes it.
    pub pattern: StateKeyPattern,
    #[serde(default)]
    pub inject: bool,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub writable: bool,
    #[serde(default)]
    pub inject_slot: StateInjectionSlot,
    #[serde(default = "default_inject_depth")]
    pub inject_depth: usize,
}

impl StateAccessEntry {
    fn access(&self) -> StateFieldAccess {
        StateFieldAccess {
            inject: self.inject,
            visible: self.visible,
            writable: self.writable,
            inject_slot: self.inject_slot,
            inject_depth: self.inject_depth,
        }
    }
}

/// What a lookup found for one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldAccess {
    /// No access configuration is bound, so nothing is constrained.
    Unconstrained,
    /// The configuration applies: exactly these switches.
    Configured(StateFieldAccess),
    /// More than one configured entry covers the key, so no single answer exists.
    ///
    /// Overlaps are refused when the configuration is saved; a policy loaded
    /// from an older file still fails loudly instead of picking one at runtime.
    Ambiguous { candidates: Vec<String> },
}

/// The per-Profile access configuration.
///
/// An empty configuration means "not configured yet" and constrains nothing; it
/// must not be mistaken for a Profile that granted nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateAccessPolicy {
    #[serde(default)]
    pub entries: Vec<StateAccessEntry>,
}

/// What the Profile's own entries said about one key.
enum PolicyVerdict<'a> {
    /// No entry covers the key. The Profile is silent, not refusing.
    NoMatch,
    Match(&'a StateAccessEntry),
    Ambiguous { candidates: Vec<String> },
}

impl StateAccessPolicy {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether any entry lets a model write.
    ///
    /// The Profile's own answer, without the declaration's defaults: use
    /// [`has_writable_access`] when the question is "can anything write at all".
    pub fn has_writable_fields(&self) -> bool {
        self.entries.iter().any(|entry| entry.writable)
    }

    /// What this Profile's own entries say about one key.
    ///
    /// Matching is anchored, exactly as the declaration's is: `角色/*` must not
    /// reach `角色/甲/着装`, or a permission would quietly widen to every child
    /// of a covered key. Case folding is only a fallback, so a configuration
    /// that deliberately keeps two case variants still resolves exactly.
    fn verdict(&self, key: &StateKey) -> PolicyVerdict<'_> {
        if self.is_empty() {
            return PolicyVerdict::NoMatch;
        }

        if let Some(entry) = self.single_match(key.as_str(), false) {
            return PolicyVerdict::Match(entry);
        }
        if let Some(entry) = self.single_match(key.as_str(), true) {
            return PolicyVerdict::Match(entry);
        }

        let candidates = self
            .entries
            .iter()
            .filter(|entry| entry.pattern.matches_tolerant(key.as_str()))
            .map(|entry| entry.pattern.as_str().to_string())
            .collect::<Vec<_>>();

        if candidates.len() > 1 {
            return PolicyVerdict::Ambiguous { candidates };
        }

        PolicyVerdict::NoMatch
    }

    /// Look up what this Profile's entries answer for one key, as an allowlist.
    ///
    /// Unlike [`resolve_access`], a key no entry covers is a refusal here rather
    /// than a question for the declaration — this is the Profile's own answer.
    pub fn lookup(&self, key: &StateKey) -> FieldAccess {
        match self.verdict(key) {
            PolicyVerdict::Match(entry) => FieldAccess::Configured(entry.access()),
            PolicyVerdict::Ambiguous { candidates } => FieldAccess::Ambiguous { candidates },
            PolicyVerdict::NoMatch if self.is_empty() => FieldAccess::Unconstrained,
            // Configured, and this key was not granted anything.
            PolicyVerdict::NoMatch => FieldAccess::Configured(StateFieldAccess::NONE),
        }
    }

    /// Configured entries that can cover the same key, for save-time validation.
    pub fn overlaps(&self) -> Vec<PatternOverlap> {
        let patterns = self
            .entries
            .iter()
            .map(|entry| entry.pattern.clone())
            .collect::<Vec<_>>();
        check_pattern_overlaps(&patterns)
    }

    fn single_match(&self, text: &str, tolerant: bool) -> Option<&StateAccessEntry> {
        let mut found = None;
        for entry in &self.entries {
            let hit = if tolerant {
                entry.pattern.matches_tolerant(text)
            } else {
                entry.pattern.matches_exact(text)
            };
            if hit {
                if found.is_some() {
                    return None;
                }
                found = Some(entry);
            }
        }
        found
    }
}

/// What one key grants, overrides first.
///
/// A Profile entry that matches is an exception in both directions, including a
/// switch left off. When the Profile is silent about a key — configured with
/// other rows, or not configured at all — the declared field's own default
/// answers; a Profile row is an exception, not the only place a decision can
/// live. Only a key no declared field owns falls back to the Profile's own
/// answer, which keeps a chat with no declaration behaving as it did before
/// field defaults existed.
pub fn resolve_access(
    policy: &StateAccessPolicy,
    declaration: &StateDeclaration,
    key: &StateKey,
) -> FieldAccess {
    match policy.verdict(key) {
        PolicyVerdict::Match(entry) => FieldAccess::Configured(entry.access()),
        PolicyVerdict::Ambiguous { candidates } => FieldAccess::Ambiguous { candidates },
        PolicyVerdict::NoMatch => match declaration.access_for(key) {
            Some(access) => FieldAccess::Configured(access),
            None if policy.is_empty() => FieldAccess::Unconstrained,
            None => FieldAccess::Configured(StateFieldAccess::NONE),
        },
    }
}

/// Whether anything at all can write, from the Profile's entries and the
/// declaration's own field defaults together.
///
/// The answer is about a chat, not a key: a key-level answer would need a key,
/// and the callers of this ask "should this floor have written something". A
/// declared field that grants a write counts, because a Profile row is an
/// exception and its silence no longer means "nothing".
pub fn has_writable_access(policy: &StateAccessPolicy, declaration: &StateDeclaration) -> bool {
    policy.has_writable_fields()
        || declaration
            .fields
            .iter()
            .any(|field| field.access.writable)
}

/// Check every key an update wants to change against the access it resolves to.
///
/// Removing a field is a write too, so removals are checked alongside updates.
/// Every refused key is reported in one batch, because a partial write would
/// leave the model unable to tell what took effect: the caller applies the
/// request only when the whole check passes.
pub fn check_writable(
    policy: &StateAccessPolicy,
    declaration: &StateDeclaration,
    request: &ResolvedStateUpdateRequest,
) -> Result<(), Vec<StateUpdateError>> {
    let mut errors = Vec::new();
    for key in request
        .fields
        .iter()
        .map(|(key, _)| key)
        .chain(request.remove.iter())
    {
        match resolve_access(policy, declaration, key) {
            FieldAccess::Unconstrained => {}
            FieldAccess::Configured(access) if access.writable => {}
            FieldAccess::Configured(_) => errors.push(StateUpdateError {
                key: Some(key.as_str().to_string()),
                code: "state.field_not_writable",
                message: format!(
                    "`{key}` is not writable for this Agent: neither this Profile's entries nor the declared field mark it writable"
                ),
            }),
            FieldAccess::Ambiguous { candidates } => errors.push(StateUpdateError {
                key: Some(key.as_str().to_string()),
                code: "state.access_ambiguous",
                message: format!(
                    "`{key}` is covered by more than one access entry: {}",
                    candidates.join(", ")
                ),
            }),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

impl fmt::Display for FieldAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unconstrained => formatter.write_str("unconstrained"),
            Self::Configured(access) => write!(
                formatter,
                "inject={}, visible={}, writable={}",
                access.inject, access.visible, access.writable
            ),
            Self::Ambiguous { .. } => formatter.write_str("ambiguous"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FieldAccess, StateAccessEntry, StateAccessPolicy, StateFieldAccess, check_writable,
        has_writable_access, resolve_access,
    };
    use crate::models::state::{
        DeclaredStateField, ResolvedStateUpdateRequest, StateDeclaration, StateKey, StateUpdateError,
        StateUpdateRequest, resolve_request,
    };
    use crate::models::state_injection::{DEFAULT_INJECT_DEPTH, StateInjectionSlot};
    use crate::models::state_key::StateKeyPattern;

    fn entry(pattern: &str, inject: bool, visible: bool, writable: bool) -> StateAccessEntry {
        StateAccessEntry {
            pattern: StateKeyPattern::parse(pattern)
                .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
            inject,
            visible,
            writable,
            inject_slot: StateInjectionSlot::AtDepth,
            inject_depth: DEFAULT_INJECT_DEPTH,
        }
    }

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    /// A chat with no declaration at all: the pre-field-default world.
    fn no_declaration() -> StateDeclaration {
        StateDeclaration::default()
    }

    /// A declaration whose fields all carry the documented default access.
    fn declaration(patterns: &[&str]) -> StateDeclaration {
        StateDeclaration {
            fields: patterns
                .iter()
                .map(|pattern| DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern).expect("test pattern"),
                    label: pattern.to_string(),
                    access: StateFieldAccess::DECLARED,
                    initial: Default::default(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn request(fields: &[&str], remove: &[&str]) -> ResolvedStateUpdateRequest {
        let raw = StateUpdateRequest {
            fields: fields.iter().map(|key| (key.to_string(), vec!["v".to_string()])).collect(),
            remove: remove.iter().map(|key| key.to_string()).collect(),
        };
        resolve_request(&Default::default(), &raw).expect("keys must resolve")
    }

    fn denial_keys(errors: &[StateUpdateError]) -> Vec<&str> {
        errors
            .iter()
            .map(|error| error.key.as_deref().unwrap_or_default())
            .collect()
    }

    #[test]
    fn a_declared_field_answers_when_the_profile_is_silent() {
        // The inversion: a Profile row is an exception now, so a field a Profile
        // never mentions keeps the default its own declaration carries — the
        // model is told the value every turn, and nothing more.
        let declaration = declaration(&["环境/日期"]);

        let access = match resolve_access(
            &StateAccessPolicy::default(),
            &declaration,
            &key("环境/日期"),
        ) {
            FieldAccess::Configured(access) => access,
            other => panic!("a declared field answers with its own default, got {other:?}"),
        };
        // Declaring a field is asking for the model to know it; the two switches
        // that hand out extra authority stay off.
        assert_eq!(
            access,
            StateFieldAccess {
                inject: true,
                visible: false,
                writable: false,
                ..StateFieldAccess::NONE
            }
        );
        let errors = check_writable(
            &StateAccessPolicy::default(),
            &declaration,
            &request(&["环境/日期"], &[]),
        )
        .expect_err("writability is extra authority, not something a declaration hands out");
        assert_eq!(errors[0].code, "state.field_not_writable");
    }

    #[test]
    fn a_declared_field_is_injected_until_a_row_says_otherwise() {
        let declaration = declaration(&["环境/日期", "环境/时间"]);
        let silent = StateAccessPolicy::default();
        assert_eq!(
            resolve_access(&silent, &declaration, &key("环境/时间")),
            FieldAccess::Configured(StateFieldAccess::DECLARED)
        );

        // An entry is an exception in both directions: this one takes injection
        // away from a field the declaration asked to be known.
        let mute = StateAccessPolicy {
            entries: vec![entry("环境/时间", false, false, false)],
        };
        assert_eq!(
            resolve_access(&mute, &declaration, &key("环境/时间")),
            FieldAccess::Configured(StateFieldAccess::NONE)
        );
    }

    #[test]
    fn writable_access_needs_a_row_because_the_default_grants_no_write() {
        let declaration = declaration(&["环境/日期"]);
        assert!(
            !has_writable_access(&StateAccessPolicy::default(), &declaration),
            "a chat that granted nobody a write has nothing to be late with"
        );

        let granted = StateAccessPolicy {
            entries: vec![entry("环境/日期", false, false, true)],
        };
        assert!(has_writable_access(&granted, &declaration));
    }

    #[test]
    fn a_profile_entry_overrides_the_declared_default() {
        let declaration = declaration(&["环境/日期"]);
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", false, false, false)],
        };

        assert_eq!(
            resolve_access(&policy, &declaration, &key("环境/日期")),
            FieldAccess::Configured(StateFieldAccess::NONE)
        );
        let errors = check_writable(&policy, &declaration, &request(&["环境/日期"], &[]))
            .expect_err("the Profile said no to a field the declaration allows");
        assert_eq!(errors[0].code, "state.field_not_writable");
    }

    #[test]
    fn a_key_no_declared_field_owns_keeps_the_profiles_own_answer() {
        // Nothing declared `环境/地点`, so the Profile's "granted nothing" stands:
        // a field default can only come from a field.
        let declaration = declaration(&["环境/日期"]);
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", false, false, true)],
        };

        assert_eq!(
            resolve_access(&policy, &declaration, &key("环境/地点")),
            FieldAccess::Configured(StateFieldAccess::NONE)
        );
        assert!(
            check_writable(&policy, &declaration, &request(&["环境/地点"], &[])).is_err(),
            "the policy is still an allowlist for the keys the declaration does not own"
        );
    }

    #[test]
    fn a_row_about_another_key_leaves_the_declared_default_alone() {
        // The Profile configured a row, and this key is not what it covers: a row
        // is an exception for what it names, not a switch that turns every
        // declared default off.
        let mut declaration = declaration(&["环境/日期", "环境/时间"]);
        declaration.fields[1].access = StateFieldAccess {
            writable: true,
            ..StateFieldAccess::DECLARED
        };
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", true, true, true)],
        };

        assert_eq!(
            resolve_access(&policy, &declaration, &key("环境/时间")),
            FieldAccess::Configured(declaration.fields[1].access)
        );
        assert!(
            check_writable(&policy, &declaration, &request(&["环境/时间"], &[])).is_ok(),
            "a declared write survives a Profile row that only speaks about another key"
        );
    }

    #[test]
    fn a_profile_that_configured_nothing_constrains_nothing() {
        let policy = StateAccessPolicy::default();

        assert_eq!(policy.lookup(&key("环境/日期")), FieldAccess::Unconstrained);
        assert!(
            check_writable(&policy, &no_declaration(), &request(&["环境/日期"], &[])).is_ok(),
            "an unconfigured Profile must not be mistaken for one that granted nothing"
        );
    }

    #[test]
    fn a_key_no_entry_covers_is_refused_not_allowed() {
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", false, false, true)],
        };

        assert_eq!(
            policy.lookup(&key("环境/地点")),
            FieldAccess::Configured(StateFieldAccess::NONE),
            "three switches default to off, so an uncovered key grants nothing"
        );
        let errors = check_writable(&policy, &no_declaration(), &request(&["环境/地点"], &[]))
            .expect_err("an ungranted key must be refused");
        assert_eq!(errors[0].code, "state.field_not_writable");
    }

    #[test]
    fn a_pattern_grants_its_own_level_and_not_the_children() {
        let policy = StateAccessPolicy {
            entries: vec![entry("角色/*/着装", false, false, true)],
        };

        assert!(
            check_writable(&policy, &no_declaration(), &request(&["角色/角色甲/着装"], &[])).is_ok(),
            "the wildcard is where the user chooses the entity"
        );
        assert!(
            check_writable(&policy, &no_declaration(), &request(&["角色/角色甲/着装/上装"], &[])).is_err(),
            "`*` covers one segment, so permissions must not widen to children"
        );
    }

    #[test]
    fn removals_are_checked_as_writes() {
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", false, false, false)],
        };

        let errors = check_writable(&policy, &no_declaration(), &request(&[], &["环境/日期"]))
            .expect_err("deleting a field is a write");
        assert_eq!(denial_keys(&errors), ["环境/日期"]);
    }

    #[test]
    fn every_refused_key_is_reported_in_one_batch() {
        let policy = StateAccessPolicy {
            entries: vec![
                entry("环境/日期", false, false, true),
                entry("环境/地点", false, false, false),
                entry("环境/时间", false, false, false),
            ],
        };

        let errors = check_writable(&policy, &no_declaration(), &request(&["环境/日期", "环境/地点"], &["环境/时间"]))
            .expect_err("two updates and one removal must all be reported");
        assert_eq!(
            denial_keys(&errors),
            ["环境/地点", "环境/时间"],
            "the caller must be able to fix the whole submission in one retry"
        );
    }

    #[test]
    fn overlapping_entries_are_reported_at_save_time() {
        let policy = StateAccessPolicy {
            entries: vec![
                entry("角色/*/着装", true, false, false),
                entry("角色/角色甲/着装", true, false, false),
            ],
        };

        assert_eq!(policy.overlaps().len(), 1);
    }

    #[test]
    fn a_key_covered_by_two_entries_is_ambiguous_not_picked() {
        // Overlaps are refused when the policy is saved, but a policy loaded from
        // an older file must still fail loudly rather than grant one of them.
        let policy = StateAccessPolicy {
            entries: vec![
                entry("角色/*/着装", true, false, false),
                entry("角色/角色甲/*", true, false, false),
            ],
        };

        let errors = check_writable(&policy, &no_declaration(), &request(&["角色/角色甲/着装"], &[]))
            .expect_err("an ambiguous grant must be refused");
        assert_eq!(errors[0].code, "state.access_ambiguous");
    }

    #[test]
    fn the_switches_are_independent() {
        let policy = StateAccessPolicy {
            entries: vec![entry("环境/日期", true, false, false)],
        };

        let FieldAccess::Configured(access) = policy.lookup(&key("环境/日期")) else {
            panic!("a covered key must report its switches");
        };
        assert!(access.inject, "injection does not imply writability");
        assert!(!access.writable, "nor the other way around");
        assert!(check_writable(&policy, &no_declaration(), &request(&["环境/日期"], &[])).is_err());
    }
}
