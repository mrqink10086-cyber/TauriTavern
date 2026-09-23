use tt_domain::errors::DomainError;
use tt_domain::models::state_predicate::StatePredicateSet;

/// Repository interface for saved predicate documents.
///
/// Mirrors the state declaration store, for the same reason: a predicate set is
/// named, reusable and kept outside the chat, and each chat binds one the way it
/// binds a declaration. What is *not* stored here is the outcome — the entries an
/// evaluation selects are produced again on every read from the current state.
pub trait StatePredicateRepository: Send + Sync {
    /// Save a predicate set under `name`.
    fn save_predicates(&self, name: &str, set: &StatePredicateSet) -> Result<(), DomainError>;

    /// Get a saved predicate set.
    fn get_predicates(&self, name: &str) -> Result<StatePredicateSet, DomainError>;

    /// List the names of all saved predicate sets.
    fn list_predicate_names(&self) -> Result<Vec<String>, DomainError>;

    /// Delete a saved predicate set.
    fn delete_predicates(&self, name: &str) -> Result<(), DomainError>;
}
