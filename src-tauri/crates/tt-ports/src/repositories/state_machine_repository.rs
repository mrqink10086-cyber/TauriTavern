use tt_domain::errors::DomainError;
use tt_domain::models::state_machine::StateMachineSpec;

/// Repository interface for saved state machine definitions.
///
/// Mirrors the state declaration store: named, reusable specs kept outside the
/// chat, bound to a chat the same way declarations are.
pub trait StateMachineRepository: Send + Sync {
    /// Save a state machine under `name`.
    fn save_machine(&self, name: &str, spec: &StateMachineSpec) -> Result<(), DomainError>;

    /// Get a saved state machine.
    fn get_machine(&self, name: &str) -> Result<StateMachineSpec, DomainError>;

    /// List the names of all saved state machines.
    fn list_machine_names(&self) -> Result<Vec<String>, DomainError>;

    /// Delete a saved state machine.
    fn delete_machine(&self, name: &str) -> Result<(), DomainError>;
}
