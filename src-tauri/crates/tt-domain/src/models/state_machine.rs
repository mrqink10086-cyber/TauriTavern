//! Data-driven state machine layer over the chat state document.
//!
//! This module is deliberately orthogonal to [`super::state`]: the state
//! document holds *values* (what is true), while a machine holds *positions*
//! (where the story stands) and the rules for moving between them. Values stay
//! model-written and mutable; positions move only through declared transitions.
//!
//! Nothing here knows a concrete state name, a concrete condition, or a
//! concrete effect:
//!
//! - **States** come from the spec. There is no cap on how many a machine can
//!   declare, and the domain never matches on an id.
//! - **Conditions** are `source` + `op` + operands, and `op` is resolved
//!   through a [`ComparatorRegistry`] — a map, not a `match` on an enum. The
//!   built-in comparators are generic string/number predicates; callers can
//!   [`register`](ComparatorRegistry::register) more without touching this
//!   module.
//! - **Actions** are `kind` + target + operands, resolved the same way.
//! - **Script hooks** are declared in the spec and run by the caller: the
//!   domain reports which transitions fired, and a second pass lets the caller
//!   deny any of them.
//!
//! The module owns only what must be deterministic and IO-free: spec
//! validation, condition evaluation, and transition resolution. Persistence,
//! script execution, and writing results back to the state document belong to
//! the application layer.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::script_spec::ScriptSpec;

/// Safety bound on how many times one `evaluate` call may fire another round of
/// transitions, so a cyclic spec fails loudly instead of spinning.
///
/// This bounds chained firing per call — a transition enabling the next one —
/// not the number of states or transitions a machine may declare. A machine
/// with hundreds of states is fine; a chain longer than this is rejected with
/// `state_machine.round_limit` rather than silently truncated.
pub const MAX_EVALUATION_ROUNDS: usize = 256;
/// Safety bound on one state or event id.
pub const MAX_ID_CHARS: usize = 128;

/// Where a condition reads its input from. Compared as an opaque string so new
/// sources can be added without a new enum variant here.
pub const SOURCE_FIELD: &str = "field";
pub const SOURCE_ACTIVE: &str = "active";

/// Built-in action kinds. Like the comparators, these are generic effects, not
/// business rules; anything domain-specific belongs in a script hook.
pub const ACTION_SET_FIELD: &str = "setField";
pub const ACTION_CLEAR_FIELD: &str = "clearField";
pub const ACTION_EMIT: &str = "emit";

/// One declared position. `terminal` is a display hint for the panel and for
/// scripts — the evaluator does not treat terminal states specially, because
/// "nothing may leave this state" is a business rule the user must express as
/// the absence of outgoing transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSpec {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub terminal: bool,
}

/// A condition: read `source`, apply `op`, compare against `value` / `values`.
///
/// `op` is looked up in a registry, so the set of predicates is open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionSpec {
    /// `field` reads the state document, `active` reads the machine's own
    /// positions.
    pub source: String,
    /// The state field key, when `source` is `field`.
    #[serde(default)]
    pub field: Option<String>,
    pub op: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub values: Vec<String>,
    /// Parts, when one comparison is not enough.
    ///
    /// A combination carries no comparison of its own: combining and comparing
    /// in the same node would leave one of them unread, which is a configuration
    /// error rather than a preference, so the save-time check refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose: Option<ConditionCompose>,
}

impl ConditionSpec {
    /// Whether this condition combines parts instead of comparing one field.
    pub fn is_combination(&self) -> bool {
        self.compose.is_some()
    }

    /// Whether any comparison field was filled in.
    fn carries_comparison(&self) -> bool {
        !self.source.trim().is_empty()
            || self.field.is_some()
            || !self.op.trim().is_empty()
            || self.value.is_some()
            || !self.values.is_empty()
    }
}

/// Several conditions combined into one.
///
/// Combining belongs in the vocabulary rather than in a script: "夜晚 且 下雨"
/// is configuration, and a user who can write it in the same place as every
/// other condition does not need to learn a second mechanism. The evaluator
/// stays total and deterministic — an empty `all` holds, an empty `any` does
/// not — while the save-time check refuses an empty combination, so a stored
/// document never contains one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConditionCompose {
    /// Every part must hold.
    All(Vec<ConditionSpec>),
    /// At least one part must hold.
    Any(Vec<ConditionSpec>),
    /// The part must not hold.
    Not(Box<ConditionSpec>),
}

/// One effect of a transition. `kind` is looked up in a known-kind set, so an
/// unrecognised effect is reported instead of silently ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionSpec {
    pub kind: String,
    /// The state field key for `setField` / `clearField`, the event name for
    /// `emit`.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub values: Vec<String>,
}

/// One rule for moving between positions.
///
/// `from` lists the positions that must all be active; `to` lists the positions
/// that become active. Either side may be empty: an empty `from` is an entry
/// rule that needs no position, an empty `to` only leaves positions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionSpec {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub conditions: Vec<ConditionSpec>,
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
    /// Higher wins when two transitions want the same position. Ties fall back
    /// to declaration order, so the result is always reproducible.
    #[serde(default)]
    pub priority: i32,
}

/// An optional JS hook. The engine, the module map, and the call itself belong
/// to the caller; the spec only names the script.
///
/// The shape is shared with every other inline script (see
/// [`ScriptSpec`]): a machine hook and a picture set's condition script are the
/// same kind of thing, so a document carries them the same way.
pub type HookSpec = ScriptSpec;

/// A machine definition. Everything behavioural lives here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMachineSpec {
    /// Positions active when a chat starts. May hold several, which is how
    /// parallel regions are expressed.
    #[serde(default)]
    pub initial: Vec<String>,
    #[serde(default)]
    pub states: Vec<StateSpec>,
    #[serde(default)]
    pub transitions: Vec<TransitionSpec>,
    #[serde(default)]
    pub hooks: Option<HookSpec>,
}

/// The machine's live positions, stored next to the state document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineState {
    #[serde(default)]
    pub active: BTreeSet<String>,
}

/// A request to move, as sent by a model or a script.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRequest {
    /// Positions to enter.
    #[serde(default)]
    pub to: Vec<String>,
    /// Positions that must be active for the move to be legal. Omitted means
    /// "any current position".
    #[serde(default)]
    pub from: Option<Vec<String>>,
}

/// One field write produced by an action. An empty `values` clears the field,
/// matching the state document's own three-state semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldWrite {
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedTransition {
    pub index: usize,
    pub id: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedTransition {
    pub index: usize,
    pub id: Option<String>,
    pub reason: &'static str,
}

/// What one script hook should be handed. The domain never runs scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookCall {
    pub index: usize,
    pub id: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
}

/// The deterministic outcome of one evaluation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MachineEvaluation {
    /// Positions after the pass.
    pub active: BTreeSet<String>,
    pub applied: Vec<AppliedTransition>,
    pub skipped: Vec<SkippedTransition>,
    /// Field writes the caller must apply through the state document's own
    /// validation. Never written by this module.
    pub writes: Vec<FieldWrite>,
    pub events: Vec<String>,
    pub hooks: Vec<HookCall>,
}

/// One problem, located as precisely as the spec allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachineError {
    /// What the problem is about: a state id, a field key, a transition.
    pub target: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl MachineError {
    fn new(target: Option<String>, code: &'static str, message: String) -> Self {
        Self {
            target,
            code,
            message,
        }
    }
}

/// What a condition is evaluated against.
#[derive(Debug, Clone, Copy)]
pub struct ConditionContext<'a> {
    /// State document fields, keyed by their canonical key.
    pub fields: &'a BTreeMap<String, Vec<String>>,
    /// Positions active before this evaluation round.
    pub active: &'a BTreeSet<String>,
}

type Comparator = fn(&ConditionContext, &ConditionSpec) -> bool;

/// Resolves `op` names to predicates.
///
/// Built-ins are generic comparisons. Registering a name replaces it, which is
/// the supported way to extend the vocabulary without editing this module.
#[derive(Debug, Clone)]
pub struct ComparatorRegistry {
    comparators: BTreeMap<String, Comparator>,
}

impl ComparatorRegistry {
    pub fn register(&mut self, name: impl Into<String>, comparator: Comparator) {
        self.comparators.insert(name.into(), comparator);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.comparators.contains_key(name)
    }

    /// Every known `op`, for error messages.
    pub fn names(&self) -> Vec<String> {
        self.comparators.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Comparator> {
        self.comparators.get(name).copied()
    }
}

impl Default for ComparatorRegistry {
    fn default() -> Self {
        let mut registry = Self {
            comparators: BTreeMap::new(),
        };
        registry.register("eq", |ctx, condition| {
            single(ctx, condition).is_some_and(|value| value == condition.value.as_deref().unwrap_or(""))
        });
        registry.register("ne", |ctx, condition| {
            single(ctx, condition).is_some_and(|value| value != condition.value.as_deref().unwrap_or(""))
        });
        registry.register("in", |ctx, condition| {
            values(ctx, condition).iter().any(|value| condition.values.contains(value))
        });
        registry.register("not_in", |ctx, condition| {
            !values(ctx, condition).iter().any(|value| condition.values.contains(value))
        });
        registry.register("contains", |ctx, condition| {
            let needle = condition.value.as_deref().unwrap_or("");
            values(ctx, condition).iter().any(|value| value.contains(needle))
        });
        registry.register("not_contains", |ctx, condition| {
            let needle = condition.value.as_deref().unwrap_or("");
            !values(ctx, condition).iter().any(|value| value.contains(needle))
        });
        registry.register("gt", |ctx, condition| number_is(ctx, condition, |a, b| a > b));
        registry.register("gte", |ctx, condition| number_is(ctx, condition, |a, b| a >= b));
        registry.register("lt", |ctx, condition| number_is(ctx, condition, |a, b| a < b));
        registry.register("lte", |ctx, condition| number_is(ctx, condition, |a, b| a <= b));
        registry.register("exists", |ctx, condition| {
            values(ctx, condition).iter().any(|value| !value.trim().is_empty())
        });
        registry.register("missing", |ctx, condition| {
            !values(ctx, condition).iter().any(|value| !value.trim().is_empty())
        });
        registry.register("matches", |ctx, condition| {
            let Some(pattern) = condition.value.as_deref() else {
                return false;
            };
            let Ok(regex) = regex::Regex::new(pattern) else {
                return false;
            };
            values(ctx, condition).iter().any(|value| regex.is_match(value))
        });
        registry.register("active", |ctx, condition| {
            ids(condition).iter().all(|id| ctx.active.contains(id))
        });
        registry.register("inactive", |ctx, condition| {
            !ids(condition).iter().any(|id| ctx.active.contains(id))
        });
        registry
    }
}

/// The result of evaluating one condition on its own.
///
/// An unregistered `op` is reported rather than folded into "does not hold": a
/// missing comparator means nothing was compared, which is a configuration
/// problem, not a state that happens not to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionOutcome {
    Holds,
    DoesNotHold,
    UnknownOp { op: String },
}

impl ConditionOutcome {
    pub fn holds(&self) -> bool {
        matches!(self, Self::Holds)
    }
}

/// Evaluate one condition outside a machine.
///
/// The machine evaluates conditions as part of a transition table; panels, image
/// selection and previews need the same vocabulary for isolated conditions.
/// Sharing this entry point is what keeps one condition language instead of two
/// that drift apart.
///
/// A combination is evaluated by its own rules, and an op nothing answers is
/// reported before any part is compared: whether the caller hears about an
/// unregistered op must not depend on which branch happened to be reached first.
/// Recursion is bounded by the document's own nesting, which the JSON parser
/// already limits.
pub fn evaluate_condition(
    condition: &ConditionSpec,
    context: &ConditionContext<'_>,
    comparators: &ComparatorRegistry,
) -> ConditionOutcome {
    if let Some(op) = first_unknown_op(condition, comparators) {
        return ConditionOutcome::UnknownOp { op };
    }
    evaluate_known(condition, context, comparators)
}

/// The first op in the tree that the registry does not answer.
fn first_unknown_op(condition: &ConditionSpec, comparators: &ComparatorRegistry) -> Option<String> {
    let Some(compose) = condition.compose.as_ref() else {
        return (!comparators.contains(&condition.op)).then(|| condition.op.clone());
    };
    match compose {
        ConditionCompose::All(parts) | ConditionCompose::Any(parts) => {
            parts.iter().find_map(|part| first_unknown_op(part, comparators))
        }
        ConditionCompose::Not(part) => first_unknown_op(part, comparators),
    }
}

/// Evaluate a condition whose ops are all registered.
fn evaluate_known(
    condition: &ConditionSpec,
    context: &ConditionContext<'_>,
    comparators: &ComparatorRegistry,
) -> ConditionOutcome {
    let Some(compose) = condition.compose.as_ref() else {
        return match comparators.get(&condition.op) {
            Some(comparator) if comparator(context, condition) => ConditionOutcome::Holds,
            Some(_) => ConditionOutcome::DoesNotHold,
            // Unreachable after `first_unknown_op`; kept total rather than
            // panicking, because a configuration problem must not take the read
            // down.
            None => ConditionOutcome::UnknownOp {
                op: condition.op.clone(),
            },
        };
    };

    match compose {
        ConditionCompose::All(parts) => {
            if parts
                .iter()
                .all(|part| evaluate_known(part, context, comparators).holds())
            {
                ConditionOutcome::Holds
            } else {
                ConditionOutcome::DoesNotHold
            }
        }
        ConditionCompose::Any(parts) => {
            if parts
                .iter()
                .any(|part| evaluate_known(part, context, comparators).holds())
            {
                ConditionOutcome::Holds
            } else {
                ConditionOutcome::DoesNotHold
            }
        }
        ConditionCompose::Not(part) => match evaluate_known(part, context, comparators) {
            ConditionOutcome::Holds => ConditionOutcome::DoesNotHold,
            _ => ConditionOutcome::Holds,
        },
    }
}

/// What is wrong with a condition's shape.
///
/// Shape rules are shared: the machine layer and the panel layer validate the
/// same trees and must agree on what "unusable" means, while each keeps its own
/// error prefix for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionError {
    /// A node that both compares something and combines parts.
    MixedCondition,
    /// A combination with nothing to combine.
    EmptyCombination { kind: &'static str },
    /// An op the registry does not answer.
    UnknownOp { op: String },
}

impl std::fmt::Display for ConditionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MixedCondition => write!(
                formatter,
                "a condition cannot both combine parts and compare one field; drop either the parts or the comparison"
            ),
            Self::EmptyCombination { kind } => write!(
                formatter,
                "`{kind}` combines nothing; list at least one condition or remove it"
            ),
            Self::UnknownOp { op } => {
                write!(formatter, "condition op `{op}` is not registered")
            }
        }
    }
}

/// Check a whole condition tree, reporting every problem at once.
pub fn condition_shape_errors(
    condition: &ConditionSpec,
    comparators: &ComparatorRegistry,
) -> Vec<ConditionError> {
    let mut errors = Vec::new();
    collect_condition_errors(condition, comparators, &mut errors);
    errors
}

fn collect_condition_errors(
    condition: &ConditionSpec,
    comparators: &ComparatorRegistry,
    errors: &mut Vec<ConditionError>,
) {
    let Some(compose) = condition.compose.as_ref() else {
        if !comparators.contains(&condition.op) {
            errors.push(ConditionError::UnknownOp {
                op: condition.op.clone(),
            });
        }
        return;
    };

    if condition.carries_comparison() {
        errors.push(ConditionError::MixedCondition);
    }
    match compose {
        ConditionCompose::All(parts) => collect_part_errors("all", parts, comparators, errors),
        ConditionCompose::Any(parts) => collect_part_errors("any", parts, comparators, errors),
        ConditionCompose::Not(part) => collect_condition_errors(part, comparators, errors),
    }
}

fn collect_part_errors(
    kind: &'static str,
    parts: &[ConditionSpec],
    comparators: &ComparatorRegistry,
    errors: &mut Vec<ConditionError>,
) {
    if parts.is_empty() {
        errors.push(ConditionError::EmptyCombination { kind });
        return;
    }
    for part in parts {
        collect_condition_errors(part, comparators, errors);
    }
}

fn values(ctx: &ConditionContext, condition: &ConditionSpec) -> Vec<String> {
    match condition.source.as_str() {
        SOURCE_FIELD => ctx
            .fields
            .get(condition.field.as_deref().unwrap_or(""))
            .cloned()
            .unwrap_or_default(),
        SOURCE_ACTIVE => ctx.active.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

fn single(ctx: &ConditionContext, condition: &ConditionSpec) -> Option<String> {
    values(ctx, condition).first().cloned()
}

fn ids(condition: &ConditionSpec) -> Vec<String> {
    if condition.values.is_empty() {
        return condition
            .value
            .as_deref()
            .map(|value| vec![value.to_string()])
            .unwrap_or_default();
    }
    condition.values.clone()
}

fn number_is(
    ctx: &ConditionContext,
    condition: &ConditionSpec,
    compare: fn(f64, f64) -> bool,
) -> bool {
    let Some(left) = single(ctx, condition).and_then(|value| value.trim().parse::<f64>().ok()) else {
        return false;
    };
    let Some(right) = condition
        .value
        .as_deref()
        .and_then(|value| value.trim().parse::<f64>().ok())
    else {
        return false;
    };
    compare(left, right)
}

/// Known action kinds, for validation messages.
pub fn known_action_kinds() -> [&'static str; 3] {
    [ACTION_SET_FIELD, ACTION_CLEAR_FIELD, ACTION_EMIT]
}

fn known_sources() -> [&'static str; 2] {
    [SOURCE_FIELD, SOURCE_ACTIVE]
}

/// Check one condition tree against the machine's rules.
///
/// Shape problems (an unregistered op, an empty or mixed combination) are
/// reported with the machine's own codes, so the UI keeps the prefix it already
/// knows; the rules that only make sense for a comparison — a known source, a
/// position op, a declared field key — apply to the leaves, wherever they sit
/// inside the combination.
fn check_condition(
    transition: usize,
    condition: &ConditionSpec,
    declared_keys: &BTreeSet<String>,
    comparators: &ComparatorRegistry,
    errors: &mut Vec<MachineError>,
) {
    for error in condition_shape_errors(condition, comparators) {
        let (code, message) = match error {
            ConditionError::MixedCondition => (
                "state_machine.condition_mixed",
                format!(
                    "transition #{transition} combines conditions and also compares one; drop either the parts or the comparison"
                ),
            ),
            ConditionError::EmptyCombination { kind } => (
                "state_machine.condition_empty_combination",
                format!(
                    "transition #{transition} declares an empty `{kind}` combination; list at least one condition or remove it"
                ),
            ),
            ConditionError::UnknownOp { op } => (
                "state_machine.condition_op_unknown",
                format!(
                    "transition #{transition} uses op `{op}`, which is not one of {}",
                    comparators.names().join(", ")
                ),
            ),
        };
        errors.push(MachineError::new(None, code, message));
    }

    visit_leaf_conditions(condition, &mut |leaf| {
        if !known_sources().contains(&leaf.source.as_str()) {
            errors.push(MachineError::new(
                None,
                "state_machine.condition_source_unknown",
                format!(
                    "transition #{transition} reads from `{}`, which is not one of {}",
                    leaf.source,
                    known_sources().join(", ")
                ),
            ));
            return;
        }
        if !comparators.contains(&leaf.op) {
            // Already reported by the shape check; the rules below would only
            // add a second, less precise complaint about the same leaf.
            return;
        }
        let is_position_op = matches!(leaf.op.as_str(), "active" | "inactive");
        if leaf.source == SOURCE_FIELD && is_position_op {
            errors.push(MachineError::new(
                None,
                "state_machine.condition_source_mismatch",
                format!(
                    "transition #{transition} uses op `{}`, which reads positions; set source to `active`",
                    leaf.op
                ),
            ));
        }
        if leaf.source == SOURCE_ACTIVE && !is_position_op {
            errors.push(MachineError::new(
                None,
                "state_machine.condition_source_mismatch",
                format!(
                    "transition #{transition} reads positions but uses op `{}`, which reads field values",
                    leaf.op
                ),
            ));
        }
        if leaf.source == SOURCE_FIELD {
            let key = leaf.field.as_deref().unwrap_or("");
            if key.trim().is_empty() {
                errors.push(MachineError::new(
                    None,
                    "state_machine.condition_field_missing",
                    format!("transition #{transition} reads a field but names no key"),
                ));
            } else if !declared_keys.is_empty() && !declared_keys.contains(key) {
                errors.push(MachineError::new(
                    Some(key.to_string()),
                    "state_machine.undeclared_field",
                    format!("transition #{transition} reads `{key}`, which the state declaration does not define"),
                ));
            }
        }
    });
}

/// Call `visit` for every comparison in the tree, in declaration order.
fn visit_leaf_conditions(condition: &ConditionSpec, visit: &mut impl FnMut(&ConditionSpec)) {
    match condition.compose.as_ref() {
        None => visit(condition),
        Some(ConditionCompose::All(parts)) | Some(ConditionCompose::Any(parts)) => {
            for part in parts {
                visit_leaf_conditions(part, visit);
            }
        }
        Some(ConditionCompose::Not(part)) => visit_leaf_conditions(part, visit),
    }
}

use super::state::{KeyResolution, StateDeclaration};

/// Every field a spec names that the declaration does not define.
///
/// Checked apart from [`validate_spec`] because a declaration may hold patterns
/// such as `角色/*/着装`, and a pattern cannot be enumerated into the set that
/// function compares against — the declaration itself is the only thing that can
/// answer whether it owns a key.
pub fn declaration_errors(
    spec: &StateMachineSpec,
    declaration: &StateDeclaration,
) -> Vec<MachineError> {
    if declaration.is_empty() {
        return Vec::new();
    }
    let mut errors = Vec::new();
    for (index, transition) in spec.transitions.iter().enumerate() {
        for condition in &transition.conditions {
            if condition.source != SOURCE_FIELD {
                continue;
            }
            let Some(key) = condition.field.as_deref() else {
                continue;
            };
            if let KeyResolution::Undeclared { .. } = declaration.resolve(key) {
                errors.push(MachineError {
                    target: Some(key.to_string()),
                    code: "state_machine.undeclared_field",
                    message: format!(
                        "transition #{index} reads `{key}`, which the state declaration does not define"
                    ),
                });
            }
        }
        for action in &transition.actions {
            if !matches!(action.kind.as_str(), ACTION_SET_FIELD | ACTION_CLEAR_FIELD) {
                continue;
            }
            let Some(key) = action.target.as_deref() else {
                continue;
            };
            if let KeyResolution::Undeclared { .. } = declaration.resolve(key) {
                errors.push(MachineError {
                    target: Some(key.to_string()),
                    code: "state_machine.undeclared_field",
                    message: format!(
                        "transition #{index} writes `{key}`, which the state declaration does not define"
                    ),
                });
            }
        }
    }
    errors
}

/// Validate a whole spec, reporting every problem at once.
///
/// `declared_keys` are the state fields the user's declaration allows; a
/// condition or action naming anything else is a configuration error, not
/// something to sort out at run time.
pub fn validate_spec(
    spec: &StateMachineSpec,
    declared_keys: &BTreeSet<String>,
    comparators: &ComparatorRegistry,
) -> Vec<MachineError> {
    let mut errors = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    for state in &spec.states {
        let id = state.id.trim();
        if id.is_empty() {
            errors.push(MachineError::new(
                None,
                "state_machine.state_id_empty",
                "a state id cannot be empty".to_string(),
            ));
            continue;
        }
        if id.chars().count() > MAX_ID_CHARS {
            errors.push(MachineError::new(
                Some(id.to_string()),
                "state_machine.state_id_too_long",
                format!("state id is longer than {MAX_ID_CHARS} characters"),
            ));
        }
        if !seen.insert(id) {
            errors.push(MachineError::new(
                Some(id.to_string()),
                "state_machine.state_id_duplicate",
                format!("state `{id}` is declared more than once"),
            ));
        }
    }

    let defined: BTreeSet<&str> = spec.states.iter().map(|state| state.id.as_str()).collect();
    let check_ids = |ids: &[String], transition: usize, side: &str, errors: &mut Vec<MachineError>| {
        for id in ids {
            if !defined.contains(id.as_str()) {
                errors.push(MachineError::new(
                    Some(id.clone()),
                    "state_machine.undefined_state",
                    format!(
                        "transition #{transition} {side} references `{id}`, which is not one of this machine's states"
                    ),
                ));
            }
        }
    };

    for (index, transition) in spec.transitions.iter().enumerate() {
        check_ids(&transition.from, index, "from", &mut errors);
        check_ids(&transition.to, index, "to", &mut errors);

        for condition in &transition.conditions {
            check_condition(index, condition, declared_keys, comparators, &mut errors);
        }

        for action in &transition.actions {
            if !known_action_kinds().contains(&action.kind.as_str()) {
                errors.push(MachineError::new(
                    None,
                    "state_machine.action_kind_unknown",
                    format!(
                        "transition #{index} uses action `{}`, which is not one of {}",
                        action.kind,
                        known_action_kinds().join(", ")
                    ),
                ));
                continue;
            }
            let target = action.target.as_deref().unwrap_or("");
            if target.trim().is_empty() {
                errors.push(MachineError::new(
                    None,
                    "state_machine.action_target_missing",
                    format!("transition #{index} has a `{}` action with no target", action.kind),
                ));
            } else if action.kind != ACTION_EMIT
                && !declared_keys.is_empty()
                && !declared_keys.contains(target)
            {
                errors.push(MachineError::new(
                    Some(target.to_string()),
                    "state_machine.undeclared_field",
                    format!(
                        "transition #{index} writes `{target}`, which the state declaration does not define"
                    ),
                ));
            }
        }
    }

    for id in &spec.initial {
        if !defined.contains(id.as_str()) {
            errors.push(MachineError::new(
                Some(id.clone()),
                "state_machine.undefined_state",
                format!("initial position `{id}` is not one of this machine's states"),
            ));
        }
    }
    if spec.initial.is_empty() && !spec.transitions.iter().any(|transition| transition.from.is_empty()) {
        errors.push(MachineError::new(
            None,
            "state_machine.no_entry",
            "the machine declares no initial position and no transition that can fire without one"
                .to_string(),
        ));
    }

    errors
}

/// Resolve moves a caller asked for into transition indices.
///
/// Unknown positions and illegal moves are reported for every request at once,
/// each with what the machine does allow, so a model can correct the whole
/// batch in one retry.
pub fn resolve_requests(
    spec: &StateMachineSpec,
    state: &MachineState,
    requests: &[TransitionRequest],
) -> Result<Vec<usize>, Vec<MachineError>> {
    let defined: BTreeSet<&str> = spec.states.iter().map(|state| state.id.as_str()).collect();
    let mut errors = Vec::new();
    let mut indices = Vec::new();

    for (position, request) in requests.iter().enumerate() {
        for id in request.to.iter().chain(request.from.iter().flatten()) {
            if !defined.contains(id.as_str()) {
                errors.push(MachineError::new(
                    Some(id.clone()),
                    "state_machine.undefined_state",
                    format!("request #{position} names `{id}`, which is not one of this machine's states"),
                ));
            }
        }
        if !errors.is_empty() {
            continue;
        }

        let wanted_from: Option<BTreeSet<&str>> = request
            .from
            .as_ref()
            .map(|ids| ids.iter().map(|id| id.as_str()).collect());
        let candidates: Vec<usize> = spec
            .transitions
            .iter()
            .enumerate()
            .filter(|(_, transition)| {
                let to_matches = !transition.to.is_empty()
                    && transition
                        .to
                        .iter()
                        .any(|id| request.to.contains(id));
                let from_matches = match &wanted_from {
                    Some(wanted) => transition
                        .from
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<BTreeSet<_>>()
                        .intersection(wanted)
                        .next()
                        .is_some()
                        || (transition.from.is_empty() && wanted.is_empty()),
                    None => transition
                        .from
                        .iter()
                        .all(|id| state.active.contains(id)),
                };
                to_matches && from_matches
            })
            .map(|(index, _)| index)
            .collect();

        if candidates.is_empty() {
            errors.push(MachineError::new(
                request.to.first().cloned(),
                "state_machine.illegal_transition",
                format!(
                    "no transition reaches {} from the current positions [{}]",
                    request.to.join(", "),
                    state.active.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
            continue;
        }
        indices.extend(candidates);
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

/// Run one deterministic evaluation pass.
///
/// Every round collects the transitions whose `from` positions are still active
/// and whose conditions hold, then applies them by priority. A transition whose
/// positions were taken by a higher-priority rule is reported as skipped
/// instead of being dropped silently, so the outcome is explainable.
///
/// `denied` carries the indices the caller's script hooks refused; they are
/// skipped with `denied_by_hook`.
pub fn evaluate(
    spec: &StateMachineSpec,
    state: &MachineState,
    fields: &BTreeMap<String, Vec<String>>,
    comparators: &ComparatorRegistry,
    denied: &BTreeSet<usize>,
    forced: &BTreeSet<usize>,
) -> Result<MachineEvaluation, MachineError> {
    let mut evaluation = MachineEvaluation {
        active: state.active.clone(),
        ..Default::default()
    };
    // A requested move is a one-off: it fires on the first round and then gets
    // out of the way. Later rounds are the spec's own consequences, so a
    // request can never keep re-firing and stall the evaluation.
    let mut pending_forced = forced.clone();

    for _ in 0..MAX_EVALUATION_ROUNDS {
        let mut fired = false;
        let order = transition_order(spec);
        let mut consumed: BTreeSet<String> = BTreeSet::new();
        // Eligibility is decided against the positions the round started with,
        // so two rules wanting the same position are both seen — and the one
        // that loses can be reported instead of disappearing.
        let round_active = evaluation.active.clone();
        let round_fields = fields;

        for index in order {
            let Some(transition) = spec.transitions.get(index) else {
                continue;
            };
            let is_forced = pending_forced.contains(&index);
            if !is_forced
                && !transition.from.iter().all(|id| round_active.contains(id))
            {
                continue;
            }
            let round_state = MachineEvaluation {
                active: round_active.clone(),
                ..Default::default()
            };
            if !is_forced
                && !conditions_hold(transition, &round_state, round_fields, comparators)
            {
                continue;
            }
            if !is_forced
                && transition
                    .from
                    .iter()
                    .any(|id| consumed.contains(id))
            {
                evaluation.skipped.push(SkippedTransition {
                    index,
                    id: transition.id.clone(),
                    reason: "position_taken",
                });
                continue;
            }
            if denied.contains(&index) {
                evaluation.skipped.push(SkippedTransition {
                    index,
                    id: transition.id.clone(),
                    reason: "denied_by_hook",
                });
                continue;
            }

            // A requested move moves the machine too: `is_forced` only lifts the
            // eligibility checks above. The position it left is claimed for this
            // round, so a rule that wanted it is reported as `position_taken`
            // instead of firing from a position that is already gone.
            for id in &transition.from {
                evaluation.active.remove(id);
                consumed.insert(id.clone());
            }
            for id in &transition.to {
                evaluation.active.insert(id.clone());
            }
            for action in &transition.actions {
                apply_action(action, &mut evaluation);
            }
            if spec.hooks.is_some() {
                evaluation.hooks.push(HookCall {
                    index,
                    id: transition.id.clone(),
                    from: transition.from.clone(),
                    to: transition.to.clone(),
                });
            }
            evaluation.applied.push(AppliedTransition {
                index,
                id: transition.id.clone(),
                from: transition.from.clone(),
                to: transition.to.clone(),
            });
            fired = true;
        }

        if !fired {
            return Ok(evaluation);
        }
        pending_forced.clear();
    }

    Err(MachineError::new(
        None,
        "state_machine.round_limit",
        format!(
            "evaluation did not settle within {MAX_EVALUATION_ROUNDS} rounds; check the spec for transitions that re-enable each other"
        ),
    ))
}

fn transition_order(spec: &StateMachineSpec) -> Vec<usize> {
    let mut order: Vec<usize> = (0..spec.transitions.len()).collect();
    order.sort_by(|left, right| {
        let by_priority = spec.transitions[*right]
            .priority
            .cmp(&spec.transitions[*left].priority);
        by_priority.then_with(|| left.cmp(right))
    });
    order
}

fn conditions_hold(
    transition: &TransitionSpec,
    evaluation: &MachineEvaluation,
    fields: &BTreeMap<String, Vec<String>>,
    comparators: &ComparatorRegistry,
) -> bool {
    let context = ConditionContext {
        fields,
        active: &evaluation.active,
    };
    transition
        .conditions
        .iter()
        .all(|condition| evaluate_condition(condition, &context, comparators).holds())
}

fn apply_action(action: &ActionSpec, evaluation: &mut MachineEvaluation) {
    let Some(target) = action.target.clone() else {
        return;
    };
    match action.kind.as_str() {
        ACTION_SET_FIELD => evaluation.writes.push(FieldWrite {
            key: target,
            values: action.values.clone(),
        }),
        ACTION_CLEAR_FIELD => evaluation.writes.push(FieldWrite {
            key: target,
            values: Vec::new(),
        }),
        ACTION_EMIT => evaluation.events.push(target),
        _ => {}
    }
}

/// Start positions for a chat with no stored machine state.
pub fn initial_state(spec: &StateMachineSpec) -> MachineState {
    MachineState {
        active: spec.initial.iter().cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(states: &[&str], transitions: Vec<TransitionSpec>, initial: &[&str]) -> StateMachineSpec {
        StateMachineSpec {
            initial: initial.iter().map(|id| id.to_string()).collect(),
            states: states
                .iter()
                .map(|id| StateSpec {
                    id: id.to_string(),
                    label: None,
                    terminal: false,
                })
                .collect(),
            transitions,
            hooks: None,
        }
    }

    fn transition(from: &[&str], to: &[&str]) -> TransitionSpec {
        TransitionSpec {
            id: None,
            from: from.iter().map(|id| id.to_string()).collect(),
            to: to.iter().map(|id| id.to_string()).collect(),
            conditions: Vec::new(),
            actions: Vec::new(),
            priority: 0,
        }
    }

    fn condition(source: &str, field: Option<&str>, op: &str, value: &str) -> ConditionSpec {
        ConditionSpec {
            source: source.to_string(),
            field: field.map(str::to_string),
            op: op.to_string(),
            value: Some(value.to_string()),
            values: Vec::new(),
            compose: None,
        }
    }

    fn fields(entries: &[(&str, &str)]) -> BTreeMap<String, Vec<String>> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), vec![value.to_string()]))
            .collect()
    }

    #[test]
    fn a_machine_can_declare_many_states_without_a_cap() {
        let ids: Vec<String> = (0..80).map(|index| format!("s{index}")).collect();
        let states: Vec<&str> = ids.iter().map(|id| id.as_str()).collect();
        let mut transitions = Vec::new();
        for pair in states.windows(2) {
            transitions.push(transition(&[pair[0]], &[pair[1]]));
        }
        let machine = spec(&states, transitions, &["s0"]);
        let declared: BTreeSet<String> = BTreeSet::new();

        assert!(
            validate_spec(&machine, &declared, &ComparatorRegistry::default()).is_empty(),
            "80 states and 79 transitions must validate"
        );

        let state = initial_state(&machine);
        let outcome = evaluate(
            &machine,
            &state,
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("chain of 79 transitions resolves within the round limit");
        assert!(outcome.active.contains("s79"));
        assert!(!outcome.active.contains("s0"));
    }

    #[test]
    fn a_requested_move_leaves_the_position_it_came_from() {
        // A requested move is a move, not an addition: a position left behind
        // would keep answering the conditions of every later round.
        let machine = spec(
            &["day", "night"],
            vec![transition(&["day"], &["night"])],
            &["day"],
        );
        let state = initial_state(&machine);
        let forced: BTreeSet<usize> = [0].into_iter().collect();

        let outcome = evaluate(
            &machine,
            &state,
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &forced,
        )
        .expect("a requested move resolves");

        assert!(outcome.active.contains("night"));
        assert!(!outcome.active.contains("day"));
    }

    #[test]
    fn a_requested_move_claims_its_position_for_the_round() {
        // The requested rule runs first, so the rule that also wanted `day` is
        // reported as having lost the position instead of firing from a position
        // the request already left.
        let machine = spec(
            &["day", "night", "dusk"],
            vec![
                transition(&["day"], &["night"]),
                transition(&["day"], &["dusk"]),
            ],
            &["day"],
        );
        let state = initial_state(&machine);
        let forced: BTreeSet<usize> = [0].into_iter().collect();

        let outcome = evaluate(
            &machine,
            &state,
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &forced,
        )
        .expect("a requested move resolves");

        assert!(outcome.active.contains("night"));
        assert!(!outcome.active.contains("dusk"));
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].reason, "position_taken");
    }

    #[test]
    fn conditions_are_matched_by_the_registry_not_by_name() {
        let machine = spec(
            &["calm", "angry"],
            vec![TransitionSpec {
                conditions: vec![condition("field", Some("mood"), "eq", "bad")],
                ..transition(&["calm"], &["angry"])
            }],
            &["calm"],
        );
        let state = initial_state(&machine);

        let quiet = evaluate(
            &machine,
            &state,
            &fields(&[("mood", "good")]),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert!(quiet.active.contains("calm"));
        assert!(quiet.applied.is_empty());

        let loud = evaluate(
            &machine,
            &state,
            &fields(&[("mood", "bad")]),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert!(loud.active.contains("angry"));
    }

    #[test]
    fn a_registered_comparator_extends_the_vocabulary() {
        let mut comparators = ComparatorRegistry::default();
        comparators.register("isLong", |ctx, condition| {
            values(ctx, condition)
                .iter()
                .any(|value| value.chars().count() > 3)
        });
        let machine = spec(
            &["short", "long"],
            vec![TransitionSpec {
                conditions: vec![condition("field", Some("note"), "isLong", "")],
                ..transition(&["short"], &["long"])
            }],
            &["short"],
        );
        let declared: BTreeSet<String> = ["note".to_string()].into_iter().collect();
        assert!(validate_spec(&machine, &declared, &comparators).is_empty());

        let outcome = evaluate(
            &machine,
            &initial_state(&machine),
            &fields(&[("note", "abcd")]),
            &comparators,
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert!(outcome.active.contains("long"));
    }

    #[test]
    fn validate_reports_every_problem_at_once() {
        let machine = spec(
            &["a", "a", "b"],
            vec![
                TransitionSpec {
                    conditions: vec![condition("field", Some("ghost"), "eq", "x")],
                    actions: vec![ActionSpec {
                        kind: "explode".to_string(),
                        target: Some("nowhere".to_string()),
                        values: Vec::new(),
                    }],
                    ..transition(&["a"], &["missing"])
                },
                TransitionSpec {
                    conditions: vec![
                        condition("field", Some("known"), "nope", "x"),
                        condition("sideways", None, "eq", "x"),
                    ],
                    ..transition(&["a"], &["b"])
                },
            ],
            &[],
        );
        let declared: BTreeSet<String> = ["known".to_string()].into_iter().collect();
        let errors = validate_spec(&machine, &declared, &ComparatorRegistry::default());
        let codes: Vec<&str> = errors.iter().map(|error| error.code).collect();

        assert!(codes.contains(&"state_machine.state_id_duplicate"));
        assert!(codes.contains(&"state_machine.undefined_state"));
        assert!(codes.contains(&"state_machine.undeclared_field"));
        assert!(codes.contains(&"state_machine.condition_op_unknown"));
        assert!(codes.contains(&"state_machine.condition_source_unknown"));
        assert!(codes.contains(&"state_machine.action_kind_unknown"));
        assert!(codes.contains(&"state_machine.no_entry"));
        assert!(
            errors
                .iter()
                .any(|error| error.code == "state_machine.action_kind_unknown"
                    && error.message.contains("setField")),
            "an unknown action must say what is available"
        );
    }

    #[test]
    fn conflicting_transitions_are_decided_by_priority_and_explained() {
        let machine = spec(
            &["start", "win", "lose"],
            vec![
                TransitionSpec {
                    priority: 5,
                    ..transition(&["start"], &["win"])
                },
                TransitionSpec {
                    priority: 1,
                    ..transition(&["start"], &["lose"])
                },
            ],
            &["start"],
        );
        let outcome = evaluate(
            &machine,
            &initial_state(&machine),
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert!(outcome.active.contains("win"));
        assert!(!outcome.active.contains("lose"));
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].reason, "position_taken");
    }

    #[test]
    fn denied_transitions_are_skipped_with_a_reason() {
        let machine = spec(&["a", "b"], vec![transition(&["a"], &["b"])], &["a"]);
        let denied: BTreeSet<usize> = [0].into_iter().collect();
        let outcome = evaluate(
            &machine,
            &initial_state(&machine),
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &denied,
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert!(outcome.active.contains("a"));
        assert_eq!(outcome.skipped[0].reason, "denied_by_hook");
    }

    #[test]
    fn actions_produce_field_writes_and_events() {
        let machine = spec(
            &["a", "b"],
            vec![TransitionSpec {
                actions: vec![
                    ActionSpec {
                        kind: ACTION_SET_FIELD.to_string(),
                        target: Some("flag".to_string()),
                        values: vec!["on".to_string()],
                    },
                    ActionSpec {
                        kind: ACTION_EMIT.to_string(),
                        target: Some("entered_b".to_string()),
                        values: Vec::new(),
                    },
                ],
                ..transition(&["a"], &["b"])
            }],
            &["a"],
        );
        let outcome = evaluate(
            &machine,
            &initial_state(&machine),
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect("evaluate");
        assert_eq!(outcome.writes.len(), 1);
        assert_eq!(outcome.writes[0].key, "flag");
        assert_eq!(outcome.writes[0].values, vec!["on".to_string()]);
        assert_eq!(outcome.events, vec!["entered_b".to_string()]);
    }

    #[test]
    fn a_model_request_rejects_unknown_and_illegal_moves() {
        let machine = spec(&["a", "b"], vec![transition(&["a"], &["b"])], &["a"]);
        let state = initial_state(&machine);

        let unknown = resolve_requests(
            &machine,
            &state,
            &[TransitionRequest {
                to: vec!["nowhere".to_string()],
                from: None,
            }],
        )
        .expect_err("an unknown state must be rejected");
        assert_eq!(unknown[0].code, "state_machine.undefined_state");

        let illegal = resolve_requests(
            &machine,
            &state,
            &[TransitionRequest {
                to: vec!["b".to_string()],
                from: Some(vec!["b".to_string()]),
            }],
        )
        .expect_err("a move that cannot fire must be rejected");
        assert_eq!(illegal[0].code, "state_machine.illegal_transition");

        let legal =
            resolve_requests(&machine, &state, &[TransitionRequest { to: vec!["b".to_string()], from: None }])
                .expect("a legal move resolves");
        assert_eq!(legal, vec![0]);
    }

    #[test]
    fn a_cyclic_spec_fails_loudly_instead_of_spinning() {
        let machine = spec(
            &["a", "b"],
            vec![transition(&["a"], &["b"]), transition(&["b"], &["a"])],
            &["a"],
        );
        let error = evaluate(
            &machine,
            &initial_state(&machine),
            &BTreeMap::new(),
            &ComparatorRegistry::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .expect_err("a cycle must not settle");
        assert_eq!(error.code, "state_machine.round_limit");
    }

    #[test]
    fn a_single_condition_evaluates_outside_a_machine() {
        let document = crate::models::state::StateDocument {
            fields: vec![
                crate::models::state::StateField {
                    key: crate::models::state::StateKey::parse("关系/好感").unwrap(),
                    values: vec!["42".to_string()],
                },
                crate::models::state::StateField {
                    key: crate::models::state::StateKey::parse("角色/艾拉/着装").unwrap(),
                    values: vec!["浅色外套".to_string()],
                },
            ],
        };
        let fields = document.condition_fields();
        let active = BTreeSet::new();
        let context = ConditionContext {
            fields: &fields,
            active: &active,
        };
        let comparators = ComparatorRegistry::default();

        let holds = ConditionSpec {
            source: SOURCE_FIELD.to_string(),
            field: Some("关系/好感".to_string()),
            op: "gte".to_string(),
            value: Some("30".to_string()),
            values: Vec::new(),
            compose: None,
        };
        assert_eq!(
            evaluate_condition(&holds, &context, &comparators),
            ConditionOutcome::Holds
        );

        let does_not_hold = ConditionSpec {
            value: Some("90".to_string()),
            ..holds.clone()
        };
        assert_eq!(
            evaluate_condition(&does_not_hold, &context, &comparators),
            ConditionOutcome::DoesNotHold
        );

        let unknown = ConditionSpec {
            op: "closer_than".to_string(),
            ..holds
        };
        assert_eq!(
            evaluate_condition(&unknown, &context, &comparators),
            ConditionOutcome::UnknownOp {
                op: "closer_than".to_string()
            },
            "an unregistered op means nothing was compared, which is not the same as a false condition"
        );
    }

    #[test]
    fn a_combination_needs_every_part_to_hold() {
        let fields = fields(&[("环境/时间", "夜晚"), ("环境/天气", "晴")]);
        let active = BTreeSet::new();
        let context = ConditionContext {
            fields: &fields,
            active: &active,
        };
        let comparators = ComparatorRegistry::default();
        let night = condition(SOURCE_FIELD, Some("环境/时间"), "eq", "夜晚");
        let rain = condition(SOURCE_FIELD, Some("环境/天气"), "eq", "下雨");

        let both = ConditionSpec {
            compose: Some(ConditionCompose::All(vec![night.clone(), rain.clone()])),
            ..Default::default()
        };
        assert_eq!(
            evaluate_condition(&both, &context, &comparators),
            ConditionOutcome::DoesNotHold,
            "one part that does not hold is enough to refuse the whole"
        );

        let either = ConditionSpec {
            compose: Some(ConditionCompose::Any(vec![night.clone(), rain.clone()])),
            ..Default::default()
        };
        assert_eq!(
            evaluate_condition(&either, &context, &comparators),
            ConditionOutcome::Holds,
            "`any` needs one part, not all of them"
        );

        let clear = ConditionSpec {
            compose: Some(ConditionCompose::Not(Box::new(rain))),
            ..Default::default()
        };
        assert_eq!(
            evaluate_condition(&clear, &context, &comparators),
            ConditionOutcome::Holds,
            "`not` answers with the opposite of its part"
        );
    }

    #[test]
    fn an_unregistered_op_inside_a_combination_is_reported_however_the_parts_land() {
        let fields = fields(&[("环境/时间", "夜晚")]);
        let active = BTreeSet::new();
        let context = ConditionContext {
            fields: &fields,
            active: &active,
        };
        let comparators = ComparatorRegistry::default();

        // The first part holds, so a short-circuiting `any` could answer
        // "holds" without ever reading the broken part. It must not: the
        // configuration is wrong whether or not a branch happened to succeed.
        let any = ConditionSpec {
            compose: Some(ConditionCompose::Any(vec![
                condition(SOURCE_FIELD, Some("环境/时间"), "eq", "夜晚"),
                condition(SOURCE_FIELD, Some("环境/时间"), "closer_than", "0"),
            ])),
            ..Default::default()
        };
        assert_eq!(
            evaluate_condition(&any, &context, &comparators),
            ConditionOutcome::UnknownOp {
                op: "closer_than".to_string()
            }
        );
    }

    #[test]
    fn a_condition_that_both_combines_and_compares_is_refused() {
        let comparators = ComparatorRegistry::default();
        let mixed = ConditionSpec {
            source: SOURCE_FIELD.to_string(),
            field: Some("环境/时间".to_string()),
            op: "eq".to_string(),
            value: Some("夜晚".to_string()),
            compose: Some(ConditionCompose::Not(Box::new(condition(
                SOURCE_FIELD,
                Some("环境/时间"),
                "eq",
                "夜晚",
            )))),
            ..Default::default()
        };

        let errors = condition_shape_errors(&mixed, &comparators);
        assert_eq!(errors, vec![ConditionError::MixedCondition]);

        let empty = ConditionSpec {
            compose: Some(ConditionCompose::All(Vec::new())),
            ..Default::default()
        };
        assert_eq!(
            condition_shape_errors(&empty, &comparators),
            vec![ConditionError::EmptyCombination { kind: "all" }]
        );
    }

    #[test]
    fn a_spec_reports_an_unregistered_op_nested_in_a_combination() {
        let comparators = ComparatorRegistry::default();
        let spec = StateMachineSpec {
            initial: vec!["start".to_string()],
            states: vec![StateSpec {
                id: "start".to_string(),
                label: None,
                terminal: false,
            }],
            transitions: vec![TransitionSpec {
                id: None,
                from: vec![],
                to: vec!["start".to_string()],
                conditions: vec![ConditionSpec {
                    compose: Some(ConditionCompose::All(vec![condition(
                        SOURCE_FIELD,
                        Some("环境/时间"),
                        "closer_than",
                        "0",
                    )])),
                    ..Default::default()
                }],
                actions: Vec::new(),
                priority: 0,
            }],
            hooks: None,
        };

        let errors = validate_spec(&spec, &BTreeSet::new(), &comparators);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error.code == "state_machine.condition_op_unknown")
                .count(),
            1,
            "the nested leaf is where the problem is, got {errors:?}"
        );
    }
}
