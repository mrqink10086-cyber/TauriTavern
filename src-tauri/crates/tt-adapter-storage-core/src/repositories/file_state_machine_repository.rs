use std::path::{Path, PathBuf};

use crate::file_system::persist_json_file_blocking;
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::state_machine::StateMachineSpec;
use tt_ports::repositories::state_machine_repository::StateMachineRepository;

/// File-based implementation of the `StateMachineRepository`.
///
/// Specs live at `user_dir/state-machines/<name>.json`, next to the state
/// declarations they read fields from.
pub struct FileStateMachineRepository {
    machines_dir: PathBuf,
}

impl FileStateMachineRepository {
    pub fn new(machines_dir: PathBuf) -> Self {
        Self { machines_dir }
    }

    fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.machines_dir.exists() {
            std::fs::create_dir_all(&self.machines_dir).map_err(|error| {
                tracing::error!("Failed to create state machines directory: {}", error);
                DomainError::InternalError(format!(
                    "Failed to create state machines directory: {error}"
                ))
            })?;
        }
        Ok(())
    }

    fn get_machine_path(&self, name: &str) -> Result<PathBuf, DomainError> {
        let filename = sanitize_filename(&format!("{name}.json"));
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "State machine name is invalid for filesystem storage".to_string(),
            ));
        }
        Ok(self.machines_dir.join(filename))
    }

    fn read_machine_file(path: &Path) -> Result<StateMachineSpec, DomainError> {
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

impl StateMachineRepository for FileStateMachineRepository {
    fn save_machine(&self, name: &str, spec: &StateMachineSpec) -> Result<(), DomainError> {
        self.ensure_directory_exists()?;
        let path = self.get_machine_path(name)?;
        persist_json_file_blocking(&path, spec)
    }

    fn get_machine(&self, name: &str) -> Result<StateMachineSpec, DomainError> {
        let path = self.get_machine_path(name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State machine not found: {name}"
            )));
        }
        Self::read_machine_file(&path)
    }

    fn list_machine_names(&self) -> Result<Vec<String>, DomainError> {
        if !self.machines_dir.exists() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(&self.machines_dir).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read state machines directory: {error}"
            ))
        })?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read state machines directory entry: {error}"
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

    fn delete_machine(&self, name: &str) -> Result<(), DomainError> {
        let path = self.get_machine_path(name)?;
        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State machine not found: {name}"
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
    use tt_domain::models::state_machine::StateSpec;

    fn create_temp_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-state-machines-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn machine(ids: &[&str]) -> StateMachineSpec {
        StateMachineSpec {
            initial: ids.first().map(|id| id.to_string()).into_iter().collect(),
            states: ids
                .iter()
                .map(|id| StateSpec {
                    id: id.to_string(),
                    label: None,
                    terminal: false,
                })
                .collect(),
            transitions: Vec::new(),
            hooks: None,
        }
    }

    #[test]
    fn save_then_get_round_trips() {
        let root = create_temp_dir();
        let repo = FileStateMachineRepository::new(root.join("state-machines"));
        let spec = machine(&["a", "b"]);

        repo.save_machine("demo", &spec).expect("save must succeed");
        let loaded = repo.get_machine("demo").expect("get must succeed");

        assert_eq!(loaded, spec);
        assert!(root.join("state-machines/demo.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_returns_sorted_names_without_extension() {
        let root = create_temp_dir();
        let repo = FileStateMachineRepository::new(root.join("state-machines"));
        let spec = machine(&["a"]);

        assert!(repo.list_machine_names().expect("list on missing dir").is_empty());
        repo.save_machine("b", &spec).expect("save must succeed");
        repo.save_machine("a", &spec).expect("save must succeed");

        assert_eq!(
            repo.list_machine_names().expect("list must succeed"),
            vec!["a".to_string(), "b".to_string()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_the_machine_and_get_reports_not_found() {
        let root = create_temp_dir();
        let repo = FileStateMachineRepository::new(root.join("state-machines"));
        let spec = machine(&["a"]);

        repo.save_machine("demo", &spec).expect("save must succeed");
        repo.delete_machine("demo").expect("delete must succeed");
        repo.delete_machine("demo")
            .expect_err("deleting twice must fail");

        assert!(repo.list_machine_names().expect("list").is_empty());
        assert!(matches!(
            repo.get_machine("demo"),
            Err(DomainError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
