//! An inline script module, declared by the spec that needs it.
//!
//! The spec carries the source and a logical module name; the engine, the
//! module map and the call itself belong to the application layer. Keeping the
//! source *inside* the document is what makes a script travel with the
//! configuration that depends on it — there is no second store whose contents
//! can drift out of step with the spec, and a document can be moved, exported
//! or imported as one thing.
//!
//! Two features declare one: the state machine's transition hook (a script may
//! refuse a transition) and a picture set's condition script (a script decides
//! which candidate applies). They share this type so "a script written here" is
//! one idea, not two that drift apart.

use serde::{Deserialize, Serialize};

/// One inline script module.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSpec {
    /// Source of the module.
    pub script: String,
    /// Logical module name handed to the engine. Defaults to `hook.js`.
    #[serde(default)]
    pub entry: Option<String>,
}

impl ScriptSpec {
    /// Whether the spec names a script at all. A blank source is not a script:
    /// it is a half-filled field, and the save-time check refuses it.
    pub fn is_configured(&self) -> bool {
        !self.script.trim().is_empty()
    }
}
