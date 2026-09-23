//! Orchestration for the state machine layer.
//!
//! The domain decides *what* a spec means and *which* transitions fire; this
//! service owns everything the domain must not touch: name validation, the
//! declaration-aware field check, storage, and running the spec's script hooks.
//!
//! Hooks are the extension point. A spec may name a JS module that sees every
//! transition the evaluator picked and may refuse any of them, which is how
//! rules too specific for a generic comparator get expressed. A refused
//! transition is reported as skipped with `denied_by_hook` — never applied and
//! never silently dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tt_domain::errors::DomainError;
use tt_domain::models::state::StateDeclaration;
use tt_domain::models::state_machine::{
    ComparatorRegistry, MachineError, MachineEvaluation, MachineState, StateMachineSpec,
    declaration_errors, validate_spec,
};
use tt_ports::repositories::state_machine_repository::StateMachineRepository;
use tt_ports::skill_script::SkillScriptEngine;

use crate::errors::ApplicationError;
use crate::services::state_runtime::{evaluate_machine, format_errors};

pub struct StateMachineService {
    repository: Arc<dyn StateMachineRepository>,
    script_engine: Option<Arc<dyn SkillScriptEngine>>,
    comparators: ComparatorRegistry,
}

impl StateMachineService {
    pub fn new(
        repository: Arc<dyn StateMachineRepository>,
        script_engine: Option<Arc<dyn SkillScriptEngine>>,
    ) -> Self {
        Self {
            repository,
            script_engine,
            comparators: ComparatorRegistry::default(),
        }
    }

    /// Extend the condition vocabulary. Registering a name that already exists
    /// replaces it — this is the supported way to add domain-specific
    /// predicates without changing the domain module.
    pub fn register_comparator(
        &mut self,
        name: impl Into<String>,
        comparator: fn(&tt_domain::models::state_machine::ConditionContext, &tt_domain::models::state_machine::ConditionSpec) -> bool,
    ) {
        self.comparators.register(name, comparator);
    }

    fn validate_name(name: &str) -> Result<String, ApplicationError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApplicationError::ValidationError(
                "state_machine.invalid_name: name must not be empty".to_string(),
            ));
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            return Err(ApplicationError::ValidationError(format!(
                "state_machine.invalid_name: name `{trimmed}` must not contain a path separator"
            )));
        }
        Ok(trimmed.to_string())
    }

    /// Validate a spec against a bound declaration, if one exists.
    ///
    /// The domain checks structure and vocabulary; the field check needs the
    /// declaration, because a declared field may be a pattern such as
    /// `角色/*/着装` rather than a literal key.
    pub fn validate(
        &self,
        spec: &StateMachineSpec,
        declaration: Option<&StateDeclaration>,
    ) -> Vec<MachineError> {
        let mut errors = validate_spec(spec, &BTreeSet::new(), &self.comparators);
        if let Some(declaration) = declaration.filter(|declaration| !declaration.is_empty()) {
            errors.extend(declaration_errors(spec, declaration));
        }
        errors
    }

    /// Save a spec, refusing it when the definition does not hold together.
    pub fn save(
        &self,
        name: &str,
        spec: &StateMachineSpec,
        declaration: Option<&StateDeclaration>,
    ) -> Result<(), ApplicationError> {
        let name = Self::validate_name(name)?;
        let errors = self.validate(spec, declaration);
        if !errors.is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "state_machine.invalid_spec:\n{}",
                format_errors(&errors)
            )));
        }
        self.repository
            .save_machine(&name, spec)
            .map_err(|error| {
                tracing::error!("Failed to save state machine {name}: {error}");
                error.into()
            })
    }

    pub fn get(&self, name: &str) -> Result<StateMachineSpec, ApplicationError> {
        self.repository.get_machine(name).map_err(|error| match error {
            DomainError::NotFound(_) => {
                ApplicationError::NotFound(format!("State machine not found: {name}"))
            }
            other => other.into(),
        })
    }

    pub fn list_names(&self) -> Result<Vec<String>, ApplicationError> {
        self.repository.list_machine_names().map_err(Into::into)
    }

    pub fn delete(&self, name: &str) -> Result<(), ApplicationError> {
        self.repository.delete_machine(name).map_err(|error| match error {
            DomainError::NotFound(_) => {
                ApplicationError::NotFound(format!("State machine not found: {name}"))
            }
            other => other.into(),
        })
    }

    /// Run the machine once, letting the spec's hook refuse what it wants.
    ///
    /// `forced` carries transition indices a caller asked for directly (the
    /// model's own move request); they fire without conditions, which is what
    /// makes "the model asked for it" different from "the rules allowed it".
    /// Everything else still has to satisfy its conditions.
    pub async fn evaluate(
        &self,
        spec: &StateMachineSpec,
        state: &MachineState,
        fields: &BTreeMap<String, Vec<String>>,
        forced: &BTreeSet<usize>,
    ) -> Result<MachineEvaluation, ApplicationError> {
        evaluate_machine(
            self.script_engine.as_deref(),
            &self.comparators,
            spec,
            state,
            fields,
            forced,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use tt_domain::models::state::{DeclaredStateField, StateDeclaration};
    use tt_domain::models::state_access::StateFieldAccess;
    use tt_domain::models::state_key::StateKeyPattern;
    use tt_domain::models::state_machine::{
        ActionSpec, ConditionSpec, StateSpec, TransitionSpec,
    };

    fn declaration(patterns: &[&str]) -> StateDeclaration {
        StateDeclaration {
            fields: patterns
                .iter()
                .map(|pattern| DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern).expect("pattern must parse"),
                    label: pattern.to_string(),
                    access: StateFieldAccess::DECLARED,
                    initial: Vec::new(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn machine(transitions: Vec<TransitionSpec>) -> StateMachineSpec {
        StateMachineSpec {
            initial: vec!["start".to_string()],
            states: vec![
                StateSpec {
                    id: "start".to_string(),
                    label: None,
                    terminal: false,
                },
                StateSpec {
                    id: "next".to_string(),
                    label: None,
                    terminal: false,
                },
            ],
            transitions,
            hooks: None,
        }
    }

    struct FakeRepository {
        saved: Mutex<Vec<String>>,
        stored: Mutex<Option<StateMachineSpec>>,
    }

    impl StateMachineRepository for FakeRepository {
        fn save_machine(&self, name: &str, spec: &StateMachineSpec) -> Result<(), DomainError> {
            self.saved.lock().unwrap().push(name.to_string());
            *self.stored.lock().unwrap() = Some(spec.clone());
            Ok(())
        }
        fn get_machine(&self, _name: &str) -> Result<StateMachineSpec, DomainError> {
            Err(DomainError::NotFound("none".to_string()))
        }
        fn list_machine_names(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec!["demo".to_string()])
        }
        fn delete_machine(&self, _name: &str) -> Result<(), DomainError> {
            Err(DomainError::NotFound("none".to_string()))
        }
    }

    fn service() -> StateMachineService {
        StateMachineService::new(Arc::new(FakeRepository {
            saved: Mutex::new(Vec::new()),
            stored: Mutex::new(None),
        }), None)
    }

    #[test]
    fn save_reports_every_spec_problem_at_once() {
        let service = service();
        let spec = machine(vec![TransitionSpec {
            conditions: vec![ConditionSpec {
                source: "field".to_string(),
                field: Some("ghost".to_string()),
                op: "eq".to_string(),
                value: Some("x".to_string()),
                values: Vec::new(),
                compose: None,
            }],
            actions: vec![ActionSpec {
                kind: "explode".to_string(),
                target: Some("known".to_string()),
                values: Vec::new(),
            }],
            ..TransitionSpec {
                id: None,
                from: vec!["start".to_string()],
                to: vec!["nowhere".to_string()],
                conditions: Vec::new(),
                actions: Vec::new(),
                priority: 0,
            }
        }]);

        let error = service
            .save("demo", &spec, Some(&declaration(&["known/*"])))
            .expect_err("a broken spec must be refused");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(message.starts_with("state_machine.invalid_spec"));
                assert!(message.contains("state_machine.undefined_state"));
                assert!(message.contains("state_machine.undeclared_field"));
                assert!(message.contains("state_machine.action_kind_unknown"));
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_pattern_covers_the_keys_it_matches() {
        let service = service();
        let spec = machine(vec![TransitionSpec {
            conditions: vec![ConditionSpec {
                source: "field".to_string(),
                field: Some("known/deep".to_string()),
                op: "eq".to_string(),
                value: Some("x".to_string()),
                values: Vec::new(),
                compose: None,
            }],
            ..TransitionSpec {
                id: None,
                from: vec!["start".to_string()],
                to: vec!["next".to_string()],
                conditions: Vec::new(),
                actions: Vec::new(),
                priority: 0,
            }
        }]);

        assert!(
            service.validate(&spec, Some(&declaration(&["known/*"]))).is_empty(),
            "a key matched by a declared pattern is allowed"
        );
    }

    #[tokio::test]
    async fn a_forced_transition_fires_without_conditions() {
        let service = service();
        let spec = machine(vec![TransitionSpec {
            conditions: vec![ConditionSpec {
                source: "field".to_string(),
                field: Some("mood".to_string()),
                op: "eq".to_string(),
                value: Some("never".to_string()),
                values: Vec::new(),
                compose: None,
            }],
            ..TransitionSpec {
                id: None,
                from: vec!["start".to_string()],
                to: vec!["next".to_string()],
                conditions: Vec::new(),
                actions: Vec::new(),
                priority: 0,
            }
        }]);
        let state = MachineState {
            active: ["start".to_string()].into_iter().collect(),
        };
        let forced: BTreeSet<usize> = [0].into_iter().collect();

        let outcome = service
            .evaluate(&spec, &state, &BTreeMap::new(), &forced)
            .await
            .expect("evaluate");

        assert!(outcome.active.contains("next"));
        assert!(!outcome.active.contains("start"));
        assert_eq!(outcome.applied.len(), 1);
    }

    #[tokio::test]
    async fn an_unforced_transition_needs_its_conditions() {
        let service = service();
        let spec = machine(vec![TransitionSpec {
            conditions: vec![ConditionSpec {
                source: "field".to_string(),
                field: Some("mood".to_string()),
                op: "eq".to_string(),
                value: Some("bad".to_string()),
                values: Vec::new(),
                compose: None,
            }],
            ..TransitionSpec {
                id: None,
                from: vec!["start".to_string()],
                to: vec!["next".to_string()],
                conditions: Vec::new(),
                actions: Vec::new(),
                priority: 0,
            }
        }]);
        let state = MachineState {
            active: ["start".to_string()].into_iter().collect(),
        };

        let quiet = service
            .evaluate(&spec, &state, &BTreeMap::new(), &BTreeSet::new())
            .await
            .expect("evaluate");
        assert!(quiet.active.contains("start"));
        assert!(quiet.applied.is_empty());
    }

    #[tokio::test]
    async fn a_hook_without_an_engine_is_reported_not_skipped() {
        let mut spec = machine(vec![TransitionSpec {
            ..TransitionSpec {
                id: None,
                from: vec!["start".to_string()],
                to: vec!["next".to_string()],
                conditions: Vec::new(),
                actions: Vec::new(),
                priority: 0,
            }
        }]);
        spec.hooks = Some(tt_domain::models::state_machine::HookSpec {
            script: "export default () => ({ allow: true });".to_string(),
            entry: None,
        });
        let service = service();
        let state = MachineState {
            active: ["start".to_string()].into_iter().collect(),
        };

        let error = service
            .evaluate(&spec, &state, &BTreeMap::new(), &BTreeSet::new())
            .await
            .expect_err("a declared hook must not be silently ignored");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(message.contains("state_machine.hook_unavailable"));
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }
}
