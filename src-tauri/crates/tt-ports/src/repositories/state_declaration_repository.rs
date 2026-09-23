use tt_domain::errors::DomainError;
use tt_domain::models::state::StateDeclaration;

/// Repository interface for managing state declarations
pub trait StateDeclarationRepository: Send + Sync {
    /// Save a state declaration
    fn save_declaration(&self, name: &str, declaration: &StateDeclaration) -> Result<(), DomainError>;

    /// Get a state declaration
    fn get_declaration(&self, name: &str) -> Result<StateDeclaration, DomainError>;

    /// List the names of all stored state declarations
    fn list_declaration_names(&self) -> Result<Vec<String>, DomainError>;

    /// Delete a state declaration
    fn delete_declaration(&self, name: &str) -> Result<(), DomainError>;
}
