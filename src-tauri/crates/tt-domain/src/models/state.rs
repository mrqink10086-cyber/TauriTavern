//! Chat state document: fields, declarations, and update semantics.
//!
//! State fields are declared by the user and filled in by the model. This module
//! owns the parts that must be deterministic and testable without IO, split in
//! two steps: [`resolve_request`] turns what a caller sent into a validated,
//! canonically keyed change set, and [`apply_update`] applies that change set to
//! a document. Keeping the steps apart is what lets a rejection report every
//! problem at once — the model fixes the whole submission in a single retry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::DomainError;

pub use super::state_key::StateKey;
use super::state_access::StateFieldAccess;
use super::state_key::{PatternOverlap, StateKeyPattern, check_pattern_overlaps, normalize_key};

/// Upper bound on the fields one update may carry, when a scene says nothing.
pub const MAX_UPDATE_FIELDS: usize = 128;
/// Upper bound on the values one field may carry, when a scene says nothing.
pub const MAX_FIELD_VALUES: usize = 32;
/// Upper bound on one value line, when a scene says nothing.
///
/// Counted in characters by default. A character is a poor unit for the thing
/// this number is really about — the same 512 is roughly 105 tokens of English
/// and 320 to 640 of Chinese — which is why the unit and the number are the
/// scene's to choose. They are only the defaults here.
pub const MAX_VALUE_CHARS: usize = 512;

/// What one scene lets a value cost, and what that cost is counted in.
///
/// `0` on any ceiling means the scene does not limit it: the shape of a key is
/// still enforced, and storage still has to be able to hold what it is given,
/// but the number is no longer the scene's business.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateLimits {
    /// The cost one value may reach.
    #[serde(default = "default_value_ceiling")]
    pub value: usize,
    /// How many values one field may hold.
    #[serde(default = "default_values_per_field")]
    pub values_per_field: usize,
    /// How many fields one update may carry.
    #[serde(default = "default_fields_per_update")]
    pub fields_per_update: usize,
    /// What `value` is counted in.
    #[serde(default)]
    pub unit: CostUnit,
    /// Which vocabulary counts a token, when the unit is tokens.
    ///
    /// Empty means "the model the chat runs", which is the answer that needs no
    /// configuration and the reason this is a choice at all: the budget follows
    /// whatever the chat is talking to. A shipped family's name (`glm`,
    /// `qwen3.8`, `gemma4` …) pins it instead, and `file:<path>` counts with a
    /// vocabulary the user supplied. It is a name rather than a reference,
    /// because the domain does not load vocabularies.
    #[serde(default)]
    pub tokenizer: String,
}

/// What a value's cost is counted in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CostUnit {
    /// One character, one unit.
    ///
    /// Deterministic, free, and identical on every machine — which is what a
    /// validation result has to be. What it cannot be is fair across languages.
    #[default]
    Chars,
    /// Tokens of a chosen vocabulary.
    ///
    /// Closer to what the budget is actually about, at the price of depending on
    /// a vocabulary: the same text can measure differently under another one, so
    /// the scene has to name which it counts in.
    Tokens,
}

impl CostUnit {
    /// What a rejection calls this unit.
    pub fn label(self) -> &'static str {
        match self {
            Self::Chars => "characters",
            Self::Tokens => "tokens",
        }
    }
}

fn default_value_ceiling() -> usize {
    MAX_VALUE_CHARS
}

fn default_values_per_field() -> usize {
    MAX_FIELD_VALUES
}

fn default_fields_per_update() -> usize {
    MAX_UPDATE_FIELDS
}

impl Default for StateLimits {
    fn default() -> Self {
        Self {
            value: MAX_VALUE_CHARS,
            values_per_field: MAX_FIELD_VALUES,
            fields_per_update: MAX_UPDATE_FIELDS,
            unit: CostUnit::Chars,
            tokenizer: String::new(),
        }
    }
}

impl StateLimits {
    /// Ceilings a value, a field and an update are no longer measured against.
    pub const UNBOUNDED: Self = Self {
        value: 0,
        values_per_field: 0,
        fields_per_update: 0,
        unit: CostUnit::Chars,
        tokenizer: String::new(),
    };

    /// The value ceiling, when the scene has one.
    pub fn value_ceiling(&self) -> Option<usize> {
        (self.value > 0).then_some(self.value)
    }

    /// The values-per-field ceiling, when the scene has one.
    pub fn values_ceiling(&self) -> Option<usize> {
        (self.values_per_field > 0).then_some(self.values_per_field)
    }

    /// The fields-per-update ceiling, when the scene has one.
    pub fn fields_ceiling(&self) -> Option<usize> {
        (self.fields_per_update > 0).then_some(self.fields_per_update)
    }
}

/// Count a value the way a scene that says nothing counts it.
pub fn count_chars(value: &str) -> usize {
    value.chars().count()
}

/// One field of the state document. A single-valued field carries one line; an
/// empty list is a field whose value has been cleared but which still exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateField {
    pub key: StateKey,
    #[serde(default)]
    pub values: Vec<String>,
}

/// The state document bound to one floor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDocument {
    #[serde(default)]
    pub fields: Vec<StateField>,
}

impl StateDocument {
    pub fn get(&self, key: &StateKey) -> Option<&StateField> {
        self.fields.iter().find(|field| &field.key == key)
    }

    /// The document as the field map conditions compare against.
    ///
    /// Conditions read `key -> values`; the document stores the same thing as a
    /// list of fields. One adapter, so the panel's image selection, the state
    /// machine and the predicate preview all see the identical shape.
    pub fn condition_fields(&self) -> BTreeMap<String, Vec<String>> {
        self.fields
            .iter()
            .map(|field| (field.key.as_str().to_string(), field.values.clone()))
            .collect()
    }

    /// The document a chat starts from: every declared field that carries an
    /// initial value, in declaration order.
    ///
    /// Used only where no document exists yet. A stored document — even one with
    /// no fields left — is the state the chat actually reached, and seeding over
    /// it would resurrect values somebody removed.
    ///
    /// A literal key that fails to parse is skipped rather than reported: the
    /// declaration's own validation refuses such a key when it is saved, so this
    /// is only reachable from a declaration that was never validated.
    pub fn seeded(declaration: &StateDeclaration) -> Self {
        let fields = declaration
            .fields
            .iter()
            .filter(|field| field.pattern.is_literal() && !field.initial.is_empty())
            .filter_map(|field| {
                StateKey::parse(field.pattern.as_str())
                    .ok()
                    .map(|key| StateField {
                        key,
                        values: field.initial.clone(),
                    })
            })
            .collect();
        Self { fields }
    }

    /// Validate a document loaded from storage. Stored keys bypass `StateKey::parse`,
    /// so they must be re-checked before use.
    pub fn validate(&self) -> Result<(), String> {
        for field in &self.fields {
            StateKey::parse(field.key.as_str())?;
        }
        Ok(())
    }
}

/// One declared field. The declaration is what makes per-field access configurable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredStateField {
    /// The key this field answers to: a literal name, or a pattern that covers
    /// many names such as `环境/*/来源`.
    pub pattern: StateKeyPattern,
    /// Display name used by the panel; independent of the key path.
    pub label: String,
    /// What this field grants unless a Profile overrides it.
    ///
    /// The default lives here rather than only in each Profile so that a field
    /// carries its own answer to "is this pushed into the prompt, may the model
    /// change it". A Profile then records exceptions, which is the reading a
    /// reader expects from a field list: what is not written down is inherited,
    /// not refused.
    #[serde(default = "declared_field_access")]
    pub access: StateFieldAccess,
    /// What this field holds before anything writes it.
    ///
    /// A chat has no state document until something writes one, and a scene
    /// whose fields start empty makes the model write every one of them before
    /// the story can begin. Values here seed the document instead: they are the
    /// state a chat starts with, not a write, so they neither need an Agent's
    /// permission nor count against one.
    ///
    /// Only a literal key can carry them. A pattern covers keys nobody has named
    /// yet, and seeding one would mean inventing the key — the one thing a
    /// declaration exists to prevent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub initial: Vec<String>,
}

fn declared_field_access() -> StateFieldAccess {
    StateFieldAccess::DECLARED
}

/// User-declared field list.
///
/// This is where the key space is defined: the user names the segments, decides
/// how deep keys go, picks the patterns, and chooses the labels the panel shows.
/// The domain never enumerates any of it.
///
/// An empty declaration means "not configured yet": only the key shape is
/// checked. A non-empty declaration additionally rejects undeclared keys, which
/// is what stops the model from inventing state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDeclaration {
    #[serde(default)]
    pub fields: Vec<DeclaredStateField>,
    /// How the declaration is shown: panels, rails, and the pictures each
    /// element may show.
    ///
    /// Display settings live with the declaration rather than in a file of
    /// their own: they describe the same key space, they are edited in the same
    /// panel, and a second store keyed by the same chat would only be able to
    /// drift out of step with this one.
    #[serde(default)]
    pub panels: super::state_panel::StatePanelConfig,
    /// The rules that move this key space, when the scene has any.
    ///
    /// A scene is one thing to its author — the fields, how they are shown, and
    /// what makes them change — so the machine travels inside the declaration
    /// instead of a second store bound separately. Absent means the scene has no
    /// stages, exactly as a chat with no bound machine behaves today; the
    /// runtime then falls back to a machine bound on its own, which is how a
    /// configuration written before this key existed keeps working.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<super::state_machine::StateMachineSpec>,
    /// The conditionally injected text of this scene, when it has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicates: Option<super::state_predicate::StatePredicateSet>,
    /// What this scene lets a value cost, and in what unit.
    ///
    /// It belongs to the scene rather than to the app: how long a value may be
    /// is the same kind of statement as which fields exist, and two scenes in
    /// one installation have no reason to agree on it. A declaration written
    /// before this key existed reads as the defaults.
    #[serde(default)]
    pub limits: StateLimits,
}

/// What the declaration made of one key supplied by a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResolution {
    /// Exactly one declared field accepted the key. `key` is the name to store.
    Matched {
        key: StateKey,
        label: String,
        /// Normalization steps that changed the caller's spelling, if any.
        steps: Vec<&'static str>,
    },
    /// No declaration is configured, so only the key shape applied.
    Unconstrained {
        key: StateKey,
        steps: Vec<&'static str>,
    },
    /// Nothing in the declaration accepted the key.
    Undeclared {
        normalized: String,
        steps: Vec<&'static str>,
    },
    /// More than one declared field accepted the key.
    Ambiguous {
        candidates: Vec<String>,
        normalized: String,
    },
    /// The key is unusable regardless of the declaration.
    Unusable { message: String },
}

impl StateDeclaration {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Initial values a field may not carry, for save-time validation.
    ///
    /// Refused rather than trimmed: a value silently dropped here would look like
    /// a scene that ignores what the user typed. The ceilings are measured in
    /// characters, which is the right check for a scene that counts in them —
    /// [`Self::initial_value_shape_errors`] and
    /// [`Self::initial_value_ceiling_errors`] are the two halves a scene counting
    /// in something else needs.
    pub fn initial_value_errors(&self) -> Vec<String> {
        let mut errors = self.initial_value_shape_errors();
        errors.extend(self.initial_value_ceiling_errors(&|value| Ok(count_chars(value))));
        errors
    }

    /// The shape rules, with no ceiling: a literal key, no empty values.
    pub fn initial_value_shape_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for field in &self.fields {
            if field.initial.is_empty() {
                continue;
            }
            if !field.pattern.is_literal() {
                errors.push(format!(
                    "`{}` is a pattern: only a literal key can carry an initial value",
                    field.pattern.as_str()
                ));
            }
            for value in &field.initial {
                if value.is_empty() {
                    errors.push(format!(
                        "`{}` has an empty initial value; remove it or give it a value",
                        field.pattern.as_str()
                    ));
                }
            }
        }
        errors
    }

    /// The ceiling rules, measured the caller's way.
    ///
    /// The domain compares a cost against the scene's ceiling and never learns
    /// what unit it is: a scene counting in tokens is checked by whoever holds
    /// the vocabulary.
    pub fn initial_value_ceiling_errors(
        &self,
        measure: &dyn Fn(&str) -> Result<usize, DomainError>,
    ) -> Vec<String> {
        let limits = &self.limits;
        let mut errors = Vec::new();
        for field in &self.fields {
            if field.initial.is_empty() {
                continue;
            }
            if let Some(ceiling) = limits.values_ceiling()
                && field.initial.len() > ceiling
            {
                errors.push(format!(
                    "`{}` has {} initial values, more than the {ceiling} a field may hold",
                    field.pattern.as_str(),
                    field.initial.len()
                ));
            }
            for value in &field.initial {
                if value.is_empty() {
                    continue;
                }
                let Some(ceiling) = limits.value_ceiling() else {
                    continue;
                };
                match measure(value) {
                    Ok(cost) if cost > ceiling => errors.push(format!(
                        "`{}` has an initial value longer than {ceiling} {}",
                        field.pattern.as_str(),
                        limits.unit.label()
                    )),
                    Ok(_) => {}
                    // A value that cannot be measured has no known cost, so it
                    // cannot be declared within the ceiling either.
                    Err(error) => errors.push(format!(
                        "`{}` has an initial value that could not be measured: {error}",
                        field.pattern.as_str()
                    )),
                }
            }
        }
        errors
    }

    /// Declared keys that can accept the same key, for save-time validation.
    pub fn overlaps(&self) -> Vec<PatternOverlap> {
        let patterns = self
            .fields
            .iter()
            .map(|field| field.pattern.clone())
            .collect::<Vec<_>>();
        check_pattern_overlaps(&patterns)
    }

    /// What the declared field that owns this canonical key grants.
    ///
    /// The key must already be canonical — every caller reaches this after
    /// [`StateDeclaration::resolve`] accepted it — so the lookup is an exact
    /// match with case folding as the same fallback `resolve` uses. `None` means
    /// the declaration does not own the key, which is only reachable when no
    /// declaration is configured at all.
    pub fn access_for(&self, key: &StateKey) -> Option<StateFieldAccess> {
        self.fields
            .iter()
            .find(|field| field.pattern.matches_exact(key.as_str()))
            .or_else(|| {
                self.fields
                    .iter()
                    .find(|field| field.pattern.matches_tolerant(key.as_str()))
            })
            .map(|field| field.access)
    }

    /// Resolve a caller-supplied key against the declaration.
    ///
    /// Tolerance is applied in two ordered stages, and each stage only runs
    /// when the previous one found nothing: first the mechanical normalization
    /// of width, separators, and whitespace, then case folding. A declaration
    /// that deliberately keeps two case variants of one name therefore still
    /// resolves exactly instead of becoming ambiguous.
    ///
    /// A literal declaration owns the spelling, so drift is pulled back to the
    /// declared name. A pattern delegates the name to the caller, because the
    /// wildcard is exactly where the caller is supposed to choose.
    pub fn resolve(&self, raw: &str) -> KeyResolution {
        let normalized = normalize_key(raw);

        if self.is_empty() {
            return match StateKey::parse(&normalized.text) {
                Ok(key) => KeyResolution::Unconstrained {
                    key,
                    steps: normalized.steps,
                },
                Err(message) => KeyResolution::Unusable { message },
            };
        }

        if normalized.text.is_empty() {
            return KeyResolution::Unusable {
                message: "key is required".to_string(),
            };
        }

        if let Some(field) = self.single_match(&normalized.text, false) {
            return accept(field, &normalized.text, normalized.steps);
        }
        if let Some(field) = self.single_match(&normalized.text, true) {
            return accept(field, &normalized.text, normalized.steps);
        }

        let candidates = self
            .fields
            .iter()
            .filter(|field| field.pattern.matches_tolerant(&normalized.text))
            .map(|field| field.pattern.as_str().to_string())
            .collect::<Vec<_>>();

        if candidates.len() > 1 {
            return KeyResolution::Ambiguous {
                candidates,
                normalized: normalized.text,
            };
        }

        KeyResolution::Undeclared {
            normalized: normalized.text,
            steps: normalized.steps,
        }
    }

    /// Find the one field that accepts `text`, or nothing when zero or several do.
    fn single_match(&self, text: &str, tolerant: bool) -> Option<&DeclaredStateField> {
        let mut found = None;
        for field in &self.fields {
            let hit = if tolerant {
                field.pattern.matches_tolerant(text)
            } else {
                field.pattern.matches_exact(text)
            };
            if hit {
                if found.is_some() {
                    return None;
                }
                found = Some(field);
            }
        }
        found
    }
}

fn accept(field: &DeclaredStateField, normalized: &str, steps: Vec<&'static str>) -> KeyResolution {
    let text = if field.pattern.is_literal() {
        field.pattern.as_str()
    } else {
        normalized
    };

    match StateKey::parse(text) {
        Ok(key) => KeyResolution::Matched {
            key,
            label: field.label.clone(),
            steps,
        },
        Err(message) => KeyResolution::Unusable { message },
    }
}

/// A raw update as it arrives from tool arguments, before any key is resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateUpdateRequest {
    /// `(raw key, values)` pairs. An empty value list clears the field.
    pub fields: Vec<(String, Vec<String>)>,
    /// Raw keys to delete.
    pub remove: Vec<String>,
}

/// A request whose keys have been resolved against the declaration.
///
/// Key strings from a model can drift — width, separators, whitespace, or a
/// pattern wildcard — so the resolver, not the applier, owns the mapping from
/// what the caller sent to the canonical key that gets stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedStateUpdateRequest {
    /// `(key, values)` pairs. An empty value list clears the field.
    pub fields: Vec<(StateKey, Vec<String>)>,
    /// Keys to delete.
    pub remove: Vec<StateKey>,
}

/// One validation problem, addressable to a single key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateUpdateError {
    /// The offending key, as the caller wrote it, so the model can find it.
    pub key: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl StateUpdateError {
    fn new(key: Option<&str>, code: &'static str, message: String) -> Self {
        Self {
            key: key.map(str::to_string),
            code,
            message,
        }
    }
}

/// Resolve a raw request against the declaration in one batch.
///
/// Every problem is collected instead of stopping at the first one, so the
/// caller can report them all at once and the model can fix the whole
/// submission in a single retry. A rejection returns no partial result.
///
/// Removals resolve first, so a key that is both updated and removed is caught
/// against canonical keys, not the caller's spelling — `环境/日期` and a
/// drifted ` 环境／日期 ` are the same key here.
pub fn resolve_request(
    declaration: &StateDeclaration,
    request: &StateUpdateRequest,
) -> Result<ResolvedStateUpdateRequest, Vec<StateUpdateError>> {
    resolve_request_measured(declaration, request, &|value| Ok(count_chars(value)))
}

/// The same resolution, for a scene that counts its values in something else.
///
/// `measure` is what one value costs, and the domain only ever compares it
/// against the scene's ceiling: counting in tokens stays the vocabulary's
/// business, and every caller that has no vocabulary passes characters. Both
/// entries exist so that a scene counting in characters — the default, and
/// every scene written before this key existed — cannot accidentally depend on
/// a tokenizer being loaded.
pub fn resolve_request_measured(
    declaration: &StateDeclaration,
    request: &StateUpdateRequest,
    measure: &dyn Fn(&str) -> Result<usize, DomainError>,
) -> Result<ResolvedStateUpdateRequest, Vec<StateUpdateError>> {
    let limits = &declaration.limits;
    let mut errors = Vec::new();

    if let Some(ceiling) = limits.fields_ceiling()
        && request.fields.len() > ceiling
    {
        errors.push(StateUpdateError::new(
            None,
            "state.too_many_fields",
            format!(
                "update carries {} fields, at most {ceiling} are allowed",
                request.fields.len()
            ),
        ));
        return Err(errors);
    }

    let mut remove = Vec::with_capacity(request.remove.len());
    let mut removed = std::collections::BTreeSet::new();
    for raw_key in &request.remove {
        match resolve_key(declaration, raw_key, &mut errors) {
            Some(key) => {
                // Removing the same key twice is idempotent in intent, unlike
                // supplying two different values for one key.
                if removed.insert(key.clone()) {
                    remove.push(key);
                }
            }
            None => continue,
        }
    }

    let mut fields = Vec::with_capacity(request.fields.len());
    let mut seen = std::collections::BTreeSet::new();
    for (raw_key, values) in &request.fields {
        let key = match resolve_key(declaration, raw_key, &mut errors) {
            Some(key) => key,
            None => continue,
        };
        if !seen.insert(key.clone()) {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.duplicate_key",
                format!("key `{raw_key}` appears more than once in this update"),
            ));
            continue;
        }
        if removed.contains(&key) {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.conflicting_key",
                format!("key `{raw_key}` cannot be both updated and removed"),
            ));
            continue;
        }
        if let Some(ceiling) = limits.values_ceiling()
            && values.len() > ceiling
        {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.too_many_values",
                format!(
                    "key `{raw_key}` carries {} values, at most {ceiling} are allowed",
                    values.len()
                ),
            ));
            continue;
        }
        match check_field_values(raw_key, values, limits, measure) {
            Ok(cleaned) => fields.push((key, cleaned)),
            Err(mut value_errors) => errors.append(&mut value_errors),
        }
    }

    if errors.is_empty() {
        Ok(ResolvedStateUpdateRequest { fields, remove })
    } else {
        Err(errors)
    }
}

/// Resolve one key, reporting the caller's own spelling on failure.
fn resolve_key(
    declaration: &StateDeclaration,
    raw_key: &str,
    errors: &mut Vec<StateUpdateError>,
) -> Option<StateKey> {
    match declaration.resolve(raw_key) {
        KeyResolution::Matched { key, .. } | KeyResolution::Unconstrained { key, .. } => Some(key),
        KeyResolution::Undeclared { .. } => {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.undeclared_key",
                format!("key `{raw_key}` is not in the state declaration"),
            ));
            None
        }
        KeyResolution::Ambiguous { candidates, .. } => {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.ambiguous_key",
                format!(
                    "key `{raw_key}` matches more than one declared field: {}",
                    candidates.join(", ")
                ),
            ));
            None
        }
        KeyResolution::Unusable { message } => {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.invalid_key",
                message,
            ));
            None
        }
    }
}

/// Check the values carried by one field, collecting every value's problems.
fn check_field_values(
    raw_key: &str,
    values: &[String],
    limits: &StateLimits,
    measure: &dyn Fn(&str) -> Result<usize, DomainError>,
) -> Result<Vec<String>, Vec<StateUpdateError>> {
    let mut errors = Vec::new();
    let mut cleaned = Vec::with_capacity(values.len());
    for value in values {
        let cost = match measure(value) {
            Ok(cost) => cost,
            // An unmeasurable value has no known cost, so it cannot be shown to
            // fit the ceiling; refusing it keeps the ceiling meaning one thing.
            Err(error) => {
                errors.push(StateUpdateError::new(
                    Some(raw_key),
                    "state.value_unmeasurable",
                    format!("one value of `{raw_key}` could not be measured: {error}"),
                ));
                continue;
            }
        };
        if let Some(ceiling) = limits.value_ceiling()
            && cost > ceiling
        {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.value_too_long",
                format!(
                    "one value of `{raw_key}` exceeds {ceiling} {}",
                    limits.unit.label()
                ),
            ));
            continue;
        }
        if value.trim().is_empty() {
            errors.push(StateUpdateError::new(
                Some(raw_key),
                "state.empty_value",
                format!(
                    "`{raw_key}` has an empty value; send an empty value list to clear the field instead"
                ),
            ));
            continue;
        }
        cleaned.push(value.clone());
    }
    if errors.is_empty() {
        Ok(cleaned)
    } else {
        Err(errors)
    }
}

/// Apply a resolved update to a document.
///
/// The request must come from [`resolve_request`]: keys canonical, values
/// checked. The contradiction checks are repeated here because this is a public
/// entry point and a request assembled by hand bypasses the resolver; nothing
/// is mutated on rejection.
pub fn apply_update(
    document: &StateDocument,
    request: &ResolvedStateUpdateRequest,
) -> Result<StateDocument, Vec<StateUpdateError>> {
    let mut errors = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (key, _) in &request.fields {
        if !seen.insert(key) {
            errors.push(StateUpdateError::new(
                Some(key.as_str()),
                "state.duplicate_key",
                format!("key `{key}` appears more than once in this update"),
            ));
        }
    }
    for key in &request.remove {
        if seen.contains(key) {
            errors.push(StateUpdateError::new(
                Some(key.as_str()),
                "state.conflicting_key",
                format!("key `{key}` cannot be both updated and removed"),
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut document = document.clone();
    for key in &request.remove {
        document.fields.retain(|field| &field.key != key);
    }
    for (key, values) in &request.fields {
        match document.fields.iter_mut().find(|field| field.key == *key) {
            Some(field) => field.values = values.clone(),
            None => document.fields.push(StateField {
                key: key.clone(),
                values: values.clone(),
            }),
        }
    }

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::{
        CostUnit, KeyResolution, MAX_VALUE_CHARS, ResolvedStateUpdateRequest, StateDeclaration,
        StateDocument, StateField, StateKey, StateKeyPattern, StateLimits, StateUpdateError,
        StateUpdateRequest, apply_update, resolve_request, resolve_request_measured,
    };
    use crate::errors::DomainError;

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    fn declaration(entries: &[(&str, &str)]) -> StateDeclaration {
        StateDeclaration {
            fields: entries
                .iter()
                .map(|(pattern, label)| super::DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern)
                        .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
                    label: label.to_string(),
                    access: super::StateFieldAccess::DECLARED,
                    initial: Vec::new(),
                })
                .collect(),
            panels: Default::default(),
            machine: None,
            predicates: None,
            limits: Default::default(),
        }
    }

    fn document(fields: &[(&str, &[&str])]) -> StateDocument {
        StateDocument {
            fields: fields
                .iter()
                .map(|(k, values)| StateField {
                    key: key(k),
                    values: values.iter().map(|v| v.to_string()).collect(),
                })
                .collect(),
        }
    }

    /// The unconstrained path: resolve against no declaration, then apply.
    fn apply(
        document: &StateDocument,
        request: &StateUpdateRequest,
    ) -> Result<StateDocument, Vec<StateUpdateError>> {
        match resolve_request(&StateDeclaration::default(), request) {
            Ok(resolved) => apply_update(document, &resolved),
            Err(errors) => Err(errors),
        }
    }

    fn request(
        fields: Vec<(&str, Vec<&str>)>,
        remove: Vec<&str>,
    ) -> StateUpdateRequest {
        StateUpdateRequest {
            fields: fields
                .into_iter()
                .map(|(key, values)| {
                    (key.to_string(), values.into_iter().map(str::to_string).collect())
                })
                .collect(),
            remove: remove.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn a_document_reads_as_the_field_map_conditions_compare_against() {
        let document = document(&[("环境/日期", &["2026/09/10"]), ("环境/地点", &[])]);

        let fields = document.condition_fields();

        assert_eq!(fields.get("环境/日期"), Some(&vec!["2026/09/10".to_string()]));
        assert_eq!(
            fields.get("环境/地点"),
            Some(&Vec::new()),
            "a cleared field is present with no values, not missing"
        );
        assert!(StateDocument::default().condition_fields().is_empty());
    }

    #[test]
    fn update_leaves_absent_fields_untouched() {
        let before = document(&[("环境/时间", &["下午"]), ("环境/地点", &["咖啡馆"])]);
        let after = apply(
            &before,
            &request(vec![("环境/时间", vec!["夜晚"])], vec![]),
        )
        .expect("update must apply");

        assert_eq!(after.get(&key("环境/时间")).unwrap().values, ["夜晚"]);
        assert_eq!(after.get(&key("环境/地点")).unwrap().values, ["咖啡馆"]);
    }

    #[test]
    fn empty_value_list_clears_without_deleting_the_field() {
        let before = document(&[("环境/地点", &["咖啡厅"])]);
        let after = apply(&before, &request(vec![("环境/地点", vec![])], vec![]))
            .expect("update must apply");

        let field = after
            .get(&key("环境/地点"))
            .expect("cleared field must still exist");
        assert!(field.values.is_empty());
    }

    #[test]
    fn removal_deletes_the_field() {
        let before = document(&[("环境/地点", &["咖啡厅"]), ("环境/时间", &["下午"])]);
        let after = apply(&before, &request(vec![], vec!["环境/地点"])).expect("update must apply");

        assert!(after.get(&key("环境/地点")).is_none());
        assert!(after.get(&key("环境/时间")).is_some());
    }

    #[test]
    fn removing_an_absent_field_is_a_no_op() {
        let before = document(&[("环境/时间", &["下午"])]);
        let after = apply(&before, &request(vec![], vec!["环境/地点"])).expect("update must apply");

        assert_eq!(after, before);
    }

    #[test]
    fn one_key_cannot_be_updated_and_removed_in_the_same_request() {
        let errors = apply(
            &StateDocument::default(),
            &request(vec![("环境/时间", vec!["夜晚"])], vec!["环境/时间"]),
        )
        .expect_err("contradictory update must be rejected");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "state.conflicting_key");
        assert_eq!(errors[0].key.as_deref(), Some("环境/时间"));
    }

    #[test]
    fn a_key_written_differently_is_the_same_key() {
        let before = document(&[("环境/时间", &["下午"])]);
        let after = apply(
            &before,
            // Fullwidth separators, padding, and a trailing separator: all
            // transcription drift, none of it a different field.
            &request(vec![(" 环境／时间/ ", vec!["夜晚"])], vec![]),
        )
        .expect("drift must be absorbed");

        assert_eq!(after.get(&key("环境/时间")).unwrap().values, ["夜晚"]);
        assert_eq!(after.fields.len(), 1, "drift must not create a second field");
    }

    #[test]
    fn every_problem_is_reported_in_one_batch() {
        let oversized = "x".repeat(MAX_VALUE_CHARS + 1);
        let errors = apply(
            &StateDocument::default(),
            &request(
                vec![
                    ("环境/来源", vec!["  "]),
                    ("环境/地点", vec![oversized.as_str()]),
                    ("环境/来源", vec!["日历"]),
                ],
                vec![],
            ),
        )
        .expect_err("invalid update must be rejected");

        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            [
                "state.empty_value",
                "state.value_too_long",
                "state.duplicate_key"
            ],
            "a rejected update must report every problem at once"
        );
    }

    #[test]
    fn key_and_value_problems_are_reported_in_the_same_batch() {
        // With no declaration every well-shaped key resolves, so use one to
        // force a key problem alongside a value problem.
        let declared = declaration(&[("环境/日期", "DATE")]);
        let errors = resolve_request(
            &declared,
            &request(
                vec![("环境/时间", vec!["下午"]), ("环境/日期", vec!["  "])],
                vec![],
            ),
        )
        .expect_err("mixed problems must all be reported");

        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            ["state.undeclared_key", "state.empty_value"],
            "the model must be able to fix both in one retry"
        );
    }

    #[test]
    fn structural_bounds_reject_unusable_keys() {
        for raw in [
            "", " 时间", "时间 ", "时间/", "/时间", "时间//日期", "时间\n/日期",
        ] {
            assert!(
                StateKey::parse(raw).is_err(),
                "`{raw}` must be rejected as unusable"
            );
        }

        let too_deep = "环境/日期/时间/地点/备注/来源/编号/颜色/材质";
        assert!(StateKey::parse(too_deep).is_err(), "depth bound must apply");
    }

    #[test]
    fn normalization_is_an_entry_tolerance_not_a_loosening_of_the_type() {
        // The stored type stays strict; only the key a caller supplies is
        // repaired on the way in.
        assert!(StateKey::parse(" 环境 / 时间 ").is_err());
        assert_eq!(
            super::normalize_key(" 环境 / 时间 ").text,
            "环境/时间",
            "the same input must become a valid key"
        );
    }

    #[test]
    fn the_key_space_comes_from_the_declaration_not_from_the_domain() {
        // Arbitrary user-chosen names, a flat single-segment key, and a deeper
        // path than any example: the domain must accept all of them.
        for raw in ["时间线/幕次/标签", "日期", "装备/主手/附魔/等级"] {
            assert_eq!(key(raw).as_str(), raw);
        }

        let declared = declaration(&[("时间线/幕次/标签", "ACT")]);
        let resolved = resolve_request(
            &declared,
            &request(vec![("时间线/幕次/标签", vec!["第二幕"])], vec![]),
        )
        .expect("a declared key must resolve");
        assert_eq!(resolved.fields[0].0.as_str(), "时间线/幕次/标签");

        let errors = resolve_request(
            &declared,
            &request(vec![("其它/地点", vec!["咖啡厅"])], vec![]),
        )
        .expect_err("an undeclared key must be rejected");
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            ["state.undeclared_key"]
        );
    }

    #[test]
    fn a_literal_declaration_pulls_drift_back_to_the_declared_spelling() {
        let declared = declaration(&[("环境/日期", "DATE")]);

        match declared.resolve("环境 ／ 日期") {
            KeyResolution::Matched { key, label, steps } => {
                assert_eq!(key.as_str(), "环境/日期");
                assert_eq!(label, "DATE");
                assert!(!steps.is_empty(), "the absorbed drift must be reported");
            }
            other => panic!("expected a match, got {other:?}"),
        }
    }

    #[test]
    fn a_pattern_declaration_lets_the_caller_name_the_entity() {
        let declared = declaration(&[("环境/*/来源", "SOURCE")]);

        match declared.resolve("环境/日期/来源") {
            KeyResolution::Matched { key, .. } => {
                assert_eq!(
                    key.as_str(),
                    "环境/日期/来源",
                    "the wildcard is where the caller chooses, so the name must survive"
                );
            }
            other => panic!("expected a match, got {other:?}"),
        }

        assert!(matches!(
            declared.resolve("环境/日期/地点"),
            KeyResolution::Undeclared { .. }
        ));
    }

    #[test]
    fn case_variants_do_not_make_an_exact_key_ambiguous() {
        let declared = declaration(&[("Date", "Date"), ("date", "date")]);

        match declared.resolve("Date") {
            KeyResolution::Matched { label, .. } => assert_eq!(label, "Date"),
            other => panic!("an exact name must win over a folded one, got {other:?}"),
        }

        assert!(
            matches!(declared.resolve("DATE"), KeyResolution::Ambiguous { .. }),
            "a spelling that only folds into both must be refused, not guessed"
        );
    }

    #[test]
    fn an_empty_declaration_still_checks_the_key_shape() {
        let declared = StateDeclaration::default();

        assert!(matches!(
            declared.resolve("任意/键名"),
            KeyResolution::Unconstrained { .. }
        ));
        assert!(matches!(
            declared.resolve(""),
            KeyResolution::Unusable { .. }
        ));
    }

    #[test]
    fn an_unusable_key_is_reported_not_passed_through() {
        let declared = declaration(&[("环境/日期", "DATE")]);
        let errors = resolve_request(
            &declared,
            // Nothing is left after normalization, so there is no key to store
            // and no declaration entry that could own it.
            &request(vec![("   ", vec!["下午"])], vec![]),
        )
        .expect_err("an unusable key must be rejected");

        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            ["state.invalid_key"],
            "a key that cannot even be shaped must be refused, never stored raw"
        );
    }

    #[test]
    fn overlapping_declarations_are_reported_at_save_time() {
        let declared = declaration(&[("环境/*/来源", "SOURCE"), ("环境/日期/来源", "DATE")]);

        assert_eq!(declared.overlaps().len(), 1);
    }

    #[test]
    fn undeclared_keys_are_rejected_only_when_a_declaration_exists() {
        let bare = request(vec![("环境/地点", vec!["咖啡厅"])], vec![]);

        assert!(
            resolve_request(&StateDeclaration::default(), &bare).is_ok(),
            "an empty declaration constrains nothing"
        );

        let declared = declaration(&[("环境/时间", "TIME")]);
        let errors = resolve_request(&declared, &bare).expect_err("must be rejected");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "state.undeclared_key");
    }

    #[test]
    fn a_key_that_matches_two_declarations_is_reported_once_per_key() {
        // Overlaps are refused when the declaration is saved, but a declaration
        // loaded from an older file must still fail loudly rather than pick.
        let declared = declaration(&[("环境/*/来源", "SOURCE"), ("环境/日期/*", "DATE")]);
        let errors = resolve_request(
            &declared,
            &request(vec![("环境/日期/来源", vec!["日历"])], vec![]),
        )
        .expect_err("must be rejected");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "state.ambiguous_key");
    }

    #[test]
    fn a_hand_built_resolved_request_cannot_sneak_in_a_conflict() {
        let before = document(&[("环境/时间", &["下午"])]);

        let errors = apply_update(
            &before,
            &ResolvedStateUpdateRequest {
                fields: vec![
                    (key("环境/时间"), vec!["夜晚".to_string()]),
                    (key("环境/时间"), vec!["清晨".to_string()]),
                ],
                remove: vec![],
            },
        )
        .expect_err("a duplicated key must be refused");

        assert_eq!(errors[0].code, "state.duplicate_key");
        assert_eq!(
            before.get(&key("环境/时间")).unwrap().values,
            ["下午"],
            "a rejected request must not mutate anything"
        );
    }

    #[test]
    fn a_rejected_update_changes_nothing() {
        let before = document(&[("环境/时间", &["下午"])]);
        let errors = apply(
            &before,
            &request(
                vec![
                    ("环境/地点", vec!["咖啡馆"]),
                    ("环境/日期/时间/地点/备注/来源/编号/颜色/材质", vec!["超深"]),
                ],
                vec![],
            ),
        )
        .expect_err("invalid update must be rejected");

        assert!(!errors.is_empty());
        assert_eq!(before.get(&key("环境/地点")), None);
    }

    #[test]
    fn a_scene_counting_in_tokens_is_measured_by_its_caller() {
        // The domain never learns what a token is: it compares a cost against the
        // scene's ceiling, and producing the cost is the caller's job. That is
        // what lets a vocabulary live outside the document.
        let declaration = StateDeclaration {
            limits: StateLimits {
                value: 2,
                unit: CostUnit::Tokens,
                ..Default::default()
            },
            ..declaration(&[("环境/备注", "NOTE")])
        };
        let words = |text: &str| Ok(text.split_whitespace().count());

        let errors = resolve_request_measured(
            &declaration,
            &request(vec![("环境/备注", vec!["一万 二千 三百"])], vec![]),
            &words,
        )
        .expect_err("three words are over a two-token ceiling");
        assert_eq!(errors[0].code, "state.value_too_long");
        assert!(errors[0].message.contains("2 tokens"), "{:?}", errors[0].message);

        // Without a measure it is characters again, and the same text fits.
        assert!(
            resolve_request(
                &declaration,
                &request(vec![("环境/备注", vec!["一万 二千 三百"])], vec![]),
            )
            .is_err(),
            "the very same scene counts characters when nobody measures tokens"
        );
    }

    #[test]
    fn a_value_that_cannot_be_measured_is_refused() {
        // No known cost means it cannot be shown to fit the ceiling, so the
        // ceiling must refuse it rather than quietly measure something else.
        let declaration = StateDeclaration {
            limits: StateLimits {
                value: 2,
                unit: CostUnit::Tokens,
                ..Default::default()
            },
            ..declaration(&[("环境/备注", "NOTE")])
        };
        let broken = |_text: &str| Err(DomainError::InternalError("no vocabulary".to_string()));

        let errors = resolve_request_measured(
            &declaration,
            &request(vec![("环境/备注", vec!["一万"])], vec![]),
            &broken,
        )
        .expect_err("an unmeasurable value cannot be accepted");

        assert_eq!(errors[0].code, "state.value_unmeasurable");
    }

    #[test]
    fn a_scene_may_drop_its_ceilings() {
        let declaration = StateDeclaration {
            limits: StateLimits::UNBOUNDED,
            ..declaration(&[("环境/备注", "NOTE")])
        };
        let long = "x".repeat(MAX_VALUE_CHARS * 4);

        assert!(
            resolve_request(
                &declaration,
                &request(vec![("环境/备注", vec![long.as_str()])], vec![]),
            )
            .is_ok(),
            "a scene that set no ceiling does not get one back"
        );
    }

    #[test]
    fn oversized_value_is_rejected() {
        let oversized = "x".repeat(MAX_VALUE_CHARS + 1);
        let errors = apply(
            &StateDocument::default(),
            &request(vec![("环境/地点", vec![oversized.as_str()])], vec![]),
        )
        .expect_err("oversized value must be rejected");

        assert_eq!(errors[0].code, "state.value_too_long");
    }

    /// One declared row, with an initial value and the switches tests ignore.
    fn declared_with_initial(pattern: &str, values: &[&str]) -> super::DeclaredStateField {
        super::DeclaredStateField {
            pattern: StateKeyPattern::parse(pattern).expect("test pattern must parse"),
            label: pattern.to_string(),
            access: crate::models::state_access::StateFieldAccess::DECLARED,
            initial: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn a_chat_starts_from_the_initial_values_its_scene_declares() {
        let declaration = StateDeclaration {
            fields: vec![
                declared_with_initial("环境/日期", &["2026/09/10"]),
                // A row with no initial value is a field the model writes, not a
                // field that starts out holding something.
                declared_with_initial("环境/天气", &[]),
            ],
            ..Default::default()
        };

        let document = StateDocument::seeded(&declaration);

        assert_eq!(document.fields.len(), 1);
        assert_eq!(document.fields[0].key.as_str(), "环境/日期");
        assert_eq!(document.fields[0].values, ["2026/09/10"]);
        assert!(declaration.initial_value_errors().is_empty());
    }

    #[test]
    fn a_pattern_cannot_carry_an_initial_value() {
        // Nothing has been named yet, so there is no key to seed: inventing one
        // is the thing a declaration exists to prevent.
        let declaration = StateDeclaration {
            fields: vec![declared_with_initial("角色/*/好感度", &["50"])],
            ..Default::default()
        };

        assert!(StateDocument::seeded(&declaration).fields.is_empty());
        let errors = declaration.initial_value_errors();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("角色/*/好感度"), "{errors:?}");
    }

    #[test]
    fn an_unusable_initial_value_is_reported_not_trimmed() {
        let oversized = "x".repeat(MAX_VALUE_CHARS + 1);
        let declaration = StateDeclaration {
            fields: vec![declared_with_initial("环境/备注", &["", &oversized])],
            ..Default::default()
        };

        let errors = declaration.initial_value_errors();

        assert_eq!(errors.len(), 2, "{errors:?}");
    }
}
