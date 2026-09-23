use std::collections::BTreeSet;
use std::sync::Arc;

use crate::errors::ApplicationError;
use tt_domain::errors::DomainError;
use tt_domain::models::state::{CostUnit, StateDeclaration};
use tt_domain::models::state_machine::{
    ComparatorRegistry, MachineError, declaration_errors, validate_spec,
};
use tt_domain::models::state_panel::validate_panel_config;
use tt_domain::models::state_predicate::validate_predicate_set;
use tt_ports::repositories::state_declaration_repository::StateDeclarationRepository;

/// One line per machine problem, so a caller can fix the whole document at once.
fn format_machine_errors(errors: &[MachineError]) -> String {
    errors
        .iter()
        .map(|error| match &error.target {
            Some(target) => format!("- {} ({target}): {}", error.code, error.message),
            None => format!("- {}: {}", error.code, error.message),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Service for managing state declarations
pub struct StateDeclarationService {
    state_declaration_repository: Arc<dyn StateDeclarationRepository>,
}

impl StateDeclarationService {
    /// Create a new StateDeclarationService
    pub fn new(state_declaration_repository: Arc<dyn StateDeclarationRepository>) -> Self {
        Self { state_declaration_repository }
    }

    fn validate_name(name: &str) -> Result<String, ApplicationError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApplicationError::ValidationError(
                "state_declaration.invalid_name: name must not be empty".to_string(),
            ));
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            return Err(ApplicationError::ValidationError(format!(
                "state_declaration.invalid_name: name `{trimmed}` must not contain a path separator"
            )));
        }

        Ok(trimmed.to_string())
    }

    /// Save a state declaration
    pub fn save(&self, name: &str, declaration: &StateDeclaration) -> Result<(), ApplicationError> {
        tracing::info!("Saving state declaration: {}", name);

        let name = Self::validate_name(name)?;

        // Overlapping declared fields make the key space ambiguous, so the
        // declaration is refused at save time instead of guessed at resolve time.
        let overlaps = declaration.overlaps();
        if !overlaps.is_empty() {
            let pairs = overlaps
                .iter()
                .map(|overlap| format!("`{}` vs `{}`", overlap.first, overlap.second))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApplicationError::ValidationError(format!(
                "state_declaration.overlapping_patterns: {pairs}"
            )));
        }

        // An initial value is what a chat starts with, so a row that cannot be
        // seeded is refused here rather than skipped when the first floor runs.
        //
        // A scene counting in tokens gets the shape rules here and its ceiling
        // checked by the caller that holds the vocabulary (the save command
        // validates against it): comparing a token ceiling to a character count
        // would refuse values the scene allows and allow ones it does not.
        let initial_errors = match declaration.limits.unit {
            CostUnit::Chars => declaration.initial_value_errors(),
            CostUnit::Tokens => declaration.initial_value_shape_errors(),
        };
        if !initial_errors.is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "state_declaration.invalid_initial: {}",
                initial_errors.join("; ")
            )));
        }

        // Panels claim keys the same way fields do, so the same rule applies:
        // two panels that can claim one key would make the key's placement
        // depend on evaluation order instead of on the configuration.
        let panel_overlaps = declaration.panels.overlaps();
        if !panel_overlaps.is_empty() {
            let pairs = panel_overlaps
                .iter()
                .map(|overlap| format!("`{}` vs `{}`", overlap.first, overlap.second))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApplicationError::ValidationError(format!(
                "state_declaration.overlapping_panels: {pairs}"
            )));
        }

        // A panel configuration that cannot be honoured is refused here rather
        // than at render time: an unreachable picture or an unregistered
        // condition op would otherwise show up as a panel that quietly never
        // shows the picture it was told to show.
        let declared_keys = declaration
            .fields
            .iter()
            .map(|field| field.pattern.clone())
            .collect::<Vec<_>>();
        let panel_errors = validate_panel_config(
            &declaration.panels,
            &ComparatorRegistry::default(),
            &declared_keys,
        );
        if !panel_errors.is_empty() {
            let detail = panel_errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApplicationError::ValidationError(format!(
                "state_declaration.invalid_panel: {detail}"
            )));
        }

        // A scene's rules travel inside its declaration, so they are checked
        // here as well as when they are stored on their own: a machine or a
        // predicate set that cannot be honoured must not be saved inside a
        // document that looks saved.
        let comparators = ComparatorRegistry::default();
        if let Some(machine) = &declaration.machine {
            let mut errors = validate_spec(machine, &BTreeSet::new(), &comparators);
            errors.extend(declaration_errors(machine, declaration));
            if !errors.is_empty() {
                return Err(ApplicationError::ValidationError(format!(
                    "state_declaration.invalid_machine:\n{}",
                    format_machine_errors(&errors)
                )));
            }
        }
        if let Some(predicates) = &declaration.predicates {
            let errors = validate_predicate_set(predicates, Some(declaration), &comparators);
            if !errors.is_empty() {
                let detail = errors
                    .iter()
                    .map(|error| match &error.target {
                        Some(target) => format!("{} ({target}): {}", error.code, error.message),
                        None => format!("{}: {}", error.code, error.message),
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(ApplicationError::ValidationError(format!(
                    "state_declaration.invalid_predicates: {detail}"
                )));
            }
        }

        // Save the declaration
        self.state_declaration_repository
            .save_declaration(&name, declaration)
            .map_err(|e| {
                tracing::error!("Failed to save state declaration {}: {}", name, e);
                e.into()
            })
    }

    /// Get a state declaration
    pub fn get(&self, name: &str) -> Result<StateDeclaration, ApplicationError> {
        tracing::info!("Getting state declaration: {}", name);

        self.state_declaration_repository
            .get_declaration(name)
            .map_err(|e| {
                tracing::error!("Failed to get state declaration {}: {}", name, e);
                // Convert NotFound to a more specific error
                match e {
                    DomainError::NotFound(_) => {
                        ApplicationError::NotFound(format!("State declaration not found: {}", name))
                    }
                    _ => e.into(),
                }
            })
    }

    /// List state declaration names
    pub fn list_names(&self) -> Result<Vec<String>, ApplicationError> {
        self.state_declaration_repository
            .list_declaration_names()
            .map_err(|e| {
                tracing::error!("Failed to list state declarations: {}", e);
                e.into()
            })
    }

    /// Delete a state declaration
    pub fn delete(&self, name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Deleting state declaration: {}", name);

        self.state_declaration_repository
            .delete_declaration(name)
            .map_err(|e| {
                tracing::error!("Failed to delete state declaration {}: {}", name, e);
                // Convert NotFound to a more specific error
                match e {
                    DomainError::NotFound(_) => {
                        ApplicationError::NotFound(format!("State declaration not found: {}", name))
                    }
                    _ => e.into(),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tt_domain::errors::DomainError;
    use tt_domain::models::state::{DeclaredStateField, StateDeclaration};
    use tt_domain::models::state_access::StateFieldAccess;
    use tt_domain::models::state_key::StateKeyPattern;
    use tt_domain::models::state_machine::{ConditionSpec, SOURCE_FIELD};
    use tt_domain::models::state_panel::{
        StateImageCandidate, StateImageSet, StatePanelConfig, StatePanelRail, StatePanelSpec,
    };
    use tt_ports::repositories::state_declaration_repository::StateDeclarationRepository;

    fn declaration(entries: &[(&str, &str)]) -> StateDeclaration {
        StateDeclaration {
            fields: entries
                .iter()
                .map(|(pattern, label)| DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern)
                        .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
                    label: label.to_string(),
                    access: StateFieldAccess::DECLARED,
                    initial: Vec::new(),
                })
                .collect(),
            panels: Default::default(),
            machine: None,
            predicates: None,
            limits: Default::default(),
        }
    }

    struct FakeRepository {
        saved: Mutex<Vec<String>>,
    }

    impl StateDeclarationRepository for FakeRepository {
        fn save_declaration(
            &self,
            name: &str,
            _declaration: &StateDeclaration,
        ) -> Result<(), DomainError> {
            self.saved.lock().unwrap().push(name.to_string());
            Ok(())
        }

        fn get_declaration(&self, name: &str) -> Result<StateDeclaration, DomainError> {
            Err(DomainError::NotFound(name.to_string()))
        }

        fn list_declaration_names(&self) -> Result<Vec<String>, DomainError> {
            Ok(Vec::new())
        }

        fn delete_declaration(&self, name: &str) -> Result<(), DomainError> {
            Err(DomainError::NotFound(name.to_string()))
        }
    }

    fn service() -> (StateDeclarationService, Arc<FakeRepository>) {
        let repository = Arc::new(FakeRepository { saved: Mutex::new(Vec::new()) });
        (StateDeclarationService::new(repository.clone()), repository)
    }

    #[test]
    fn save_accepts_a_valid_declaration() {
        let (service, repository) = service();
        let declaration = declaration(&[("环境/日期", "DATE"), ("环境/时间", "TIME")]);

        service.save("demo", &declaration).expect("valid declaration must be stored");

        assert_eq!(*repository.saved.lock().unwrap(), vec!["demo"]);
    }

    #[test]
    fn save_rejects_overlapping_patterns() {
        let (service, _repository) = service();
        let declaration = declaration(&[("环境/*/来源", "SOURCE"), ("环境/日期/来源", "DATE")]);

        let error = service.save("demo", &declaration).expect_err("overlaps must be refused");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(
                    message.starts_with("state_declaration.overlapping_patterns"),
                    "message must carry the overlap code: {message}"
                );
                assert!(message.contains("环境/*/来源"));
                assert!(message.contains("环境/日期/来源"));
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    fn panel(title: &str, match_pattern: &str) -> StatePanelSpec {
        StatePanelSpec {
            title: title.to_string(),
            rail: StatePanelRail::Left,
            match_pattern: StateKeyPattern::parse(match_pattern).expect("pattern must parse"),
            background: None,
            fields: Vec::new(),
            markup: None,
            prose: None,
        }
    }

    #[test]
    fn save_rejects_panels_that_claim_the_same_key() {
        let (service, _repository) = service();
        let mut declaration = declaration(&[("环境/日期", "DATE")]);
        declaration.panels = StatePanelConfig {
            panels: vec![panel("环境", "环境/**"), panel("日期", "环境/*")],
            css: Default::default(),
            scripts: Default::default(),
        };

        let error = service
            .save("demo", &declaration)
            .expect_err("two panels claiming one key must be refused");

        match error {
            ApplicationError::ValidationError(message) => assert!(
                message.starts_with("state_declaration.overlapping_panels"),
                "message must carry the overlap code: {message}"
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn save_rejects_a_panel_whose_picture_could_never_be_chosen() {
        let (service, _repository) = service();
        let mut declaration = declaration(&[("环境/日期", "DATE")]);
        let mut fallback_first = panel("环境", "环境/**");
        fallback_first.background = Some(StateImageSet {
            condition_script: None,
            candidates: vec![
                StateImageCandidate {
                    source: "/backgrounds/default.png".to_string(),
                    when: None,
                },
                StateImageCandidate {
                    source: "/backgrounds/night.png".to_string(),
                    when: Some(ConditionSpec {
                        source: SOURCE_FIELD.to_string(),
                        field: Some("环境/时间".to_string()),
                        op: "eq".to_string(),
                        value: Some("夜晚".to_string()),
                        values: Vec::new(),
                        compose: None,
                    }),
                },
            ],
            ..Default::default()
        });
        declaration.panels = StatePanelConfig {
            panels: vec![fallback_first],
            css: Default::default(),
            scripts: Default::default(),
        };

        let error = service
            .save("demo", &declaration)
            .expect_err("an unreachable picture must be refused at save time");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(
                    message.starts_with("state_declaration.invalid_panel"),
                    "message must carry the invalid_panel code: {message}"
                );
                assert!(
                    message.contains("/backgrounds/default.png"),
                    "the message must name the fallback that made the others unreachable: {message}"
                );
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn save_rejects_a_name_with_a_path_separator() {
        let (service, _repository) = service();
        let declaration = declaration(&[("环境/日期", "DATE")]);

        let error = service
            .save("环境/日期", &declaration)
            .expect_err("a name with a path separator must be refused");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(
                    message.starts_with("state_declaration.invalid_name"),
                    "message must carry the invalid_name code: {message}"
                );
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn save_rejects_an_empty_name() {
        let (service, _repository) = service();
        let declaration = declaration(&[("环境/日期", "DATE")]);

        let error = service
            .save("   ", &declaration)
            .expect_err("an empty name must be refused");

        match error {
            ApplicationError::ValidationError(message) => {
                assert!(
                    message.starts_with("state_declaration.invalid_name"),
                    "message must carry the invalid_name code: {message}"
                );
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }
}
