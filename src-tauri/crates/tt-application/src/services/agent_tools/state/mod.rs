//! The `state.update` tool: the one way state changes.
//!
//! State is a document rather than free text, so the model never has to write a
//! format the renderer has to parse back. One call carries the whole change set,
//! the domain validates it as a batch, and a rejection reports every problem at
//! once so the model can fix the submission in a single retry.

mod descriptors;
mod transition;
mod update;

pub(super) use descriptors::{state_transition_descriptor, state_update_descriptor};
pub(super) use transition::transition;
pub(super) use update::update;

pub(crate) const STATE_UPDATE: &str = "state.update";
pub(crate) const STATE_TRANSITION: &str = "state.transition";
/// Where a chat's state lives inside the run workspace.
///
/// One document per run, published whole, so a floor never carries a partially
/// written state.
pub(crate) const STATE_DOCUMENT_PATH: &str = "state/document.json";

/// The run-input key that carries the user's declaration, when one exists.
///
/// Absent means "not configured yet": only the key shape is checked.
pub(crate) const DECLARATION_SNAPSHOT_KEY: &str = "stateDeclaration";

/// The run-input key that carries this Profile's per-field access.
///
/// Absent means "not configured yet": no field is refused for access reasons,
/// exactly as an absent declaration refuses no well-shaped key.
pub(crate) const ACCESS_SNAPSHOT_KEY: &str = "stateAccess";
