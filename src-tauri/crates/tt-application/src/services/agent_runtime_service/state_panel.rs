use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use super::AgentRuntimeService;
use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use crate::services::script_module::call_script_module;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::{AgentChatRef, WorkspacePath};
use tt_domain::models::state::{StateDeclaration, StateDocument};
use tt_domain::models::state_machine::ComparatorRegistry;
use tt_domain::models::state_panel::{
    ResolvedPanel, ResolvedPanelField, ResolvedProse, StateImageCandidate, StateImageSet,
    StatePanelSpec, resolve_all_fields, resolve_panels,
};
use tt_ports::skill_script::SkillScriptEngine;

/// The error code a failing condition script reports under.
const PANEL_SCRIPT_FAILURE_CODE: &str = "state.panel_script_failed";

/// The error code a script answer that cannot be read reports under.
const PANEL_SCRIPT_INVALID_CODE: &str = "state.panel_script_invalid";

/// What the panel shows for one chat.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePanelDto {
    /// The published version the panel reads. Absent means the chat has no
    /// committed state yet, which is a fact about the chat and not a failure.
    pub state_id: Option<String>,
    pub panels: Vec<ResolvedPanel>,
    /// Every field the chat's state carries, for a template to bind beyond the
    /// panel it sits in.
    ///
    /// A panel's own rows are what its `match` covers; a panel that can only see
    /// that slice cannot show a summary of the whole scene.
    pub fields: Vec<ResolvedPanelField>,
    /// The declaration's theme stylesheet, exactly as it was stored.
    ///
    /// It travels with the resolved panels because the renderer needs both in
    /// one answer: the theme is what makes the panels look like anything, and a
    /// second call to fetch it would be a second chance to draw a panel without
    /// it. The text is already scoped and validated — nothing is decided here.
    pub theme_css: String,
    /// Whether the newest floor was expected to update state and did not.
    ///
    /// The panel shows the newest committed version, so a floor that wrote no
    /// state leaves it showing the previous one. Saying so is what keeps the
    /// panel from presenting an older state as the current one; a chat whose
    /// floors never expected an update reports `true` and shows no mark.
    pub state_updated: bool,
}

impl AgentRuntimeService {
    /// Resolve the panels for one chat.
    ///
    /// The panel reads the chat's **latest committed** state, the same version
    /// the injection reads: a panel that showed a run's unpublished working copy
    /// would show state the chat has not accepted yet.
    ///
    /// The declaration arrives from the host, which owns the binding
    /// (chat metadata -> character/group), exactly as it does for the run input.
    /// A chat with no declaration has no panel: the key space and the display
    /// names both come from the declaration.
    pub async fn resolve_state_panel(
        &self,
        chat_ref: &AgentChatRef,
        stable_chat_id: &str,
        declaration: &StateDeclaration,
    ) -> Result<StatePanelDto, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(stable_chat_id)?;
        let state_updated = self.resolve_latest_state_updated(chat_ref).await?;

        // A chat whose first run has not published yet still has a state to show:
        // the values the declaration says its fields hold. Without this the panel
        // is blank on exactly the floor where a scene was just set up — the moment
        // a person is most likely to look at it. `state_id: None` says plainly
        // that nothing has been published yet.
        let Some(state_id) = self.resolve_latest_persisted_state_id(chat_ref).await? else {
            let document = StateDocument::seeded(declaration);
            return Ok(StatePanelDto {
                state_id: None,
                panels: resolve_panels(
                    declaration,
                    &document,
                    &declaration.panels,
                    &ComparatorRegistry::default(),
                )
                .map_err(|error| {
                    ApplicationError::ValidationError(format!("state.panel_invalid: {error}"))
                })?,
                fields: resolve_all_fields(
                    declaration,
                    &document,
                    &declaration.panels,
                    &ComparatorRegistry::default(),
                )
                .map_err(|error| {
                    ApplicationError::ValidationError(format!("state.panel_invalid: {error}"))
                })?,
                theme_css: declaration.panels.css.clone(),
                state_updated,
            });
        };
        let workspace_id = workspace_id_for_stable_chat_id(chat_ref, &stable_chat_id)?;
        let Some(document) = self
            .read_persisted_state_document(&workspace_id, &state_id)
            .await?
        else {
            return Ok(StatePanelDto {
                state_id: Some(state_id),
                panels: Vec::new(),
                fields: Vec::new(),
                theme_css: declaration.panels.css.clone(),
                state_updated,
            });
        };

        // A picture set that declares a condition script is decided by that
        // script, so the scripts run first and the sets they decided are handed
        // to the domain already resolved. The domain then evaluates plain
        // conditions exactly as it always did, which keeps the deterministic
        // selection rules in one place — and a set whose script chose nothing
        // simply has no candidates left to show.
        let mut resolved = declaration.clone();
        let fields = document.condition_fields();
        let shared = resolved.panels.scripts.clone();
        apply_condition_scripts(&mut resolved, &fields, &shared, self.script_engine.as_ref()).await?;

        // A configuration that cannot be honoured stops the read: showing a
        // panel with the wrong picture, or with no picture at all, is exactly
        // the silent failure this system exists to remove.
        let panels = resolve_panels(
            &resolved,
            &document,
            &resolved.panels,
            &ComparatorRegistry::default(),
        )
        .map_err(|error| {
            ApplicationError::ValidationError(format!("state.panel_invalid: {error}"))
        })?;

        // A prose block is read after the fields, from the same version: the panel
        // shows what that one floor published, not what a later run has been
        // writing into its working copy.
        let panels = self
            .attach_prose(panels, &resolved.panels.panels, &workspace_id, &state_id)
            .await?;

        // The whole key space, not just what the panels' `match` covers: a
        // template may bind any field the declaration defines.
        let fields = resolve_all_fields(
            &resolved,
            &document,
            &resolved.panels,
            &ComparatorRegistry::default(),
        )
        .map_err(|error| {
            ApplicationError::ValidationError(format!("state.panel_invalid: {error}"))
        })?;

        Ok(StatePanelDto {
            state_id: Some(state_id),
            panels,
            fields,
            theme_css: declaration.panels.css.clone(),
            state_updated,
        })
    }

    /// Fill in each panel's prose from the version the panel is showing.
    ///
    /// A file the floor never wrote is not an error: a scene is saved before its
    /// diary has any pages, and a panel that refused to draw until one existed
    /// would make the scene look broken. The block is simply left out.
    async fn attach_prose(
        &self,
        mut panels: Vec<ResolvedPanel>,
        specs: &[StatePanelSpec],
        workspace_id: &str,
        state_id: &str,
    ) -> Result<Vec<ResolvedPanel>, ApplicationError> {
        for (index, panel) in panels.iter_mut().enumerate() {
            let Some(spec) = specs.get(index).and_then(|spec| spec.prose.as_ref()) else {
                continue;
            };
            let Ok(path) = WorkspacePath::parse(spec.path.trim()) else {
                // The save refuses such a path, so this is only reachable from a
                // document that was never validated.
                continue;
            };
            match self
                .workspace_repository
                .read_persistent_state_file(workspace_id, state_id, &path)
                .await
            {
                Ok(file) => {
                    panel.prose = Some(ResolvedProse {
                        title: spec.title.trim().to_string(),
                        text: file.text,
                        path: path.as_str().to_string(),
                    });
                }
                Err(DomainError::NotFound(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(panels)
    }

    /// Whether the newest floor reported a state update.
    ///
    /// The flag rides on the floor's metadata, written when the run published its
    /// state. A floor that never said anything — an older chat, a normal
    /// (non-Agent) reply, one generated before this flag existed — reads as
    /// updated: the mark means "what you are looking at is older than the
    /// story", and raising it for floors that never promised an update would
    /// make it noise.
    pub(super) async fn resolve_latest_state_updated(
        &self,
        chat_ref: &AgentChatRef,
    ) -> Result<bool, ApplicationError> {
        let Some(message) = self.find_last_chat_message(chat_ref).await? else {
            return Ok(true);
        };
        Ok(message
            .message
            .pointer("/extra/tauritavern/agent/stateUpdated")
            .and_then(Value::as_bool)
            .unwrap_or(true))
    }
}

/// Replace every scripted picture set with what its script decided.
///
/// Only sets that declare a script are touched, and each is rewritten to the
/// single candidate its script chose (or to no candidates at all). Rewriting the
/// configuration instead of threading a decision through the resolver is what
/// keeps the domain free of script handling: what the domain sees afterwards is
/// an ordinary set with one unconditional picture.
async fn apply_condition_scripts(
    declaration: &mut StateDeclaration,
    fields: &BTreeMap<String, Vec<String>>,
    shared: &BTreeMap<String, String>,
    engine: &dyn SkillScriptEngine,
) -> Result<(), ApplicationError> {
    for panel in &mut declaration.panels.panels {
        apply_condition_script(&mut panel.background, fields, shared, engine).await?;
        for field in &mut panel.fields {
            apply_condition_script(&mut field.images, fields, shared, engine).await?;
        }
    }
    Ok(())
}

async fn apply_condition_script(
    set: &mut Option<StateImageSet>,
    fields: &BTreeMap<String, Vec<String>>,
    shared: &BTreeMap<String, String>,
    engine: &dyn SkillScriptEngine,
) -> Result<(), ApplicationError> {
    let Some(set) = set.as_mut() else {
        return Ok(());
    };
    let Some(script) = set.condition_script.clone() else {
        return Ok(());
    };
    if !script.is_configured() {
        // Refusing beats falling back to the conditions: a set whose script is
        // empty would otherwise show a picture the configuration never chose.
        return Err(invalid(
            "a picture set declares a condition script with no source",
        ));
    }

    let candidates = set
        .candidates
        .iter()
        .map(|candidate| candidate.source.clone())
        .collect::<Vec<_>>();
    let value = call_script_module(
        engine,
        script.script.as_str(),
        script.entry.as_deref(),
        shared,
        json!({ "state": fields, "candidates": candidates }),
        PANEL_SCRIPT_FAILURE_CODE,
    )
    .await?;

    set.candidates = match read_script_choice(&value, set.candidates.len())? {
        // The chosen picture keeps no condition: the script already decided, and
        // re-evaluating the condition it happened to carry could drop it again.
        Some(index) => vec![StateImageCandidate {
            source: set.candidates[index].source.clone(),
            when: None,
        }],
        None => Vec::new(),
    };
    Ok(())
}

/// Read the candidate a condition script chose.
///
/// The contract is small on purpose: `{ "index": 2 }` picks a candidate,
/// `{ "index": null }` (or no `index` at all) says no picture applies. Anything
/// else is an error rather than a guess — a picture the script did not choose is
/// exactly what this whole path exists to avoid.
fn read_script_choice(
    value: &serde_json::Value,
    candidate_count: usize,
) -> Result<Option<usize>, ApplicationError> {
    let Some(index) = value.get("index") else {
        return Ok(None);
    };
    if index.is_null() {
        return Ok(None);
    }

    let Some(index) = index.as_u64() else {
        return Err(invalid("`index` must be a whole number, or null for no picture"));
    };
    let index = usize::try_from(index)
        .map_err(|_| invalid("`index` is larger than this platform can address"))?;
    if index >= candidate_count {
        return Err(invalid(&format!(
            "the script chose candidate #{index}, but the set has {candidate_count}"
        )));
    }
    Ok(Some(index))
}

fn invalid(message: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!("{PANEL_SCRIPT_INVALID_CODE}: {message}"))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tt_domain::models::script_spec::ScriptSpec;
    use tt_domain::models::state::{
        DeclaredStateField, StateDeclaration, StateDocument, StateField, StateKey,
    };
    use tt_domain::models::state_access::StateFieldAccess;
    use tt_domain::models::state_key::StateKeyPattern;
    use tt_domain::models::state_machine::{ComparatorRegistry, ConditionSpec, SOURCE_FIELD};
    use tt_domain::models::state_panel::{
        StateImageCandidate, StateImageSet, StatePanelConfig, StatePanelRail, StatePanelSpec,
        resolve_panels,
    };
    use tt_ports::skill_script::{
        SkillScriptEngine, SkillScriptEngineError, SkillScriptRequest, SkillScriptResult,
    };

    use super::{
        PANEL_SCRIPT_FAILURE_CODE, PANEL_SCRIPT_INVALID_CODE, apply_condition_scripts,
        read_script_choice,
    };

    /// One execution, as the engine saw it.
    struct Call {
        args: Value,
        modules: HashMap<String, String>,
    }

    /// An engine that answers with whatever the test queued, and remembers what
    /// it was asked.
    struct FakeEngine {
        answer: Value,
        calls: Mutex<Vec<Call>>,
        fails: bool,
    }

    impl FakeEngine {
        fn answering(answer: Value) -> Self {
            Self {
                answer,
                calls: Mutex::new(Vec::new()),
                fails: false,
            }
        }

        fn failing() -> Self {
            Self {
                answer: json!({}),
                calls: Mutex::new(Vec::new()),
                fails: true,
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls
                .lock()
                .expect("test lock")
                .iter()
                .map(|call| Call {
                    args: call.args.clone(),
                    modules: call.modules.clone(),
                })
                .collect()
        }
    }

    #[async_trait]
    impl SkillScriptEngine for FakeEngine {
        async fn execute(
            &self,
            request: SkillScriptRequest,
        ) -> Result<SkillScriptResult, SkillScriptEngineError> {
            self.calls.lock().expect("test lock").push(Call {
                args: request.args,
                modules: request.modules,
            });
            if self.fails {
                return Err(SkillScriptEngineError::ExecutionFailed {
                    message: "boom".to_string(),
                });
            }
            Ok(SkillScriptResult {
                value: self.answer.clone(),
                writes: Vec::new(),
                last_write_path: None,
                logs: Vec::new(),
            })
        }
    }

    fn condition(value: &str) -> ConditionSpec {
        ConditionSpec {
            source: SOURCE_FIELD.to_string(),
            field: Some("环境/天气".to_string()),
            op: "eq".to_string(),
            value: Some(value.to_string()),
            values: Vec::new(),
            compose: None,
        }
    }

    fn candidate(source: &str, when: Option<ConditionSpec>) -> StateImageCandidate {
        StateImageCandidate {
            source: source.to_string(),
            when,
        }
    }

    /// A declaration whose background would show the fallback under `晴`.
    fn declaration(script: Option<ScriptSpec>) -> StateDeclaration {
        StateDeclaration {
            fields: vec![DeclaredStateField {
                pattern: StateKeyPattern::parse("环境/天气").expect("pattern"),
                label: "WEATHER".to_string(),
                access: StateFieldAccess::DECLARED,
                initial: Vec::new(),
            }],
            panels: StatePanelConfig {
                panels: vec![StatePanelSpec {
                    title: "环境".to_string(),
                    rail: StatePanelRail::Left,
                    match_pattern: StateKeyPattern::parse("环境/*").expect("pattern"),
                    background: Some(StateImageSet {
                        candidates: vec![
                            candidate("/backgrounds/rainy.png", Some(condition("下雨"))),
                            candidate("/backgrounds/day.png", None),
                        ],
                        condition_script: script,
                        ..Default::default()
                    }),
                    fields: Vec::new(),
                    markup: None,
                    prose: None,
                }],
                css: Default::default(),
                scripts: Default::default(),
            },
            machine: None,
            predicates: None,
            limits: Default::default(),
        }
    }

    fn document(values: &[&str]) -> StateDocument {
        StateDocument {
            fields: vec![StateField {
                key: StateKey::parse("环境/天气").expect("key"),
                values: values.iter().map(|value| value.to_string()).collect(),
            }],
        }
    }

    fn script(source: &str) -> Option<ScriptSpec> {
        Some(ScriptSpec {
            script: source.to_string(),
            entry: None,
        })
    }

    async fn resolve(declaration: &StateDeclaration, engine: &dyn SkillScriptEngine) -> Vec<String> {
        let document = document(&["晴"]);
        let mut resolved = declaration.clone();
        let shared = resolved.panels.scripts.clone();
        apply_condition_scripts(&mut resolved, &document.condition_fields(), &shared, engine)
            .await
            .expect("the scripts resolve");
        resolve_panels(
            &resolved,
            &document,
            &resolved.panels,
            &ComparatorRegistry::default(),
        )
        .expect("the panels resolve")
        .into_iter()
        .filter_map(|panel| panel.background)
        .collect()
    }

    #[tokio::test]
    async fn a_condition_script_decides_instead_of_the_conditions() {
        // Under `晴` the conditions would answer with the fallback; the script
        // says otherwise, and the script is what a set that declares one means.
        let engine = FakeEngine::answering(json!({ "index": 0 }));
        let panels = resolve(&declaration(script("export default () => ({ index: 0 });")), &engine).await;

        assert_eq!(panels, vec!["/backgrounds/rainy.png".to_string()]);
    }

    #[tokio::test]
    async fn a_set_without_a_script_still_follows_its_conditions() {
        let engine = FakeEngine::answering(json!({ "index": 0 }));
        let panels = resolve(&declaration(None), &engine).await;

        assert_eq!(panels, vec!["/backgrounds/day.png".to_string()]);
        assert!(
            engine.calls().is_empty(),
            "a set with no script must not call the engine at all"
        );
    }

    #[tokio::test]
    async fn a_script_that_says_no_picture_leaves_the_element_without_one() {
        let engine = FakeEngine::answering(json!({ "index": null }));
        let panels = resolve(&declaration(script("export default () => ({ index: null });")), &engine).await;

        assert!(panels.is_empty());
    }

    #[tokio::test]
    async fn the_script_sees_the_state_and_the_candidates_it_chooses_from() {
        let engine = FakeEngine::answering(json!({ "index": 1 }));
        resolve(&declaration(script("export default () => ({ index: 1 });")), &engine).await;

        let calls = engine.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args["candidates"], json!(["/backgrounds/rainy.png", "/backgrounds/day.png"]));
        assert_eq!(calls[0].args["state"]["环境/天气"], json!(["晴"]));
    }

    #[tokio::test]
    async fn a_choice_past_the_candidates_is_refused() {
        let engine = FakeEngine::answering(json!({ "index": 5 }));
        let error = run_for_error(&declaration(script("export default () => ({ index: 5 });")), &engine).await;

        let message = format!("{error:?}");
        assert!(
            message.contains(PANEL_SCRIPT_INVALID_CODE),
            "an out-of-range choice must be reported, got {message}"
        );
    }

    #[tokio::test]
    async fn a_script_that_fails_stops_the_read() {
        let engine = FakeEngine::failing();
        let error = run_for_error(&declaration(script("throw new Error('boom')")), &engine).await;

        assert!(format!("{error:?}").contains(PANEL_SCRIPT_FAILURE_CODE));
    }

    #[tokio::test]
    async fn a_set_that_declares_a_blank_script_is_refused() {
        let engine = FakeEngine::answering(json!({ "index": 0 }));
        let error = run_for_error(&declaration(script("   ")), &engine).await;

        assert!(format!("{error:?}").contains(PANEL_SCRIPT_INVALID_CODE));
    }

    async fn run_for_error(
        declaration: &StateDeclaration,
        engine: &dyn SkillScriptEngine,
    ) -> crate::errors::ApplicationError {
        let document = document(&["晴"]);
        let mut resolved = declaration.clone();
        let shared = resolved.panels.scripts.clone();
        apply_condition_scripts(&mut resolved, &document.condition_fields(), &shared, engine)
            .await
            .expect_err("the read must stop")
    }

    #[tokio::test]
    async fn a_shared_module_reaches_the_engine_alongside_the_entry() {
        // The whole point of the shared store: an entry script stays one line
        // and the rules live in one module, which therefore has to arrive with
        // the entry it is imported from.
        let mut declaration = declaration(script(
            "import { pick } from './scene.js';\nexport default pick;",
        ));
        declaration
            .panels
            .scripts
            .insert("scene.js".to_string(), "export function pick() { return { index: 1 }; }".to_string());
        let engine = FakeEngine::answering(json!({ "index": 1 }));

        resolve(&declaration, &engine).await;

        let modules = &engine.calls()[0].modules;
        assert_eq!(
            modules.get("scene.js").map(String::as_str),
            Some("export function pick() { return { index: 1 }; }")
        );
        assert!(
            modules.contains_key("hook.js"),
            "the entry module travels too; got {:?}",
            modules.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_script_answer_that_is_not_an_index_is_refused() {
        assert_eq!(read_script_choice(&json!({}), 2).expect("no index means none"), None);
        assert_eq!(
            read_script_choice(&json!({ "index": 1 }), 2).expect("a whole number is a choice"),
            Some(1)
        );
        assert!(
            read_script_choice(&json!({ "index": 1.5 }), 2).is_err(),
            "a fractional index is not a candidate"
        );
        assert!(
            read_script_choice(&json!({ "index": "1" }), 2).is_err(),
            "a string is not an index"
        );
    }

    #[test]
    fn the_condition_fields_map_is_what_the_script_reads() {
        // Guards the shape the script is handed: keys to values, the same map
        // the condition vocabulary compares against.
        let fields: BTreeMap<String, Vec<String>> = document(&["晴", "有风"]).condition_fields();
        assert_eq!(fields.get("环境/天气"), Some(&vec!["晴".to_string(), "有风".to_string()]));
    }
}
