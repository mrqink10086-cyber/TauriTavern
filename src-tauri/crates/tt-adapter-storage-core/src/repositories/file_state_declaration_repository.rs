use std::path::{Path, PathBuf};

use crate::file_system::persist_json_file_blocking;
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::state::StateDeclaration;
use tt_ports::repositories::state_declaration_repository::StateDeclarationRepository;

/// File-based implementation of the StateDeclarationRepository
pub struct FileStateDeclarationRepository {
    /// The directory where state declarations are stored
    declarations_dir: PathBuf,
}

impl FileStateDeclarationRepository {
    /// Create a new FileStateDeclarationRepository
    pub fn new(declarations_dir: PathBuf) -> Self {
        Self { declarations_dir }
    }

    /// Ensure the declarations directory exists
    fn ensure_directory_exists(&self) -> Result<(), DomainError> {
        if !self.declarations_dir.exists() {
            std::fs::create_dir_all(&self.declarations_dir).map_err(|e| {
                tracing::error!("Failed to create state declarations directory: {}", e);
                DomainError::InternalError(format!(
                    "Failed to create state declarations directory: {}",
                    e
                ))
            })?;
        }

        Ok(())
    }

    /// Get the path to a state declaration file
    fn get_declaration_path(&self, name: &str) -> Result<PathBuf, DomainError> {
        let filename = sanitize_filename(&format!("{name}.json"));
        if filename.is_empty() {
            return Err(DomainError::InvalidData(
                "State declaration name is invalid for filesystem storage".to_string(),
            ));
        }

        Ok(self.declarations_dir.join(filename))
    }

    /// Read a state declaration file
    fn read_declaration_file(path: &Path) -> Result<StateDeclaration, DomainError> {
        tracing::debug!("Reading state declaration file: {:?}", path);

        let read_path = path.to_owned();
        let bytes = std::fs::read(&read_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("File not found: {}", read_path.display()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read {}: {error}",
                    read_path.display()
                ))
            }
        })?;
        let contents = String::from_utf8(bytes).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid UTF-8 in {}: {error}",
                read_path.display()
            ))
        })?;

        serde_json::from_str(&contents).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid JSON in {}: {error}",
                read_path.display()
            ))
        })
    }
}

impl StateDeclarationRepository for FileStateDeclarationRepository {
    fn save_declaration(
        &self,
        name: &str,
        declaration: &StateDeclaration,
    ) -> Result<(), DomainError> {
        tracing::debug!("Saving state declaration: {}", name);

        // Ensure the directory exists
        self.ensure_directory_exists()?;

        // Get the path to the state declaration file
        let path = self.get_declaration_path(name)?;

        // Write the declaration to the file
        persist_json_file_blocking(&path, declaration)
    }

    fn get_declaration(&self, name: &str) -> Result<StateDeclaration, DomainError> {
        tracing::debug!("Getting state declaration: {}", name);

        let path = self.get_declaration_path(name)?;

        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State declaration not found: {}",
                name
            )));
        }

        Self::read_declaration_file(&path)
    }

    fn list_declaration_names(&self) -> Result<Vec<String>, DomainError> {
        if !self.declarations_dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        let entries = std::fs::read_dir(&self.declarations_dir).map_err(|e| {
            tracing::error!("Failed to read state declarations directory: {}", e);
            DomainError::InternalError(format!(
                "Failed to read state declarations directory: {}",
                e
            ))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                DomainError::InternalError(format!(
                    "Failed to read state declarations directory entry: {}",
                    e
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

    fn delete_declaration(&self, name: &str) -> Result<(), DomainError> {
        tracing::debug!("Deleting state declaration: {}", name);

        let path = self.get_declaration_path(name)?;

        if !path.exists() {
            return Err(DomainError::NotFound(format!(
                "State declaration not found: {}",
                name
            )));
        }

        std::fs::remove_file(&path).map_err(|e| {
            tracing::error!("Failed to delete state declaration {}: {}", path.display(), e);
            DomainError::InternalError(format!("Failed to delete file: {}", e))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use tt_domain::models::state::{DeclaredStateField, StateDeclaration};
    use tt_domain::models::state_access::StateFieldAccess;
    use tt_domain::models::state_key::StateKeyPattern;

    fn create_temp_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-state-declarations-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

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
            ..Default::default()
        }
    }

    #[test]
    fn save_then_get_round_trips() {
        let root = create_temp_dir();
        let repo = FileStateDeclarationRepository::new(root.join("state-declarations"));
        let declaration = declaration(&[("环境/日期", "DATE"), ("环境/*/来源", "SOURCE")]);

        repo.save_declaration("demo", &declaration)
            .expect("save must succeed");
        let loaded = repo.get_declaration("demo").expect("get must succeed");

        assert_eq!(loaded, declaration);
        assert!(root.join("state-declarations/demo.json").is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_returns_sorted_names_without_extension() {
        let root = create_temp_dir();
        let repo = FileStateDeclarationRepository::new(root.join("state-declarations"));
        let declaration = declaration(&[("环境/日期", "DATE")]);

        assert!(repo.list_declaration_names().expect("list on missing dir").is_empty());

        repo.save_declaration("b", &declaration)
            .expect("save must succeed");
        repo.save_declaration("a", &declaration)
            .expect("save must succeed");

        let names = repo.list_declaration_names().expect("list must succeed");
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_removes_the_declaration() {
        let root = create_temp_dir();
        let repo = FileStateDeclarationRepository::new(root.join("state-declarations"));
        let declaration = declaration(&[("环境/日期", "DATE")]);

        repo.save_declaration("demo", &declaration)
            .expect("save must succeed");
        repo.delete_declaration("demo").expect("delete must succeed");

        let names = repo.list_declaration_names().expect("list must succeed");
        assert!(names.is_empty());
        assert!(repo
            .get_declaration("demo")
            .is_err(), "deleted declaration must not be readable");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn get_returns_not_found_for_a_missing_declaration() {
        let root = create_temp_dir();
        let repo = FileStateDeclarationRepository::new(root.join("state-declarations"));

        let error = repo.get_declaration("missing").expect_err("missing must fail");
        assert!(
            matches!(error, DomainError::NotFound(ref message) if message.contains("missing")),
            "expected a NotFound error, got {error:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
