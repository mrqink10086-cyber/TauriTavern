//! Predicate entries: content that is injected only when the state says so.
//!
//! A world book entry answers "does the chat text mention this?". A predicate
//! entry answers a different question — "is the story in this state?" — and the
//! answer comes from the state document, evaluated by the same condition
//! vocabulary the machine and the panels use. One condition language, three
//! readers.
//!
//! Two things make this a layer rather than a list of entries:
//!
//! - **Groups are exclusive.** "Staying distant" and "acting close" cannot both
//!   hold, so a group selects exactly one entry — by priority, with declaration
//!   order breaking ties — instead of injecting both and hoping the model sorts
//!   it out. The model only ever sees the outcome.
//! - **Entries act on each other.** An entry may inhibit entries by label, or
//!   require one to be selected. That relation is decided here, deterministically,
//!   because a model cannot tell which rule should win.
//!
//! Nothing here is stored as content: an evaluation produces contents for the
//! caller to place, and the next evaluation produces them again from the current
//! state. There is no copy to go stale and none to delete.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::state::{KeyResolution, StateDeclaration, StateDocument};
use super::state_machine::{
    ComparatorRegistry, ConditionCompose, ConditionContext, ConditionOutcome, ConditionSpec,
    condition_shape_errors, evaluate_condition,
};

/// Upper bound on one entry's content, counted in characters.
pub const MAX_PREDICATE_CONTENT_CHARS: usize = 8_192;

/// Upper bound on how many entries one evaluation may inject.
///
/// The smallest budget that still means something: it bounds the block the
/// caller has to place, and it turns "too many entries" into a reported fact
/// instead of a quietly enormous prompt.
pub const MAX_SELECTED_PREDICATES: usize = 64;

/// When an entry is a candidate at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "condition", rename_all = "camelCase")]
pub enum PredicateSource {
    /// Always a candidate. A constant entry inside a group is that group's
    /// default branch; a constant entry outside one is always injected.
    #[default]
    Constant,
    /// A candidate when its condition holds against the state document.
    State(ConditionSpec),
}

/// What one entry does to other entries while it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateEffect {
    pub kind: PredicateEffectKind,
    /// The labels this effect reaches. Matching is "carries any of these", and
    /// the save-time check refuses an effect that reaches nothing.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PredicateEffectKind {
    /// While this entry holds, entries carrying one of these labels are not
    /// injected.
    Inhibit,
    /// This entry holds only while at least one entry carrying one of these
    /// labels is also selected.
    Require,
}

/// One entry.
///
/// The content is what the reader sees; everything else decides whether it is
/// seen at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateEntry {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    /// What the entry contributes when it is selected.
    pub content: String,
    /// The labels effects select this entry by. Entry tags are their own thing:
    /// they have nothing to do with a character's tags.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: PredicateSource,
    /// An extra premise. When it does not hold the entry is not a candidate at
    /// all — which is not the same as being inhibited, and the preview keeps the
    /// two apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<ConditionSpec>,
    #[serde(default)]
    pub effects: Vec<PredicateEffect>,
    /// Higher wins inside a group; ties keep declaration order.
    #[serde(default)]
    pub priority: i32,
}

/// Entries that cannot hold at the same time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateGroup {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub entries: Vec<PredicateEntry>,
}

/// One saved predicate document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePredicateSet {
    #[serde(default)]
    pub groups: Vec<PredicateGroup>,
    /// Entries that never compete. A standing instruction belongs here; anything
    /// whose condition should replace an alternative belongs in a group.
    #[serde(default)]
    pub constants: Vec<PredicateEntry>,
}

/// One entry that made it into the final set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPredicate {
    /// The group it won, when it competed in one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub entry_id: String,
    pub content: String,
}

/// Why an entry that was a candidate did not make it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPredicate {
    pub entry_id: String,
    /// `unavailable` / `inhibited` / `requirement_missing` / `group_lost` /
    /// `over_budget`.
    pub reason: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateEvaluation {
    pub selected: Vec<SelectedPredicate>,
    pub skipped: Vec<SkippedPredicate>,
}

/// One problem, located as precisely as the document allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateError {
    pub target: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl PredicateError {
    fn new(target: Option<&str>, code: &'static str, message: String) -> Self {
        Self {
            target: target.map(str::to_string),
            code,
            message,
        }
    }
}

/// An entry as the evaluator sees it: where it came from, and its own order.
struct Candidate<'a> {
    group_id: Option<&'a str>,
    order: usize,
    entry: &'a PredicateEntry,
}

/// Evaluate one predicate document against one state document.
///
/// The steps are the design's four, in order: collect candidates, apply effects
/// until the set stops shrinking, pick one entry per group, then order and cut
/// to budget. Every step is deterministic, and every entry that was a candidate
/// but did not make it is reported with its reason — a preview that could not
/// explain itself would be no better than guessing.
pub fn evaluate_predicates(
    set: &StatePredicateSet,
    document: &StateDocument,
    comparators: &ComparatorRegistry,
) -> Result<PredicateEvaluation, PredicateError> {
    let fields = document.condition_fields();
    let positions = BTreeSet::new();
    let context = ConditionContext {
        fields: &fields,
        active: &positions,
    };

    let candidates = collect_candidates(set);
    let mut skipped = Vec::new();

    // 1. Candidates: the source holds and the premise holds.
    let mut available: BTreeSet<usize> = BTreeSet::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let entry = candidate.entry;
        if !source_holds(entry, &context, comparators)? {
            skipped.push(SkippedPredicate {
                entry_id: entry.id.clone(),
                reason: "unavailable",
            });
            continue;
        }
        if let Some(premise) = entry.availability.as_ref() {
            match evaluate_condition(premise, &context, comparators) {
                ConditionOutcome::Holds => {}
                ConditionOutcome::DoesNotHold => {
                    skipped.push(SkippedPredicate {
                        entry_id: entry.id.clone(),
                        reason: "unavailable",
                    });
                    continue;
                }
                ConditionOutcome::UnknownOp { op } => {
                    return Err(unknown_op(&entry.id, &op));
                }
            }
        }
        available.insert(index);
    }

    // 2. Effects, applied until the set stops shrinking.
    //
    // Each round can only remove entries, so the loop runs at most as many times
    // as there are candidates. The bound guards against a future mistake; it is
    // not a limit the configuration has to stay under.
    let mut in_force = available.clone();
    let mut settled = false;
    for _ in 0..=candidates.len() {
        let next = apply_effects(&in_force, &candidates);
        if next == in_force {
            settled = true;
            break;
        }
        in_force = next;
    }
    if !settled {
        return Err(PredicateError::new(
            None,
            "state.predicate_effect_cycle",
            "the effect pass did not settle; that is a defect in the evaluator, not a state of the document"
                .to_string(),
        ));
    }

    for index in &available {
        if in_force.contains(index) {
            continue;
        }
        let reason = if meets_requirements(*index, &in_force, &candidates) {
            "inhibited"
        } else {
            "requirement_missing"
        };
        skipped.push(SkippedPredicate {
            entry_id: candidates[*index].entry.id.clone(),
            reason,
        });
    }

    // 3. One entry per group, by priority; ties keep declaration order.
    let mut winners: Vec<usize> = Vec::new();
    for group in &set.groups {
        let mut in_group: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(index, candidate)| {
                in_force.contains(index) && candidate.group_id == Some(group.id.as_str())
            })
            .map(|(index, _)| index)
            .collect();
        if in_group.is_empty() {
            continue;
        }
        in_group.sort_by(|left, right| compare_by_priority(&candidates, *left, *right));
        winners.push(in_group[0]);
        for loser in &in_group[1..] {
            skipped.push(SkippedPredicate {
                entry_id: candidates[*loser].entry.id.clone(),
                reason: "group_lost",
            });
        }
    }

    // Constants never compete: they are injected whenever they are in force.
    let constants: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| candidate.group_id.is_none() && in_force.contains(index))
        .map(|(index, _)| index)
        .collect();

    // 4. Order, then cut to budget.
    let mut selected_indices: Vec<usize> = winners.into_iter().chain(constants).collect();
    selected_indices.sort_by(|left, right| compare_by_priority(&candidates, *left, *right));

    let mut selected = Vec::new();
    for (position, index) in selected_indices.into_iter().enumerate() {
        let candidate = &candidates[index];
        if position >= MAX_SELECTED_PREDICATES {
            skipped.push(SkippedPredicate {
                entry_id: candidate.entry.id.clone(),
                reason: "over_budget",
            });
            continue;
        }
        selected.push(SelectedPredicate {
            group_id: candidate.group_id.map(str::to_string),
            entry_id: candidate.entry.id.clone(),
            content: candidate.entry.content.clone(),
        });
    }

    Ok(PredicateEvaluation { selected, skipped })
}

/// Check a whole document, reporting every problem at once.
///
/// The declaration is optional for the same reason it is everywhere else: a chat
/// that has not bound one still has well-shaped conditions checked.
pub fn validate_predicate_set(
    set: &StatePredicateSet,
    declaration: Option<&StateDeclaration>,
    comparators: &ComparatorRegistry,
) -> Vec<PredicateError> {
    let mut errors = Vec::new();
    let mut group_ids: BTreeSet<&str> = BTreeSet::new();
    let mut entry_ids: BTreeSet<&str> = BTreeSet::new();
    let mut declared_tags: BTreeSet<&str> = BTreeSet::new();

    for group in &set.groups {
        let id = group.id.trim();
        if id.is_empty() {
            errors.push(PredicateError::new(
                None,
                "state.predicate_group_id_required",
                "a group needs an id".to_string(),
            ));
        } else if !group_ids.insert(id) {
            errors.push(PredicateError::new(
                Some(id),
                "state.predicate_group_id_duplicate",
                format!("two groups share the id `{id}`"),
            ));
        }
    }

    for entry in entries_of(set) {
        let id = entry.id.trim();
        if id.is_empty() {
            errors.push(PredicateError::new(
                None,
                "state.predicate_entry_id_required",
                "an entry needs an id".to_string(),
            ));
        } else if !entry_ids.insert(id) {
            errors.push(PredicateError::new(
                Some(id),
                "state.predicate_entry_id_duplicate",
                format!("two entries share the id `{id}`"),
            ));
        }
        if entry.content.trim().is_empty() {
            errors.push(PredicateError::new(
                Some(id),
                "state.predicate_content_required",
                format!("entry `{id}` has no content to inject"),
            ));
        }
        if entry.content.chars().count() > MAX_PREDICATE_CONTENT_CHARS {
            errors.push(PredicateError::new(
                Some(id),
                "state.predicate_content_too_long",
                format!("entry `{id}` is longer than {MAX_PREDICATE_CONTENT_CHARS} characters"),
            ));
        }
        for tag in &entry.tags {
            // Trimmed on both sides: the lookup below trims, so an untrimmed
            // declaration would report a label as unknown to itself.
            let tag = tag.trim();
            if !tag.is_empty() {
                declared_tags.insert(tag);
            }
        }
        if let PredicateSource::State(condition) = &entry.source {
            errors.extend(condition_errors(
                id,
                "source",
                condition,
                declaration,
                comparators,
            ));
        }
        if let Some(premise) = entry.availability.as_ref() {
            errors.extend(condition_errors(
                id,
                "availability",
                premise,
                declaration,
                comparators,
            ));
        }
    }

    // An effect that reaches a label no entry carries can never do anything.
    // Reporting it is the difference between a typo and a rule the user believes
    // they wrote.
    for entry in entries_of(set) {
        let id = entry.id.trim();
        for effect in &entry.effects {
            let mut reaches_something = false;
            for tag in &effect.tags {
                let tag = tag.trim();
                if tag.is_empty() {
                    continue;
                }
                reaches_something = true;
                if !declared_tags.contains(tag) {
                    errors.push(PredicateError::new(
                        Some(id),
                        "state.predicate_effect_tag_unknown",
                        format!("entry `{id}` acts on the label `{tag}`, which no entry carries"),
                    ));
                }
            }
            if !reaches_something {
                errors.push(PredicateError::new(
                    Some(id),
                    "state.predicate_effect_tags_required",
                    format!("entry `{id}` has an effect that reaches no label"),
                ));
            }
        }
    }

    errors
}

/// Every entry the document holds, groups first, in declaration order.
fn entries_of(set: &StatePredicateSet) -> impl Iterator<Item = &PredicateEntry> {
    set.groups
        .iter()
        .flat_map(|group| group.entries.iter())
        .chain(set.constants.iter())
}

/// Whether an entry is a candidate, treating an unregistered `op` as an error.
fn source_holds(
    entry: &PredicateEntry,
    context: &ConditionContext<'_>,
    comparators: &ComparatorRegistry,
) -> Result<bool, PredicateError> {
    match &entry.source {
        PredicateSource::Constant => Ok(true),
        PredicateSource::State(condition) => {
            match evaluate_condition(condition, context, comparators) {
                ConditionOutcome::Holds => Ok(true),
                ConditionOutcome::DoesNotHold => Ok(false),
                ConditionOutcome::UnknownOp { op } => Err(unknown_op(&entry.id, &op)),
            }
        }
    }
}

/// Apply one round of effects: requirements first, then inhibitions.
///
/// Both tests run against the set the round started with, so "A inhibits B while
/// B inhibits A" resolves to neither, every time — the outcome must not depend
/// on iteration order.
fn apply_effects(in_force: &BTreeSet<usize>, candidates: &[Candidate<'_>]) -> BTreeSet<usize> {
    let mut inhibited_tags: BTreeSet<&str> = BTreeSet::new();
    for index in in_force {
        for effect in &candidates[*index].entry.effects {
            if effect.kind == PredicateEffectKind::Inhibit {
                inhibited_tags.extend(effect.tags.iter().map(String::as_str));
            }
        }
    }

    in_force
        .iter()
        .copied()
        .filter(|index| meets_requirements(*index, in_force, candidates))
        .filter(|index| {
            !candidates[*index]
                .entry
                .tags
                .iter()
                .any(|tag| inhibited_tags.contains(tag.as_str()))
        })
        .collect()
}

/// Whether the labels this entry requires are carried by the set it is judged in.
fn meets_requirements(index: usize, in_force: &BTreeSet<usize>, candidates: &[Candidate<'_>]) -> bool {
    candidates[index]
        .entry
        .effects
        .iter()
        .filter(|effect| effect.kind == PredicateEffectKind::Require)
        .all(|effect| {
            in_force.iter().any(|other| {
                other != &index
                    && candidates[*other]
                        .entry
                        .tags
                        .iter()
                        .any(|tag| effect.tags.contains(tag))
            })
        })
}

/// Higher priority first; ties keep declaration order.
fn compare_by_priority(candidates: &[Candidate<'_>], left: usize, right: usize) -> std::cmp::Ordering {
    candidates[right]
        .entry
        .priority
        .cmp(&candidates[left].entry.priority)
        .then(candidates[left].order.cmp(&candidates[right].order))
}

fn unknown_op(entry_id: &str, op: &str) -> PredicateError {
    PredicateError::new(
        Some(entry_id),
        "state.predicate_unknown_op",
        format!("entry `{entry_id}` compares with `{op}`, which nothing answers"),
    )
}

fn condition_errors(
    entry_id: &str,
    at: &str,
    condition: &ConditionSpec,
    declaration: Option<&StateDeclaration>,
    comparators: &ComparatorRegistry,
) -> Vec<PredicateError> {
    let mut errors = Vec::new();
    for error in condition_shape_errors(condition, comparators) {
        errors.push(PredicateError::new(
            Some(entry_id),
            "state.predicate_condition_invalid",
            format!("entry `{entry_id}` has an unusable {at} condition: {error:?}"),
        ));
    }

    // A predicate reads the state document and nothing else: a machine's own
    // positions are not part of what a predicate entry answers, so a condition
    // that looks at them would silently never hold.
    // A declaration that has not been bound checks nothing: its keys are
    // unknown rather than absent, and refusing on that basis would make every
    // set unsaveable before a chat ever binds one.
    let declared = declaration.filter(|declaration| !declaration.is_empty());
    for read in condition_reads(condition) {
        if read.source != "field" {
            errors.push(PredicateError::new(
                Some(entry_id),
                "state.predicate_condition_source_invalid",
                format!(
                    "entry `{entry_id}` {at} reads `{}`, but a predicate entry can only read state fields",
                    read.source
                ),
            ));
            continue;
        }
        let Some(field) = read.field else {
            continue;
        };
        let Some(declaration) = declared else {
            continue;
        };
        if let KeyResolution::Undeclared { .. } = declaration.resolve(&field) {
            errors.push(PredicateError::new(
                Some(entry_id),
                "state.predicate_undeclared_field",
                format!(
                    "entry `{entry_id}` reads `{field}`, which the state declaration does not define"
                ),
            ));
        }
    }

    errors
}

/// One leaf of a condition tree: what it reads and from where.
struct ConditionRead {
    source: String,
    field: Option<String>,
}

fn condition_reads(condition: &ConditionSpec) -> Vec<ConditionRead> {
    let mut reads = Vec::new();
    collect_condition_reads(condition, &mut reads);
    reads
}

fn collect_condition_reads(condition: &ConditionSpec, reads: &mut Vec<ConditionRead>) {
    match condition.compose.as_ref() {
        None => reads.push(ConditionRead {
            source: condition.source.trim().to_string(),
            field: condition.field.clone(),
        }),
        Some(ConditionCompose::All(parts)) | Some(ConditionCompose::Any(parts)) => {
            for part in parts {
                collect_condition_reads(part, reads);
            }
        }
        Some(ConditionCompose::Not(part)) => collect_condition_reads(part, reads),
    }
}

/// Every entry the set holds, in the order the document declares them.
fn collect_candidates(set: &StatePredicateSet) -> Vec<Candidate<'_>> {
    let mut candidates = Vec::new();
    for group in &set.groups {
        for entry in &group.entries {
            candidates.push(Candidate {
                group_id: Some(group.id.as_str()),
                order: candidates.len(),
                entry,
            });
        }
    }
    for entry in &set.constants {
        candidates.push(Candidate {
            group_id: None,
            order: candidates.len(),
            entry,
        });
    }
    candidates
}

#[cfg(test)]
mod tests {
    use crate::models::state_access::StateFieldAccess;
    use super::*;
    use crate::models::state::{DeclaredStateField, StateField, StateKey};
    use crate::models::state_key::StateKeyPattern;

    fn condition(field: &str, op: &str, value: &str) -> ConditionSpec {
        ConditionSpec {
            source: "field".to_string(),
            field: Some(field.to_string()),
            op: op.to_string(),
            value: Some(value.to_string()),
            values: Vec::new(),
            compose: None,
        }
    }

    fn entry(id: &str) -> PredicateEntry {
        PredicateEntry {
            id: id.to_string(),
            content: format!("content of {id}"),
            ..Default::default()
        }
    }

    fn tagged(id: &str, tags: &[&str]) -> PredicateEntry {
        PredicateEntry {
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..entry(id)
        }
    }

    fn inhibiting(id: &str, tags: &[&str], inhibits: &str) -> PredicateEntry {
        PredicateEntry {
            effects: vec![PredicateEffect {
                kind: PredicateEffectKind::Inhibit,
                tags: vec![inhibits.to_string()],
            }],
            ..tagged(id, tags)
        }
    }

    fn document(pairs: &[(&str, &str)]) -> StateDocument {
        StateDocument {
            fields: pairs
                .iter()
                .map(|(key, value)| StateField {
                    key: StateKey::parse(key).expect("test key must be valid"),
                    values: vec![value.to_string()],
                })
                .collect(),
        }
    }

    fn registry() -> ComparatorRegistry {
        ComparatorRegistry::default()
    }

    fn ids(evaluation: &PredicateEvaluation) -> Vec<&str> {
        evaluation
            .selected
            .iter()
            .map(|selected| selected.entry_id.as_str())
            .collect()
    }

    #[test]
    fn a_group_selects_one_entry_by_priority() {
        let mut high = entry("close");
        high.priority = 10;
        let set = StatePredicateSet {
            groups: vec![PredicateGroup {
                id: "tone".to_string(),
                label: None,
                entries: vec![entry("distant"), high],
            }],
            constants: Vec::new(),
        };

        let outcome =
            evaluate_predicates(&set, &StateDocument::default(), &registry()).expect("evaluate");

        assert_eq!(ids(&outcome), vec!["close"]);
        assert_eq!(
            outcome.skipped,
            vec![SkippedPredicate {
                entry_id: "distant".to_string(),
                reason: "group_lost",
            }],
            "the losing entry is reported, not dropped"
        );
    }

    #[test]
    fn a_constant_entry_never_competes() {
        let set = StatePredicateSet {
            groups: vec![PredicateGroup {
                id: "tone".to_string(),
                label: None,
                entries: vec![entry("tone-a"), entry("tone-b")],
            }],
            constants: vec![entry("standing")],
        };

        let outcome =
            evaluate_predicates(&set, &StateDocument::default(), &registry()).expect("evaluate");

        // Same priority, so declaration order decides: the group comes first in
        // the document, and so does its winner.
        assert_eq!(ids(&outcome), vec!["tone-a", "standing"]);
        assert!(
            outcome
                .selected
                .iter()
                .any(|selected| selected.entry_id == "standing" && selected.group_id.is_none()),
            "a standing entry keeps no group and is never outvoted"
        );
    }

    #[test]
    fn a_condition_that_does_not_hold_leaves_the_entry_unavailable() {
        let mut night = entry("night");
        night.source = PredicateSource::State(condition("环境/时间", "eq", "夜"));
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![night],
        };

        let day = evaluate_predicates(&set, &document(&[("环境/时间", "昼")]), &registry())
            .expect("evaluate");
        assert!(day.selected.is_empty());
        assert_eq!(day.skipped[0].reason, "unavailable");

        let dark = evaluate_predicates(&set, &document(&[("环境/时间", "夜")]), &registry())
            .expect("evaluate");
        assert_eq!(ids(&dark), vec!["night"]);
    }

    #[test]
    fn an_entry_inhibits_the_labels_it_reaches() {
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![
                tagged("distant", &["distance"]),
                tagged("close", &["closeness"]),
                inhibiting("guard", &["guard"], "distance"),
            ],
        };

        let outcome =
            evaluate_predicates(&set, &StateDocument::default(), &registry()).expect("evaluate");

        assert_eq!(ids(&outcome), vec!["close", "guard"]);
        assert_eq!(
            outcome.skipped,
            vec![SkippedPredicate {
                entry_id: "distant".to_string(),
                reason: "inhibited",
            }]
        );
    }

    #[test]
    fn a_requirement_that_is_not_met_is_reported_separately() {
        let mut needy = tagged("needy", &["solo"]);
        needy.effects = vec![PredicateEffect {
            kind: PredicateEffectKind::Require,
            tags: vec!["ally".to_string()],
        }];

        let alone = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![needy.clone()],
        };
        let outcome =
            evaluate_predicates(&alone, &StateDocument::default(), &registry()).expect("evaluate");
        assert!(outcome.selected.is_empty());
        assert_eq!(
            outcome.skipped[0].reason,
            "requirement_missing",
            "an unmet requirement is not the same as being inhibited"
        );

        let accompanied = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![needy, tagged("ally", &["ally"])],
        };
        let outcome = evaluate_predicates(&accompanied, &StateDocument::default(), &registry())
            .expect("evaluate");
        assert_eq!(ids(&outcome), vec!["needy", "ally"]);
    }

    #[test]
    fn two_entries_that_inhibit_each_other_both_lose() {
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![
                inhibiting("left", &["l"], "r"),
                inhibiting("right", &["r"], "l"),
            ],
        };

        let outcome =
            evaluate_predicates(&set, &StateDocument::default(), &registry()).expect("evaluate");

        assert!(
            outcome.selected.is_empty(),
            "neither can hold while the other does, and the answer must not depend on order"
        );
        assert_eq!(outcome.skipped.len(), 2);
        assert!(
            outcome
                .skipped
                .iter()
                .all(|skipped| skipped.reason == "inhibited")
        );
    }

    #[test]
    fn an_unregistered_comparison_stops_the_evaluation() {
        let mut weird = entry("weird");
        weird.source = PredicateSource::State(condition("环境/时间", "isBlue", "yes"));
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![weird],
        };

        let error = evaluate_predicates(&set, &StateDocument::default(), &registry())
            .expect_err("an op nothing answers is a configuration problem, not a false condition");

        assert_eq!(error.code, "state.predicate_unknown_op");
    }

    #[test]
    fn the_budget_reports_what_it_left_out() {
        let constants = (0..MAX_SELECTED_PREDICATES + 2)
            .map(|index| entry(&format!("entry-{index}")))
            .collect();
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants,
        };

        let outcome =
            evaluate_predicates(&set, &StateDocument::default(), &registry()).expect("evaluate");

        assert_eq!(outcome.selected.len(), MAX_SELECTED_PREDICATES);
        assert_eq!(
            outcome
                .skipped
                .iter()
                .filter(|skipped| skipped.reason == "over_budget")
                .count(),
            2
        );
    }

    #[test]
    fn validation_reports_every_problem_at_once() {
        let mut nameless = entry("");
        nameless.content = String::new();
        let mut active_reader = entry("active-reader");
        active_reader.source = PredicateSource::State(ConditionSpec {
            source: "active".to_string(),
            field: None,
            op: "active".to_string(),
            value: None,
            values: vec!["day".to_string()],
            compose: None,
        });

        let set = StatePredicateSet {
            groups: vec![
                PredicateGroup {
                    id: "tone".to_string(),
                    label: None,
                    entries: vec![nameless],
                },
                PredicateGroup {
                    id: "tone".to_string(),
                    label: None,
                    entries: vec![inhibiting("guard", &["guard"], "ghost"), active_reader],
                },
            ],
            constants: Vec::new(),
        };

        let codes: Vec<&str> = validate_predicate_set(&set, None, &registry())
            .iter()
            .map(|error| error.code)
            .collect();

        for expected in [
            "state.predicate_group_id_duplicate",
            "state.predicate_entry_id_required",
            "state.predicate_content_required",
            "state.predicate_effect_tag_unknown",
            "state.predicate_condition_source_invalid",
        ] {
            assert!(codes.contains(&expected), "expected {expected} in {codes:?}");
        }
    }

    #[test]
    fn an_undeclared_field_is_refused_against_a_bound_declaration() {
        let mut reader = entry("reader");
        reader.source = PredicateSource::State(condition("幽灵/字段", "eq", "x"));
        let set = StatePredicateSet {
            groups: Vec::new(),
            constants: vec![reader],
        };
        let declaration = StateDeclaration {
            fields: vec![DeclaredStateField {
                pattern: StateKeyPattern::parse("环境/日期").expect("pattern must parse"),
                label: "DATE".to_string(),
                access: StateFieldAccess::DECLARED,
                initial: Vec::new(),
            }],
            panels: Default::default(),
            machine: None,
            predicates: None,
            limits: Default::default(),
        };

        let errors = validate_predicate_set(&set, Some(&declaration), &registry());

        assert!(
            errors
                .iter()
                .any(|error| error.code == "state.predicate_undeclared_field")
        );
    }
}
