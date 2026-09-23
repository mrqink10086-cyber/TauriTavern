//! Calling one inline script module.
//!
//! Two features run a script a spec carries in its own document: the state
//! machine's transition hook (a script may refuse a transition) and a picture
//! set's condition script (a script decides which candidate applies). The
//! engine call, the module map and the error prefixing live here, so both report
//! a failure the same way and neither has to remember the port's shape.
//!
//! What stays with each caller is the *contract* of the returned value: a hook
//! reads `allow`, a picture set reads `index`. That difference is the reason
//! these are two features rather than one, and it is not something this module
//! should guess.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::json;
use tt_domain::frozen_macros::FrozenMacros;
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest};

use crate::errors::ApplicationError;

/// The logical module name used when a spec does not name one.
pub(crate) const DEFAULT_SCRIPT_MODULE: &str = "hook.js";

/// Run one script module and hand back its returned value.
///
/// Scripts run in the skill script sandbox: modules come from memory, there is
/// no workspace and no host context, so a script can read what it was handed and
/// nothing else. A script that fails is an error the caller must see —
/// `failure_code` is the caller's own code, so the message says which feature
/// failed while keeping one implementation of the call.
///
/// `shared` carries the modules the entry may import, so one document can keep
/// its logic once and let several one-line entries use it. A shared module
/// cannot shadow the entry: the entry is what the caller asked to run.
pub(crate) async fn call_script_module(
    engine: &dyn SkillScriptEngine,
    script: &str,
    entry: Option<&str>,
    shared: &BTreeMap<String, String>,
    args: serde_json::Value,
    failure_code: &str,
) -> Result<serde_json::Value, ApplicationError> {
    let entry = entry.unwrap_or(DEFAULT_SCRIPT_MODULE);
    let mut modules: HashMap<String, String> = shared
        .iter()
        .map(|(name, source)| (name.clone(), source.clone()))
        .collect();
    modules.insert(entry.to_string(), script.to_string());

    let result = engine
        .execute(SkillScriptRequest {
            frozen_macros: Arc::new(FrozenMacros::default()),
            entry_module: entry.to_string(),
            modules,
            args,
            workspace_files: HashMap::new(),
            visible_roots: Vec::new(),
            writable_roots: Vec::new(),
            context: json!({}),
        })
        .await
        .map_err(|error| {
            ApplicationError::ValidationError(format!("{failure_code}: {error}"))
        })?;

    Ok(result.value)
}
