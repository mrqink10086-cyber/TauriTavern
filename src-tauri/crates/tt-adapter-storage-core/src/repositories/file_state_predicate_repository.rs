use std::path::{Path, PathBuf};

use crate::file_system::persist_json_file_blocking;
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::state_predicate::StatePredicateSet;
use tt_ports::repositories::state_predicate_repository::StatePredicateRepository;

/// File-based implementation of the `StatePredicateRepository`.
///
/// Sets live at `user_dir/state-predicates/<name>.json`, next to the state
/// declarations whose fields their conditions read.
pub struct FileStatePredicateRepository {
    predicates_dir: PathBuf,
}

impl FileStatePredicateRepository {
    pub fn new(predicates_dir: PathBuf) -> Self {
        Self { predicates_dir }
    }

    fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.predicates_dir.exists() {
            std::fs::create_dir_all(&self.predicates_dir).map_err(|error| {
                tracing::error!("Failed to create state predicates directory: {}", error);
                DomainError::InternalError(format!(
                    "Failed to create state predicates directory: {error}"
                ))
            })?;
        }
        Ok(())
    }

    fn get_predicates_path(&self, name: &str) -> Result<PathBuf, DomainError> {
        let filename = sanitize_filename(&format!("{name}.json"));
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "State predicate name is invalid for filesystem storage".to_string(),
            ));
        }
        Ok(self.predicates_dir.join(filename))
    }

    fn read_predicates_file(path: &Path) -> Result<StatePredicateSet, DomainError> {
        let bytes = std::fs::read(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("File not found: {}", path.display()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read {}: {error}",
                    path.display()
                ))
            }
        })?;
        let contents = String::from_utf8(bytes).map_err(|error| {
            DomainError::InvalidData(format!("Invalid UTF-8 in {}: {error}", path.display()))
        })?;
        serde_json::from_str(&contents).map_err(|error| {
            DomainError::InvalidData(format!("Invalid JSON in {}: {error}", path.display()))
        })
    }
}

impl StatePredicateRepository for FileStatePredicateRepository {
    fn save_predicates(&self, name: &str, set: &StatePredicateSet) -> Result<(), DomainError> {
        self.ensure_directory_exists()?;
        let path = self.get_predicates_path(name)?;
        persist_json_file_blocking(&path, set)
    }

    fn get_predicates(&self, name: &str) -> Result<StatePredicateSet, DomainError> {
        let path = self.get_predicates_path(name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State predicates not found: {name}"
            )));
        }
        Self::read_predicates_file(&path)
    }

    fn list_predicate_names(&self) -> Result<Vec<String>, DomainError> {
        if !self.predicates_dir.exists() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(&self.predicates_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read state predicates directory: {error}"
            ))
        })?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read state predicates directory entry: {error}"
                ))
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    fn delete_predicates(&self, name: &str) -> Result<(), DomainError> {
        let path = self.get_predicates_path(name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State predicates not found: {name}"
            )));
        }
        std::fs::remove_file(&path).map_err(|error| {
            DomainError::InternalError(format!("Failed to delete file: {error}"))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tt_domain::models::state_predicate::{PredicateEntry, PredicateGroup};

    fn create_temp_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-state-predicates-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn set(ids: &[&str]) -> StatePredicateSet {
        StatePredicateSet {
            groups: vec![PredicateGroup {
                id: "tone".to_string(),
                label: None,
                entries: ids
                    .iter()
                    .map(|id| PredicateEntry {
                        id: id.to_string(),
                        content: format!("content of {id}"),
                        ..Default::default()
                    })
                    .collect(),
            }],
            constants: Vec::new(),
        }
    }

    #[test]
    fn save_then_get_round_trips() {
        let root = create_temp_dir();
        let repo = FileStatePredicateRepository::new(root.join("state-predicates"));
        let saved = set(&["close", "distant"]);

        repo.save_predicates("scene", &saved).expect("save must succeed");
        let loaded = repo.get_predicates("scene").expect("get must succeed");

        assert_eq!(loaded, saved);
        assert!(root.join("state-predicates/scene.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_returns_sorted_names_without_extension() {
        let root = create_temp_dir();
        let repo = FileStatePredicateRepository::new(root.join("state-predicates"));
        let saved = set(&["only"]);

        assert!(repo.list_predicate_names().expect("list on missing dir").is_empty());
        repo.save_predicates("b", &saved).expect("save must succeed");
        repo.save_predicates("a", &saved).expect("save must succeed");

        assert_eq!(
            repo.list_predicate_names().expect("list must succeed"),
            vec!["a".to_string(), "b".to_string()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_the_set_and_get_reports_not_found() {
        let root = create_temp_dir();
        let repo = FileStatePredicateRepository::new(root.join("state-predicates"));
        let saved = set(&["only"]);

        repo.save_predicates("scene", &saved).expect("save must succeed");
        repo.delete_predicates("scene").expect("delete must succeed");
        repo.delete_predicates("scene")
            .expect_err("deleting twice must fail");

        assert!(repo.list_predicate_names().expect("list").is_empty());
        assert!(matches!(
            repo.get_predicates("scene"),
            Err(DomainError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
