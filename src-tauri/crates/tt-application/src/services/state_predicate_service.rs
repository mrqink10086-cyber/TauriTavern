//! The predicate set store: named documents, validated before they are written.
//!
//! The domain decides what a set means and which entries it selects; this
//! service owns what the domain must not touch — the name, the storage, and the
//! declaration-aware part of the check (a condition reads a *declared* field,
//! and only the bound declaration knows which keys those are).

use std::collections::BTreeMap;
use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::state::{StateDeclaration, StateDocument, StateField, StateKey};
use tt_domain::models::state_machine::{ComparatorRegistry, ConditionContext, ConditionSpec};
use tt_domain::models::state_predicate::{
    PredicateError, PredicateEvaluation, StatePredicateSet, evaluate_predicates,
    validate_predicate_set,
};
use tt_ports::repositories::state_predicate_repository::StatePredicateRepository;

use crate::errors::ApplicationError;

pub struct StatePredicateService {
    repository: Arc<dyn StatePredicateRepository>,
    comparators: ComparatorRegistry,
}

impl StatePredicateService {
    pub fn new(repository: Arc<dyn StatePredicateRepository>) -> Self {
        Self {
            repository,
            comparators: ComparatorRegistry::default(),
        }
    }

    /// Extend the condition vocabulary.
    ///
    /// The same registry the machine and the panels use: one condition language,
    /// so a comparator registered for one of them is available to all three.
    pub fn register_comparator(
        &mut self,
        name: impl Into<String>,
        comparator: fn(&ConditionContext, &ConditionSpec) -> bool,
    ) {
        self.comparators.register(name, comparator);
    }

    fn validate_name(name: &str) -> Result<String, ApplicationError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApplicationError::ValidationError(
                "state_predicate.invalid_name: name must not be empty".to_string(),
            ));
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            return Err(ApplicationError::ValidationError(format!(
                "state_predicate.invalid_name: name `{trimmed}` must not contain a path separator"
            )));
        }
        Ok(trimmed.to_string())
    }

    /// Check a set against a bound declaration, if one exists.
    pub fn validate(
        &self,
        set: &StatePredicateSet,
        declaration: Option<&StateDeclaration>,
    ) -> Vec<PredicateError> {
        validate_predicate_set(set, declaration, &self.comparators)
    }

    /// Save a set, refusing it when it does not hold together.
    pub fn save(
        &self,
        name: &str,
        set: &StatePredicateSet,
        declaration: Option<&StateDeclaration>,
    ) -> Result<(), ApplicationError> {
        let name = Self::validate_name(name)?;
        let errors = self.validate(set, declaration);
        if !errors.is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "state.predicate_invalid_set:\n{}",
                format_predicate_errors(&errors)
            )));
        }
        self.repository
            .save_predicates(&name, set)
            .map_err(|error| {
                tracing::error!("Failed to save state predicates {name}: {error}");
                error.into()
            })
    }

    pub fn get(&self, name: &str) -> Result<StatePredicateSet, ApplicationError> {
        self.repository
            .get_predicates(name)
            .map_err(|error| match error {
                DomainError::NotFound(_) => {
                    ApplicationError::NotFound(format!("State predicates not found: {name}"))
                }
                other => other.into(),
            })
    }

    pub fn list_names(&self) -> Result<Vec<String>, ApplicationError> {
        self.repository
            .list_predicate_names()
            .map_err(Into::into)
    }

    pub fn delete(&self, name: &str) -> Result<(), ApplicationError> {
        self.repository.delete_predicates(name).map_err(|error| match error {
            DomainError::NotFound(_) => {
                ApplicationError::NotFound(format!("State predicates not found: {name}"))
            }
            other => other.into(),
        })
    }

    /// Resolve one set against one state document.
    ///
    /// The document is the caller's: a preview resolves against assumed values,
    /// the assembly path against the chat's newest committed one. Making that the
    /// caller's choice is what keeps a preview from having to fake a chat.
    pub fn evaluate(
        &self,
        set: &StatePredicateSet,
        document: &StateDocument,
    ) -> Result<PredicateEvaluation, ApplicationError> {
        evaluate_predicates(set, document, &self.comparators).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.predicate_invalid_read:\n{}",
                format_predicate_errors(std::slice::from_ref(&error))
            ))
        })
    }

    /// Resolve one set against assumed field values.
    ///
    /// The editor's preview has no chat to read, so it sends the values it wants
    /// judged instead. Building the document here keeps key parsing in the
    /// domain rather than in the command layer, and the rest of the evaluation
    /// is the same path the assembly uses.
    pub fn evaluate_fields(
        &self,
        set: &StatePredicateSet,
        fields: &BTreeMap<String, Vec<String>>,
    ) -> Result<PredicateEvaluation, ApplicationError> {
        let mut document = StateDocument::default();
        for (key, values) in fields {
            let key = state_key_from_preview(key)?;
            document.fields.push(StateField {
                key,
                values: values.clone(),
            });
        }
        document.validate().map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.predicate_preview_field_invalid: {error}"
            ))
        })?;
        self.evaluate(set, &document)
    }

    /// The comparators in force, for callers that evaluate on their own.
    pub fn comparators(&self) -> &ComparatorRegistry {
        &self.comparators
    }
}

/// One preview field key, as the state document stores it.
///
/// A caller sends only the rows it filled in, so a key that does not parse is a
/// real refusal rather than a half-typed row to tolerate.
fn state_key_from_preview(raw: &str) -> Result<StateKey, ApplicationError> {
    StateKey::parse(raw).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "state.predicate_preview_field_invalid: {error}"
        ))
    })
}

/// One line per problem, so a caller can show all of them at once.
pub fn format_predicate_errors(errors: &[PredicateError]) -> String {
    errors
        .iter()
        .map(|error| match &error.target {
            Some(target) => format!("- {} ({target}): {}", error.code, error.message),
            None => format!("- {}: {}", error.code, error.message),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tt_domain::models::state_predicate::{
        PredicateEffect, PredicateEffectKind, PredicateEntry, PredicateGroup,
    };

    struct FakeRepository {
        stored: Mutex<Option<StatePredicateSet>>,
        saved: Mutex<Vec<String>>,
    }

    impl StatePredicateRepository for FakeRepository {
        fn save_predicates(&self, name: &str, set: &StatePredicateSet) -> Result<(), DomainError> {
            self.saved.lock().unwrap().push(name.to_string());
            *self.stored.lock().unwrap() = Some(set.clone());
            Ok(())
        }
        fn get_predicates(&self, _name: &str) -> Result<StatePredicateSet, DomainError> {
            Err(DomainError::NotFound("none".to_string()))
        }
        fn list_predicate_names(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec!["scene".to_string()])
        }
        fn delete_predicates(&self, _name: &str) -> Result<(), DomainError> {
            Err(DomainError::NotFound("none".to_string()))
        }
    }

    fn service() -> StatePredicateService {
        StatePredicateService::new(Arc::new(FakeRepository {
            stored: Mutex::new(None),
            saved: Mutex::new(Vec::new()),
        }))
    }

    fn entry(id: &str, tags: &[&str]) -> PredicateEntry {
        PredicateEntry {
            id: id.to_string(),
            content: format!("content of {id}"),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn save_reports_every_problem_at_once() {
        let mut guard = entry("guard", &["guard"]);
        guard.effects = vec![PredicateEffect {
            kind: PredicateEffectKind::Inhibit,
            tags: vec!["ghost".to_string()],
        }];
        let mut blank = entry("blank", &[]);
        blank.content = String::new();
        let set = StatePredicateSet {
            groups: vec![PredicateGroup {
                id: "tone".to_string(),
                label: None,
                entries: vec![guard, blank],
            }],
            constants: Vec::new(),
        };

        let error = service()
            .save("scene", &set, None)
            .expect_err("a broken set must be refused");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(message.starts_with("state.predicate_invalid_set"));
                assert!(message.contains("state.predicate_effect_tag_unknown"));
                assert!(message.contains("state.predicate_content_required"));
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_name_with_a_path_separator_is_refused_before_anything_is_written() {
        let set = StatePredicateSet {
            groups: vec![PredicateGroup {
                id: "tone".to_string(),
                label: None,
                entries: vec![entry("only", &[])],
            }],
            constants: Vec::new(),
        };

        let error = service()
            .save("../escape", &set, None)
            .expect_err("a path separator must not reach storage");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(message.contains("state_predicate.invalid_name"));
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }
}
