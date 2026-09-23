//! Runtime plumbing shared by everything that reads or writes a chat's state.
//!
//! The domain owns what state means and which transitions fire; this module owns
//! where the state document and the machine's position live, how a spec's script
//! hook is run, and how an evaluation's field writes become a document request.
//! Machine writes are routed through here so a transition can only emit a
//! request the document's own path validates, whether it came from an action or
//! from a hook.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::state::{
    CostUnit, StateDeclaration, StateDocument, StateUpdateRequest, count_chars,
};
use tt_domain::models::state_access::StateAccessPolicy;
use tt_domain::models::state_machine::{
    ComparatorRegistry, FieldWrite, MachineError, MachineEvaluation, MachineState, StateMachineSpec,
    evaluate, initial_state,
};
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;
use tt_ports::repositories::workspace_repository::{WorkspaceFile, WorkspaceRepository};
use tt_ports::skill_script::SkillScriptEngine;

use crate::errors::ApplicationError;
use crate::services::agent_tools::{
    ACCESS_SNAPSHOT_KEY, DECLARATION_SNAPSHOT_KEY, STATE_DOCUMENT_PATH,
};
use crate::services::script_module::call_script_module;

/// Where a machine's live positions live inside the run workspace.
///
/// Next to the state document on the same persistent root, so a machine's
/// position is published with the floor that produced it and inherited by the
/// next run, exactly like the values it moved.
pub(crate) const STATE_MACHINE_PATH: &str = "state/machine.json";

/// The run-input key that carries the chat's bound machine, when one exists.
///
/// Absent means "this chat has no machine": the layer stays out of the way
/// completely, which is what makes it optional.
pub(crate) const MACHINE_SNAPSHOT_KEY: &str = "stateMachine";

/// The `reason` a hook sees when it is asked about a transition about to fire.
pub(crate) const TRANSITION_REASON: &str = "transition";

/// The `reason` a hook sees when it is called to recompute, not to decide.
pub(crate) const RECALCULATE_REASON: &str = "recalculate";

/// Where the run's frozen inputs live inside the run workspace.
const RUN_PROMPT_SNAPSHOT_PATH: &str = "input/prompt_snapshot.json";

/// The error code a failing transition hook reports under.
const HOOK_FAILURE_CODE: &str = "state_machine.hook_failed";

/// The error code a hook's unreadable write list reports under.
///
/// Dropping such a write would corrupt state silently, so it stops the call
/// instead: the hook is user configuration, and its author is the one who can
/// fix it.
const HOOK_INVALID_WRITES_CODE: &str = "state_machine.hook_invalid_writes";

/// Read the run's frozen input snapshot.
///
/// Every state decision a run makes — which declaration, which access, which
/// machine — is read from here rather than from live configuration, so the run
/// behaves the same way from its first tool call to its last.
pub(crate) async fn read_run_prompt_snapshot(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
) -> Result<Value, ApplicationError> {
    let path = WorkspacePath::parse(RUN_PROMPT_SNAPSHOT_PATH)?;
    let file = workspace_repository
        .read_text(run_id, &path)
        .await
        .map_err(ApplicationError::from)?;
    serde_json::from_str(&file.text).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "agent.invalid_prompt_snapshot_file: failed to parse prompt snapshot JSON: {error}"
        ))
    })
}

/// The declaration this chat is bound to, when there is one.
///
/// A malformed declaration stops the call: it is a configuration problem, not
/// something the model can retry around.
pub(crate) fn declaration_from_snapshot(
    snapshot: &Value,
) -> Result<StateDeclaration, ApplicationError> {
    match snapshot.get(DECLARATION_SNAPSHOT_KEY) {
        None | Some(Value::Null) => Ok(StateDeclaration::default()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.invalid_declaration: `{DECLARATION_SNAPSHOT_KEY}` in the run input is not a valid state declaration: {error}"
            ))
        }),
    }
}

/// The model this run is talking to, when the snapshot names one.
///
/// It is what a scene counting in tokens follows by default: a tokenizer is a
/// property of the model, and asking the user to name it as well would be asking
/// them to keep two settings in step.
pub(crate) fn model_from_snapshot(snapshot: &Value) -> Option<String> {
    let payload = snapshot
        .get("chatCompletionPayload")
        .or_else(|| snapshot.get("chat_completion_payload"))
        .or_else(|| snapshot.get("generateData"))
        .or_else(|| snapshot.get("generate_data"))?;

    payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

/// How a scene counts one state value, and what it needs to count.
///
/// Characters need nothing, which is why they are the default and stay one. A
/// token count needs a vocabulary and the model it belongs to, and both are
/// carried here so a caller can hand the domain a single measurement function.
pub(crate) enum StateValueMeasure {
    Chars,
    Tokens {
        tokenizer: Arc<dyn TokenizerRepository>,
        model: String,
    },
}

impl StateValueMeasure {
    /// What one value costs, in this scene's unit.
    ///
    /// A vocabulary that cannot encode is an error, not a reason to fall back to
    /// characters: that would silently change what the scene's ceiling means,
    /// which is the one failure a budget must not have.
    pub(crate) fn count(&self, value: &str) -> Result<usize, DomainError> {
        match self {
            Self::Chars => Ok(count_chars(value)),
            Self::Tokens { tokenizer, model } => {
                tokenizer.encode(model, value).map(|ids| ids.len())
            }
        }
    }
}

/// Prepare the measure a scene asked for, or say why it cannot be had.
///
/// A scene counting in characters needs nothing. A scene counting in tokens
/// needs a vocabulary that is available **now**: a family compiled into the app
/// parses from the binary and a file the user named is read from disk, so a
/// write may wait for either. A vocabulary that would have to be downloaded is
/// refused, and so is a missing one — the failure mode this avoids is the worst
/// one a budget can have, where the ceiling silently means something else than
/// what its author wrote.
pub(crate) async fn resolve_state_value_measure(
    declaration: &StateDeclaration,
    snapshot_model: Option<&str>,
    tokenizers: Option<&Arc<dyn TokenizerRepository>>,
) -> Result<StateValueMeasure, ApplicationError> {
    let limits = &declaration.limits;
    if limits.unit != CostUnit::Tokens {
        return Ok(StateValueMeasure::Chars);
    }

    let named = limits.tokenizer.trim();
    let model = if named.is_empty() {
        snapshot_model.map(str::to_string)
    } else {
        Some(named.to_string())
    };
    let Some(model) = model else {
        return Err(ApplicationError::ValidationError(
            "state.tokenizer_unavailable: this scene counts values in tokens but follows a model this chat does not name; name a vocabulary in its limits, or count in characters".to_string(),
        ));
    };

    let Some(tokenizer) = tokenizers else {
        return Err(ApplicationError::ValidationError(format!(
            "state.tokenizer_unavailable: `{model}` cannot be counted with because this host has no tokenizer wired in"
        )));
    };

    if !tokenizer.can_count_offline(&model).await {
        return Err(ApplicationError::ValidationError(format!(
            "state.tokenizer_unavailable: `{model}` is a vocabulary that would have to be downloaded, which a write does not do; name one of the shipped families or a file you supply as `file:<path>`"
        )));
    }

    tokenizer.ensure_model_ready(&model).await.map_err(|error| {
        ApplicationError::ValidationError(format!(
            "state.tokenizer_unavailable: `{model}` cannot count this scene's tokens: {error}"
        ))
    })?;

    Ok(StateValueMeasure::Tokens {
        tokenizer: Arc::clone(tokenizer),
        model,
    })
}

/// This Profile's per-field access, when it configured any.
pub(crate) fn access_from_snapshot(snapshot: &Value) -> Result<StateAccessPolicy, ApplicationError> {
    match snapshot.get(ACCESS_SNAPSHOT_KEY) {
        None | Some(Value::Null) => Ok(StateAccessPolicy::default()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.invalid_access: `{ACCESS_SNAPSHOT_KEY}` in the run input is not a valid state access policy: {error}"
            ))
        }),
    }
}

/// Read the machine the run was started with.
///
/// A malformed spec stops the call: it is a configuration problem, not
/// something the model can retry around.
pub(crate) fn machine_from_snapshot(
    snapshot: &Value,
) -> Result<Option<StateMachineSpec>, ApplicationError> {
    match snapshot.get(MACHINE_SNAPSHOT_KEY) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                ApplicationError::ValidationError(format!(
                    "state.invalid_machine: `{MACHINE_SNAPSHOT_KEY}` in the run input is not a valid state machine: {error}"
                ))
            }),
    }
}

/// Read the chat's state document from the run workspace.
///
/// The run workspace inherits the previous floor's document before the model
/// runs, so this is the whole change set so far, not just this run's writes.
pub(crate) async fn read_state_document(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    declaration: &StateDeclaration,
) -> Result<StateDocument, ApplicationError> {
    let path = state_document_path()?;
    match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => {
            let document: StateDocument = serde_json::from_str(&file.text).map_err(|error| {
                ApplicationError::ValidationError(format!(
                    "state.invalid_document: `{}` is not a valid state document: {error}",
                    path.as_str()
                ))
            })?;
            document.validate().map_err(|message| {
                ApplicationError::ValidationError(format!(
                    "state.invalid_document: `{}` is not usable: {message}",
                    path.as_str()
                ))
            })?;
            Ok(document)
        }
        // A chat's first update has no document yet; that is an expected state,
        // not a failure. What it starts from is the declaration's answer — the
        // values a scene says its fields hold before anybody writes them.
        Err(DomainError::NotFound(_)) => Ok(StateDocument::seeded(declaration)),
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn write_state_document(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    document: &StateDocument,
) -> Result<WorkspaceFile, ApplicationError> {
    let path = state_document_path()?;
    let text = document_text(document)?;
    write_workspace_text(workspace_repository, run_id, &path, &text).await
}

/// Write the document and the machine position as one step.
///
/// They are two files, so a failure between the writes would leave a floor whose
/// values moved but whose position did not — and the next pass would apply the
/// same rule a second time. The document goes first and is put back if the
/// position cannot be written, so a failed pass leaves the previous state whole.
///
/// `previous_document` is what the pass started from, used to restore the values.
/// A document that did not exist yet is restored to what its absence means, so
/// the rollback never has to delete a file.
pub(crate) async fn write_machine_pass(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    previous_document: &StateDocument,
    updated_document: Option<&StateDocument>,
    machine_state: &MachineState,
) -> Result<Vec<WorkspaceFile>, ApplicationError> {
    let document_path = state_document_path()?;
    let machine_path = state_machine_path()?;
    // Serialize both before writing either: a serialization failure must not
    // leave the first file already written.
    let document_source = updated_document.map(document_text).transpose()?;
    let machine_source = machine_state_text(machine_state)?;

    let mut files = Vec::new();
    if let Some(text) = &document_source {
        files.push(write_workspace_text(workspace_repository, run_id, &document_path, text).await?);
    }

    match write_workspace_text(workspace_repository, run_id, &machine_path, &machine_source).await {
        Ok(file) => {
            files.push(file);
            Ok(files)
        }
        Err(error) => {
            if document_source.is_some() {
                let restore = document_text(previous_document)?;
                if let Err(restore_error) =
                    write_workspace_text(workspace_repository, run_id, &document_path, &restore).await
                {
                    return Err(ApplicationError::InternalError(format!(
                        "state.machine_pass_rollback_failed: {restore_error}; the write that needed it: {error}"
                    )));
                }
            }
            Err(error)
        }
    }
}

fn document_text(document: &StateDocument) -> Result<String, ApplicationError> {
    serde_json::to_string_pretty(document).map_err(|error| {
        ApplicationError::InternalError(format!("state.document_serialize_failed: {error}"))
    })
}

fn machine_state_text(state: &MachineState) -> Result<String, ApplicationError> {
    serde_json::to_string_pretty(state).map_err(|error| {
        ApplicationError::InternalError(format!("state.machine_state_serialize_failed: {error}"))
    })
}

async fn write_workspace_text(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    path: &WorkspacePath,
    text: &str,
) -> Result<WorkspaceFile, ApplicationError> {
    workspace_repository
        .write_text(run_id, path, text)
        .await
        .map_err(ApplicationError::from)
}

/// Read where the machine currently stands.
///
/// A machine whose position file is absent has never moved in this chat, so it
/// stands where its spec says it starts. That is the same answer a machine with
/// no position history gets, and it keeps the first run from needing a special
/// case.
pub(crate) async fn read_machine_state(
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    spec: &StateMachineSpec,
) -> Result<MachineState, ApplicationError> {
    let path = state_machine_path()?;
    match workspace_repository.read_text(run_id, &path).await {
        Ok(file) => serde_json::from_str(&file.text).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "state.invalid_machine_state: `{}` is not a valid machine state: {error}",
                path.as_str()
            ))
        }),
        Err(DomainError::NotFound(_)) => Ok(initial_state(spec)),
        Err(error) => Err(error.into()),
    }
}

/// Run one evaluation pass, letting the spec's hook refuse what it wants and
/// write what the machine's actions cannot express.
///
/// The domain picks the transitions; the hook, when there is one, sees every
/// transition about to fire and can refuse any of them. A refusal is not a
/// silent drop: the pass runs again with those indices denied, so the result
/// reports `denied_by_hook` and the remaining rules still get their turn.
///
/// A hook may also return field writes. They belong to the transition the hook
/// was asked about: a write only joins the result when that transition ended up
/// applying. Like an action's writes they are only values — the caller resolves
/// them against the declaration before anything is stored.
pub(crate) async fn evaluate_machine(
    engine: Option<&dyn SkillScriptEngine>,
    comparators: &ComparatorRegistry,
    spec: &StateMachineSpec,
    state: &MachineState,
    fields: &BTreeMap<String, Vec<String>>,
    forced: &BTreeSet<usize>,
) -> Result<MachineEvaluation, ApplicationError> {
    let outcome = evaluate(
        spec,
        state,
        fields,
        comparators,
        &BTreeSet::new(),
        forced,
    )
    .map_err(|error| ApplicationError::ValidationError(format_machine_error(&error)))?;

    let Some(hooks) = spec.hooks.as_ref() else {
        return Ok(outcome);
    };
    let Some(engine) = engine else {
        return Err(ApplicationError::ValidationError(
            "state_machine.hook_unavailable: this machine declares a hook but no script engine is configured"
                .to_string(),
        ));
    };

    let mut denied: BTreeSet<usize> = BTreeSet::new();
    let mut granted: Vec<(usize, Vec<FieldWrite>)> = Vec::new();
    for call in &outcome.hooks {
        let value = call_script_module(
            engine,
            hooks.script.as_str(),
            hooks.entry.as_deref(),
            // A machine's hook is self-contained: the machine has no shared
            // module store, so there is nothing to import from.
            &BTreeMap::new(),
            json!({
                "reason": TRANSITION_REASON,
                "transition": call.id,
                "index": call.index,
                "from": call.from,
                "to": call.to,
                "active": outcome.active.iter().cloned().collect::<Vec<_>>(),
                "fields": fields,
            }),
            HOOK_FAILURE_CODE,
        )
        .await?;
        let verdict = read_hook_verdict(&value)?;
        if verdict.allow {
            if !verdict.writes.is_empty() {
                granted.push((call.index, verdict.writes));
            }
        } else {
            denied.insert(call.index);
        }
    }

    let mut outcome = if denied.is_empty() {
        outcome
    } else {
        evaluate(spec, state, fields, comparators, &denied, forced)
            .map_err(|error| ApplicationError::ValidationError(format_machine_error(&error)))?
    };

    // A hook's writes belong to the transition it was asked about. A transition
    // that did not end up applying — refused by the hook, or outbid by a
    // higher-priority rule in the second pass — takes its writes with it, so the
    // result never carries a value no transition stands behind.
    let applied: BTreeSet<usize> = outcome.applied.iter().map(|entry| entry.index).collect();
    for (index, writes) in granted {
        if applied.contains(&index) {
            outcome.writes.extend(writes);
        }
    }

    Ok(outcome)
}

/// Turn a set of field writes into the state document's request shape.
///
/// Neither a transition nor a hook writes storage: they hand back writes, and
/// they go through `resolve_request` and `apply_update` like any other change —
/// so a script cannot reach a key the declaration does not define either, and an
/// empty value list still means "cleared" rather than "deleted".
pub(crate) fn field_writes_as_request(writes: &[FieldWrite]) -> StateUpdateRequest {
    StateUpdateRequest {
        fields: writes
            .iter()
            .map(|write| (write.key.clone(), write.values.clone()))
            .collect(),
        remove: Vec::new(),
    }
}

/// Ask the machine's hook to recompute what the state cannot derive by itself.
///
/// The hook is the same script, called for a different reason: it sees the
/// fields as they are now and may return writes. `allow` is out of scope here —
/// there is no transition for it to refuse — so only `writes` is read.
///
/// This is what lets a user's edit ripple: a click writes the fields it owns,
/// and the hook turns them into the derived ones (a total, a date, an equipped
/// bonus) without the engine knowing a single formula.
pub(crate) async fn run_recalculation_hook(
    engine: Option<&dyn SkillScriptEngine>,
    spec: &StateMachineSpec,
    active: &BTreeSet<String>,
    fields: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<FieldWrite>, ApplicationError> {
    let Some(hooks) = spec.hooks.as_ref().filter(|hooks| hooks.is_configured()) else {
        return Ok(Vec::new());
    };
    let Some(engine) = engine else {
        return Err(ApplicationError::ValidationError(
            "state_machine.hook_unavailable: this machine declares a hook but no script engine is configured"
                .to_string(),
        ));
    };

    let value = call_script_module(
        engine,
        hooks.script.as_str(),
        hooks.entry.as_deref(),
        &BTreeMap::new(),
        json!({
            "reason": RECALCULATE_REASON,
            "transition": Value::Null,
            "index": Value::Null,
            "from": Vec::<String>::new(),
            "to": Vec::<String>::new(),
            "active": active.iter().cloned().collect::<Vec<_>>(),
            "fields": fields,
        }),
        HOOK_FAILURE_CODE,
    )
    .await?;

    Ok(read_hook_verdict(&value)?.writes)
}

pub(crate) fn state_document_path() -> Result<WorkspacePath, ApplicationError> {
    WorkspacePath::parse(STATE_DOCUMENT_PATH).map_err(|error| {
        ApplicationError::InternalError(format!("state.document_path_invalid: {error}"))
    })
}

pub(crate) fn state_machine_path() -> Result<WorkspacePath, ApplicationError> {
    WorkspacePath::parse(STATE_MACHINE_PATH).map_err(|error| {
        ApplicationError::InternalError(format!("state.machine_path_invalid: {error}"))
    })
}

/// What one hook said about one transition.
///
/// `writes` is how a hook owns the semantics the engine deliberately does not:
/// a calendar, a currency conversion, a damage formula, a derived stat. The
/// engine only has to carry the values through the state document's own path.
struct HookVerdict {
    allow: bool,
    writes: Vec<FieldWrite>,
}

/// Read a hook's verdict.
///
/// The contract is small on purpose: return `{ allow: false, reason: "…" }` to
/// refuse, and `{ writes: [{ key, values }] }` to compute a value. Anything that
/// is not an explicit refusal counts as permission, because a script that throws
/// is an error the caller must see, not a veto the caller has to guess about.
///
/// A write list that cannot be read is that same error rather than a silent
/// drop: half-applied hook writes are worse than a call the author can fix.
fn read_hook_verdict(value: &Value) -> Result<HookVerdict, ApplicationError> {
    let allow = value
        .get("allow")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let writes = match value.get("writes") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(hook_write)
            .collect::<Result<Vec<_>, ApplicationError>>()?,
        Some(other) => {
            return Err(hook_invalid_writes(format!(
                "`writes` must be an array, got {other}"
            )));
        }
    };
    Ok(HookVerdict { allow, writes })
}

fn hook_write(item: &Value) -> Result<FieldWrite, ApplicationError> {
    let key = item
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            hook_invalid_writes(format!("every write needs a non-empty `key`: {item}"))
        })?;
    let values = item
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| hook_invalid_writes(format!("`{key}` needs `values` as an array: {item}")))?;
    let values = values
        .iter()
        .map(hook_value)
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    Ok(FieldWrite {
        key: key.to_string(),
        values,
    })
}

/// A state value is a line of text, so the three literals a script naturally
/// reaches for are accepted and everything else is refused rather than guessed.
fn hook_value(value: &Value) -> Result<String, ApplicationError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        other => Err(hook_invalid_writes(format!(
            "a value must be a string, number or boolean, got {other}"
        ))),
    }
}

fn hook_invalid_writes(detail: String) -> ApplicationError {
    ApplicationError::ValidationError(format!("{HOOK_INVALID_WRITES_CODE}: {detail}"))
}

/// One line per problem, so a caller can show all of them at once.
pub(crate) fn format_errors(errors: &[MachineError]) -> String {
    errors
        .iter()
        .map(|error| match &error.target {
            Some(target) => format!("- {} ({target}): {}", error.code, error.message),
            None => format!("- {}: {}", error.code, error.message),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn format_machine_error(error: &MachineError) -> String {
    format_errors(std::slice::from_ref(error))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::{Value, json};

    use super::{
        access_from_snapshot, evaluate_machine, field_writes_as_request, machine_from_snapshot,
        run_recalculation_hook,
    };
    use crate::errors::ApplicationError;
    use crate::services::agent_tools::ACCESS_SNAPSHOT_KEY;
    use tt_domain::models::script_spec::ScriptSpec;
    use tt_domain::models::state::StateKey;
    use tt_domain::models::state_access::{FieldAccess, StateFieldAccess};
    use tt_domain::models::state_machine::{
        ActionSpec, ComparatorRegistry, ConditionSpec, FieldWrite, MachineEvaluation, MachineState,
        SOURCE_FIELD, StateMachineSpec, StateSpec, TransitionSpec,
    };
    use tt_ports::skill_script::{
        SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest, SkillScriptResult,
    };

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    /// An engine that answers with whatever the test queued, and remembers what
    /// it was asked.
    struct FakeEngine {
        answer: Value,
        calls: Mutex<Vec<Value>>,
    }

    impl FakeEngine {
        fn answering(answer: Value) -> Self {
            Self {
                answer,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.lock().expect("test lock").len()
        }

        fn args(&self) -> Vec<Value> {
            self.calls.lock().expect("test lock").clone()
        }
    }

    #[async_trait]
    impl SkillScriptEngine for FakeEngine {
        async fn execute(
            &self,
            request: SkillScriptRequest,
        ) -> Result<SkillScriptResult, SkillScriptEngineError> {
            self.calls.lock().expect("test lock").push(request.args);
            Ok(SkillScriptResult {
                value: self.answer.clone(),
                writes: Vec::new(),
                last_write_path: None,
                logs: Vec::new(),
            })
        }
    }

    /// A machine with one rule: 好感 ≥ 30 moves 初见 → 试探 and stamps the stage.
    fn one_transition_spec(hook: Option<&str>) -> StateMachineSpec {
        StateMachineSpec {
            initial: vec!["初见".to_string()],
            states: vec![
                StateSpec {
                    id: "初见".to_string(),
                    label: None,
                    terminal: false,
                },
                StateSpec {
                    id: "试探".to_string(),
                    label: None,
                    terminal: false,
                },
            ],
            transitions: vec![TransitionSpec {
                id: Some("warm-up".to_string()),
                from: vec!["初见".to_string()],
                to: vec!["试探".to_string()],
                conditions: vec![ConditionSpec {
                    source: SOURCE_FIELD.to_string(),
                    field: Some("关系/好感".to_string()),
                    op: "gte".to_string(),
                    value: Some("30".to_string()),
                    ..Default::default()
                }],
                actions: vec![ActionSpec {
                    kind: "setField".to_string(),
                    target: Some("关系/阶段".to_string()),
                    values: vec!["试探".to_string()],
                }],
                priority: 0,
            }],
            hooks: hook.map(|script| ScriptSpec {
                script: script.to_string(),
                entry: None,
            }),
        }
    }

    fn fields() -> std::collections::BTreeMap<String, Vec<String>> {
        [("关系/好感".to_string(), vec!["40".to_string()])]
            .into_iter()
            .collect()
    }

    /// The machine stands where its `from` side expects it to.
    fn at_start() -> MachineState {
        MachineState {
            active: ["初见".to_string()].into_iter().collect(),
        }
    }

    /// The empty set of forced transitions, which is what a completion pass uses.
    fn nothing_forced() -> std::collections::BTreeSet<usize> {
        std::collections::BTreeSet::new()
    }

    async fn evaluate_with(engine: &dyn SkillScriptEngine) -> MachineEvaluation {
        evaluate_machine(
            Some(engine),
            &ComparatorRegistry::default(),
            &one_transition_spec(Some("export default () => ({})")),
            &at_start(),
            &fields(),
            &nothing_forced(),
        )
        .await
        .expect("the pass must succeed")
    }

    #[tokio::test]
    async fn a_hook_can_write_what_the_actions_cannot_express() {
        // A calendar or a formula is not something `setField` can say: the hook
        // computes it, and the engine only carries the value onwards.
        let engine = FakeEngine::answering(json!({
            "writes": [{ "key": "环境/日期", "values": ["2026-09-23"] }]
        }));

        let evaluation = evaluate_with(&engine).await;

        assert_eq!(engine.calls(), 1, "one hook call per selected transition");
        assert_eq!(
            evaluation.writes,
            vec![
                FieldWrite {
                    key: "关系/阶段".to_string(),
                    values: vec!["试探".to_string()],
                },
                FieldWrite {
                    key: "环境/日期".to_string(),
                    values: vec!["2026-09-23".to_string()],
                },
            ],
            "the hook's write must join the action's, after it"
        );
    }

    #[tokio::test]
    async fn a_hook_write_takes_loose_scalars_as_values() {
        // A value is a line of text; a script writing a number or a boolean
        // should not have to spell it as one.
        let engine = FakeEngine::answering(json!({
            "writes": [{ "key": "战斗/伤害", "values": [57, true] }]
        }));

        let evaluation = evaluate_with(&engine).await;

        assert_eq!(
            evaluation.writes.last().expect("the hook's write"),
            &FieldWrite {
                key: "战斗/伤害".to_string(),
                values: vec!["57".to_string(), "true".to_string()],
            }
        );
    }

    #[tokio::test]
    async fn a_refused_transition_takes_its_hook_writes_with_it() {
        let engine = FakeEngine::answering(json!({
            "allow": false,
            "reason": "信任已破裂，不能进入试探",
            "writes": [{ "key": "环境/日期", "values": ["2026-09-23"] }]
        }));

        let evaluation = evaluate_with(&engine).await;

        assert!(
            evaluation.writes.iter().all(|write| write.key != "环境/日期"),
            "a refused transition must not leave its hook's writes behind"
        );
        assert!(
            evaluation
                .skipped
                .iter()
                .any(|entry| entry.reason == "denied_by_hook"),
            "the refusal must still be reported"
        );
    }

    #[tokio::test]
    async fn a_hook_write_list_that_cannot_be_read_stops_the_call() {
        // Half-applying a hook's writes would corrupt state silently, so an
        // unreadable list is an error the hook's author has to see.
        let engine = FakeEngine::answering(json!({ "writes": "2026-09-23" }));

        let error = evaluate_machine(
            Some(&engine),
            &ComparatorRegistry::default(),
            &one_transition_spec(Some("export default () => ({})")),
            &at_start(),
            &fields(),
            &nothing_forced(),
        )
        .await
        .expect_err("an unreadable write list must be refused");

        match error {
            ApplicationError::ValidationError(message) => assert!(
                message.starts_with("state_machine.hook_invalid_writes"),
                "the error must carry the invalid-writes code: {message}"
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_recalculation_call_asks_the_hook_to_recompute() {
        // A user edit ripples through the same hook, told apart by `reason`: it
        // computes a derived value, and the engine still carries only values.
        let engine = FakeEngine::answering(json!({
            "writes": [{ "key": "战斗/总攻", "values": [57] }]
        }));

        let writes = run_recalculation_hook(
            Some(&engine),
            &one_transition_spec(Some("export default ({ fields }) => ({})")),
            &at_start().active,
            &fields(),
        )
        .await
        .expect("the hook must run");

        assert_eq!(
            writes,
            vec![FieldWrite {
                key: "战斗/总攻".to_string(),
                values: vec!["57".to_string()],
            }]
        );
        let args = engine.args();
        assert_eq!(
            args[0].get("reason").and_then(Value::as_str),
            Some("recalculate"),
            "the hook must be able to tell a recomputation from a decision"
        );
        assert!(
            args[0].get("transition").expect("the key is present").is_null(),
            "a recomputation is about no transition"
        );
        assert_eq!(args[0].get("fields"), Some(&json!(fields())));
    }

    #[tokio::test]
    async fn a_transition_call_tells_the_hook_which_reason_it_was_called_for() {
        let engine = FakeEngine::answering(json!({}));

        evaluate_with(&engine).await;

        assert_eq!(
            engine.args()[0].get("reason").and_then(Value::as_str),
            Some("transition")
        );
    }

    #[tokio::test]
    async fn a_machine_without_a_hook_recomputes_nothing() {
        let engine = FakeEngine::answering(json!({}));

        let writes = run_recalculation_hook(
            Some(&engine),
            &one_transition_spec(None),
            &at_start().active,
            &fields(),
        )
        .await
        .expect("no hook is not an error");

        assert!(writes.is_empty());
        assert_eq!(engine.calls(), 0);
    }

    #[tokio::test]
    async fn a_recalculation_without_an_engine_is_reported_not_skipped() {
        // The hook is what makes the edit mean anything; running the edit without
        // it would publish a state the user did not ask for.
        let error = run_recalculation_hook(
            None,
            &one_transition_spec(Some("export default () => ({})")),
            &at_start().active,
            &fields(),
        )
        .await
        .expect_err("a declared hook with no engine must be refused");

        match error {
            ApplicationError::ValidationError(message) => assert!(
                message.starts_with("state_machine.hook_unavailable"),
                "the error must carry the hook_unavailable code: {message}"
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_machine_without_a_hook_carries_no_hook_writes() {
        let engine = FakeEngine::answering(json!({}));

        let evaluation = evaluate_machine(
            Some(&engine),
            &ComparatorRegistry::default(),
            &one_transition_spec(None),
            &at_start(),
            &fields(),
            &nothing_forced(),
        )
        .await
        .expect("the pass must succeed");

        assert_eq!(engine.calls(), 0, "no hook means no script call");
        assert_eq!(evaluation.writes.len(), 1, "only the action's write is left");
    }

    #[test]
    fn an_absent_machine_key_means_no_machine() {
        for snapshot in [json!({}), json!({ "stateMachine": null })] {
            assert!(
                machine_from_snapshot(&snapshot)
                    .expect("an absent machine must not be an error")
                    .is_none(),
                "a chat without a bound machine must stay on the state-document-only path"
            );
        }
    }

    #[test]
    fn a_malformed_machine_stops_the_call() {
        // A broken spec is a configuration problem, not a model mistake, so it
        // must not be handed back as something the model can retry.
        let snapshot = json!({ "stateMachine": { "initial": "not-an-array" } });

        let error = machine_from_snapshot(&snapshot).expect_err("a broken machine must be refused");

        match error {
            ApplicationError::ValidationError(message) => assert!(
                message.starts_with("state.invalid_machine"),
                "the error must carry the invalid_machine code: {message}"
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn machine_writes_keep_their_keys_and_clears() {
        let evaluation = MachineEvaluation {
            writes: vec![
                FieldWrite {
                    key: "关系/阶段".to_string(),
                    values: vec!["试探".to_string()],
                },
                FieldWrite {
                    key: "关系/备注".to_string(),
                    values: Vec::new(),
                },
            ],
            ..Default::default()
        };

        let request = field_writes_as_request(&evaluation.writes);

        assert_eq!(
            request.fields,
            vec![
                ("关系/阶段".to_string(), vec!["试探".to_string()]),
                ("关系/备注".to_string(), Vec::new()),
            ],
            "an empty value list must stay a clear, not become a removal"
        );
        assert!(request.remove.is_empty(), "a machine never deletes keys");
    }

    #[test]
    fn a_run_without_an_access_key_is_unconstrained() {
        for snapshot in [json!({}), json!({ "stateAccess": null })] {
            let policy = access_from_snapshot(&snapshot)
                .expect("an absent access policy must fall back to the unconfigured default");

            assert!(policy.is_empty());
            assert_eq!(
                policy.lookup(&key("环境/日期")),
                FieldAccess::Unconstrained,
                "an absent policy must not be read as a policy that granted nothing"
            );
        }
    }

    #[test]
    fn a_bound_access_policy_arrives_with_its_switches() {
        let snapshot = json!({
            ACCESS_SNAPSHOT_KEY: {
                "entries": [
                    { "pattern": "环境/日期", "inject": true, "visible": false, "writable": false }
                ]
            }
        });

        let policy = access_from_snapshot(&snapshot).expect("a well-formed policy must be read");

        assert_eq!(
            policy.lookup(&key("环境/日期")),
            FieldAccess::Configured(StateFieldAccess {
                inject: true,
                visible: false,
                writable: false,
                // The entry names no slot, so the default applies: close to the
                // end of the chat, out of the cached prefix.
                inject_slot: tt_domain::models::state_injection::StateInjectionSlot::AtDepth,
                inject_depth: tt_domain::models::state_injection::DEFAULT_INJECT_DEPTH,
            })
        );
        assert_eq!(
            policy.lookup(&key("环境/地点")),
            FieldAccess::Configured(StateFieldAccess::NONE),
            "a configured policy grants nothing to the keys it does not cover"
        );
    }

    #[test]
    fn a_malformed_access_policy_stops_the_call() {
        // A broken policy is a configuration problem, not a model mistake, so it
        // must not be handed back as something the model can retry.
        let snapshot = json!({ ACCESS_SNAPSHOT_KEY: { "entries": [{ "pattern": "环境//日期" }] } });

        let error = access_from_snapshot(&snapshot).expect_err("a broken policy must be refused");

        match error {
            ApplicationError::ValidationError(message) => assert!(
                message.starts_with("state.invalid_access"),
                "the error must carry the invalid_access code: {message}"
            ),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::Value;

    use super::{StateValueMeasure, resolve_state_value_measure};
    use tt_domain::errors::DomainError;
    use tt_domain::models::state::{CostUnit, StateDeclaration, StateLimits};
    use tt_ports::repositories::tokenizer_repository::TokenizerRepository;

    /// A vocabulary that counts words, and says whether it is already here.
    struct WordTokenizer {
        offline: bool,
    }

    #[async_trait]
    impl TokenizerRepository for WordTokenizer {
        async fn ensure_model_ready(&self, _model: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn encode(&self, _model: &str, text: &str) -> Result<Vec<u32>, DomainError> {
            Ok(vec![0; text.split_whitespace().count()])
        }

        fn decode(&self, _model: &str, _ids: &[u32]) -> Result<String, DomainError> {
            Ok(String::new())
        }

        fn count_messages(&self, _model: &str, _messages: &[Value]) -> Result<usize, DomainError> {
            Ok(0)
        }

        async fn can_count_offline(&self, _model: &str) -> bool {
            self.offline
        }
    }

    /// A vocabulary that is loaded but cannot encode.
    struct UnencodableTokenizer;

    #[async_trait]
    impl TokenizerRepository for UnencodableTokenizer {
        async fn ensure_model_ready(&self, _model: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn encode(&self, model: &str, _text: &str) -> Result<Vec<u32>, DomainError> {
            Err(DomainError::InternalError(format!("{model} cannot encode")))
        }

        fn decode(&self, _model: &str, _ids: &[u32]) -> Result<String, DomainError> {
            Ok(String::new())
        }

        fn count_messages(&self, _model: &str, _messages: &[Value]) -> Result<usize, DomainError> {
            Ok(0)
        }

        async fn can_count_offline(&self, _model: &str) -> bool {
            true
        }
    }

    fn scene(unit: CostUnit, tokenizer: &str) -> StateDeclaration {
        StateDeclaration {
            limits: StateLimits {
                unit,
                tokenizer: tokenizer.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_scene_counting_in_characters_never_needs_a_vocabulary() {
        let declaration = scene(CostUnit::Chars, "");
        let measure = resolve_state_value_measure(&declaration, None, None)
            .await
            .expect("characters need nothing");

        assert_eq!(measure.count("一万 二千").expect("characters count"), 5);
        assert!(matches!(measure, StateValueMeasure::Chars));
    }

    #[tokio::test]
    async fn a_scene_counting_in_tokens_counts_with_the_vocabulary_it_named() {
        let tokenizer: Arc<dyn TokenizerRepository> = Arc::new(WordTokenizer { offline: true });
        let declaration = scene(CostUnit::Tokens, "glm");
        let measure = resolve_state_value_measure(&declaration, None, Some(&tokenizer))
            .await
            .expect("a named vocabulary answers");

        assert_eq!(measure.count("一万 二千 三百").expect("tokens count"), 3);
    }

    #[tokio::test]
    async fn a_value_that_cannot_be_measured_is_refused_rather_than_counted_in_characters() {
        let tokenizer: Arc<dyn TokenizerRepository> = Arc::new(UnencodableTokenizer);
        let declaration = scene(CostUnit::Tokens, "glm");
        let measure = resolve_state_value_measure(&declaration, None, Some(&tokenizer))
            .await
            .expect("a vocabulary that is here is accepted up front");

        assert!(measure.count("一万").is_err());
    }

    #[tokio::test]
    async fn a_scene_following_a_model_that_needs_downloading_is_refused() {
        let tokenizer: Arc<dyn TokenizerRepository> = Arc::new(WordTokenizer { offline: false });
        let declaration = scene(CostUnit::Tokens, "");

        let error = match resolve_state_value_measure(&declaration, Some("llama"), Some(&tokenizer))
            .await
        {
            Ok(_) => panic!("a write does not start a download"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("state.tokenizer_unavailable"), "{error}");

        let missing =
            match resolve_state_value_measure(&scene(CostUnit::Tokens, ""), None, Some(&tokenizer))
                .await
            {
                Ok(_) => panic!("a scene with no model and no vocabulary cannot be measured"),
                Err(error) => error,
            };
        assert!(missing.to_string().contains("state.tokenizer_unavailable"), "{missing}");
    }
}
