use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{AgentRunPresentation, ArtifactSpec};
use crate::models::state_access::StateAccessPolicy;
use crate::models::tool::{ToolDescriptionOverride, ToolId};

pub const AGENT_PROFILE_SCHEMA_VERSION: u32 = 3;
pub const AGENT_PROFILE_KIND: &str = "tauritavern.agentProfile";
pub const DEFAULT_AGENT_PROFILE_ID: &str = "default-writer";
pub const DEFAULT_AGENT_TOOL_MAX_ROUNDS: usize = 80;
pub const DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN: usize = 80;
/// Newest tool turns that keep their native protocol shape inside a run; older
/// ones fold into plain text. Two is enough to leave the model a live example
/// of the calling convention without paying the shell of every past round.
pub const DEFAULT_AGENT_TOOL_UNFOLDED_TURNS: usize = 2;
pub const DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT: usize = 50_000;
pub const DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL: usize = 20_000;
pub const DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN: usize = 80_000;
pub const DEFAULT_AGENT_MODEL_MAX_RETRIES: usize = 3;
pub const DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS: u64 = 3_000;
pub const DEFAULT_AGENT_INITIAL_CHAT_HISTORY_MESSAGES: i64 = -1;
pub const DEFAULT_AGENT_DELEGATION_MAX_CONCURRENT_INVOCATIONS: usize = 3;
pub const DEFAULT_AGENT_DELEGATION_MAX_INVOCATIONS_PER_RUN: usize = 8;
pub const DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS: usize = 8_000;
pub const DEFAULT_AGENT_HANDOFF_MAX_DEPTH: usize = 8;

fn default_agent_run_direct_runnable() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentProfileId(String);

impl AgentProfileId {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, String> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err("agent.profile_id_empty: profile id cannot be empty".to_string());
        }
        if raw.len() > 128 {
            return Err("agent.profile_id_too_long: profile id must be <= 128 chars".to_string());
        }
        if !raw.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        }) {
            return Err(
                "agent.profile_id_invalid: profile id must use lowercase ASCII, digits, '-' or '_'"
                    .to_string(),
            );
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for AgentProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileSummary {
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_agent_run_direct_runnable")]
    pub direct_runnable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileDefinition {
    pub schema_version: u32,
    pub kind: String,
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preset: AgentPresetBinding,
    pub model: AgentModelBinding,
    pub run: AgentRunPolicy,
    #[serde(default)]
    pub context: AgentContextPolicy,
    #[serde(default)]
    pub delegation: AgentDelegationPolicy,
    #[serde(default)]
    pub instructions: AgentProfileInstructions,
    pub tools: AgentToolPolicy,
    pub skills: AgentSkillPolicy,
    pub workspace: AgentWorkspacePolicy,
    /// Per-field access to the chat's state: inject, retrieve, or write.
    ///
    /// The default is an empty policy, which means "not configured yet" and
    /// constrains nothing — the same reading an empty declaration gets.
    #[serde(default)]
    pub state_access: StateAccessPolicy,
    /// What this Agent does with the chat's recall blocks.
    #[serde(default)]
    pub recall: AgentRecallPolicy,
    pub plan: super::plan::AgentPlanPolicy,
    pub output: AgentOutputPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedAgentProfile {
    pub schema_version: u32,
    pub kind: String,
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preset: AgentPresetBinding,
    pub model: AgentModelBinding,
    pub run: AgentRunPolicy,
    #[serde(default)]
    pub context: AgentContextPolicy,
    #[serde(default)]
    pub delegation: AgentDelegationPolicy,
    pub instructions: AgentProfileInstructions,
    pub tools: ResolvedAgentToolPolicy,
    pub skills: AgentSkillPolicy,
    pub workspace: AgentWorkspacePolicy,
    #[serde(default)]
    pub state_access: StateAccessPolicy,
    #[serde(default)]
    pub recall: AgentRecallPolicy,
    pub plan: super::plan::AgentPlanPolicy,
    pub output: ResolvedAgentOutputPolicy,
    pub source_trace: AgentProfileSourceTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileSourceTrace {
    pub profile_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPresetBinding {
    pub mode: AgentPresetBindingMode,
    #[serde(rename = "ref")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<AgentPresetRef>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentPresetBindingMode {
    CurrentPromptSnapshot,
    Ref,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPresetRef {
    pub api_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelBinding {
    pub mode: AgentModelBindingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentModelBindingMode {
    CurrentPromptSnapshot,
    ConnectionRef,
    RequiresConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRunPolicy {
    pub presentation: AgentRunPresentation,
    #[serde(default)]
    pub stream: bool,
    #[serde(default = "default_agent_run_direct_runnable")]
    pub direct_runnable: bool,
    #[serde(default)]
    pub model_retry: AgentModelRetryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentContextPolicy {
    #[serde(default = "default_agent_initial_chat_history_messages")]
    pub initial_chat_history_messages: i64,
    #[serde(default = "default_agent_include_activated_world_info")]
    pub include_activated_world_info: bool,
    /// Per-entry exceptions to that switch.
    #[serde(default)]
    pub world_info: AgentWorldInfoPolicy,
}

/// Which World Info entries this Agent may be told about.
///
/// The switch above answers for every entry the chat activated; a rule here is an
/// exception for one entry, in both directions. Rows exist because fixed material
/// that suits a world book — a style sheet, a house rule, a format — is awkward
/// anywhere else: a Skill has to be read on purpose, and anything the model
/// summarises on the way in is no longer the text that was written.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorldInfoPolicy {
    #[serde(default)]
    pub entries: Vec<AgentWorldInfoEntryRule>,
    /// Whether a delegated invocation carries the run's activated entries.
    ///
    /// Off by default, and off is what a SubAgent gets today: it starts from a
    /// prompt of its own — a system prompt and the task — so it reads World Info
    /// only if someone says it should. On, it carries them all unless a row below
    /// says otherwise.
    #[serde(default)]
    pub subagent_inherits: bool,
}

/// One entry, named the way the scan names it: its book and its uid.
///
/// A comment is a title a human wrote and may repeat; the pair does not.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorldInfoEntryRule {
    pub book: String,
    pub uid: u64,
    /// Whether the entry is injected for this Agent.
    pub inject: bool,
}

/// What an Agent does with the recall blocks a chat's extensions produced.
///
/// A block is written before a run starts and frozen with the rest of its input,
/// so every invocation of that run reads the same text. That is why nothing here
/// can ask for another recall: the question was already answered against the same
/// index with the same context, and asking again would buy the same answer for
/// another round trip and one more chance for the two to disagree. What is
/// per-Agent is only whether a prompt carries what was recalled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRecallPolicy {
    /// Whether this Agent's own prompt carries the blocks.
    #[serde(default = "default_agent_recall_inject")]
    pub inject: bool,
    /// Which extension prompts count as recall.
    ///
    /// A key, or a prefix when it ends in `*`: one recall extension writes under
    /// several keys (its own tag, one per position group, one per channel), and a
    /// prefix names all of them at once. Only these are the Agent's to place or
    /// drop — everything else an extension injects is that extension's business.
    #[serde(default = "default_agent_recall_sources")]
    pub sources: Vec<String>,
    /// What a delegated invocation does with the blocks its parent already has.
    #[serde(default)]
    pub subagent: AgentRecallInheritance,
}

/// What a delegated invocation does with the recall blocks it inherited.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentRecallInheritance {
    /// Carry them through: the sub-agent is answering about the same story.
    Inherit,
    /// Drop them: the work is narrow, and the parent's recall is noise in it.
    ///
    /// The default, because a delegated task carries what it needs in its own
    /// prompt and because a recall block is the largest thing a sub-agent
    /// inherits that it has no way to use.
    #[default]
    Skip,
}

/// The recall extension this installation is built for, by its prompt tag.
///
/// A default rather than a rule: `sources` is the Agent's to change, and an
/// installation running a different recall extension names it instead.
pub const DEFAULT_RECALL_SOURCE: &str = "3_vectfox*";

fn default_agent_recall_inject() -> bool {
    true
}

fn default_agent_recall_sources() -> Vec<String> {
    vec![DEFAULT_RECALL_SOURCE.to_string()]
}

impl Default for AgentRecallPolicy {
    fn default() -> Self {
        Self {
            inject: default_agent_recall_inject(),
            sources: default_agent_recall_sources(),
            subagent: AgentRecallInheritance::default(),
        }
    }
}

impl AgentRecallPolicy {
    /// Whether that extension prompt key is one of this Agent's recall blocks.
    pub fn matches_source(&self, key: &str) -> bool {
        let key = key.trim();
        !key.is_empty()
            && self
                .sources
                .iter()
                .any(|source| matches_source(source, key))
    }
}

/// A source that ends in `*` is a prefix; anything else is the whole key.
pub fn matches_source(source: &str, key: &str) -> bool {
    let source = source.trim();
    match source.strip_suffix('*') {
        Some(prefix) => key.starts_with(prefix),
        None => key == source,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDelegationPolicy {
    #[serde(default)]
    pub can_delegate: bool,
    #[serde(default)]
    pub can_handoff: bool,
    #[serde(default)]
    pub callable: bool,
    #[serde(default)]
    pub allow_as_subagent: bool,
    #[serde(default)]
    pub allow_as_handoff_target: bool,
    #[serde(default)]
    pub allow_nested_delegation: bool,
    #[serde(default = "default_agent_delegation_allowed_callers")]
    pub allowed_callers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_for_agents: Option<String>,
    #[serde(default = "default_agent_delegation_max_concurrent_invocations")]
    pub max_concurrent_invocations: usize,
    #[serde(default = "default_agent_delegation_max_invocations_per_run")]
    pub max_invocations_per_run: usize,
    #[serde(default = "default_agent_delegation_result_budget_tokens")]
    pub result_budget_tokens: usize,
    #[serde(default = "default_agent_handoff_max_depth")]
    pub max_handoff_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelRetryPolicy {
    #[serde(default = "default_agent_model_max_retries")]
    pub max_retries: usize,
    #[serde(default = "default_agent_model_retry_interval_ms")]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileInstructions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(
        serialize = "T: Serialize + Ord",
        deserialize = "T: Deserialize<'de> + Ord"
    )
)]
pub struct AgentToolPolicy<T = String> {
    pub allow: Vec<T>,
    #[serde(default)]
    pub deny: Vec<T>,
    #[serde(default)]
    pub tool_descriptions: BTreeMap<T, ToolDescriptionOverride>,
    pub max_rounds: usize,
    #[serde(default = "default_agent_tool_max_calls_per_run")]
    pub max_calls_per_run: usize,
    #[serde(default = "default_agent_mcp_result_inline_char_limit")]
    pub mcp_result_inline_char_limit: usize,
    /// How many of the newest tool turns stay in native protocol format when
    /// an in-run transcript is compacted. `1` is the effective floor: folding
    /// the newest turn would leave tool calls unanswered.
    #[serde(default = "default_agent_tool_unfolded_turns")]
    pub unfolded_tool_turns: usize,
    #[serde(default)]
    pub max_calls_per_tool: BTreeMap<T, usize>,
}

pub type ResolvedAgentToolPolicy = AgentToolPolicy<ToolId>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSkillPolicy {
    pub visible: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default = "default_agent_skill_max_read_chars_per_call")]
    pub max_read_chars_per_call: usize,
    #[serde(default = "default_agent_skill_max_read_chars_per_run")]
    pub max_read_chars_per_run: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorkspacePolicy {
    pub visible_roots: Vec<String>,
    pub writable_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOutputPolicy {
    pub artifacts: Vec<AgentOutputArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedAgentOutputPolicy {
    pub artifacts: Vec<ArtifactSpec>,
    pub message_body_artifact_id: String,
    pub message_body_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOutputArtifact {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub target: AgentOutputArtifactTarget,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub assembly_order: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentOutputArtifactTarget {
    MessageBody,
}

impl AgentProfileDefinition {
    pub fn summary(&self) -> AgentProfileSummary {
        AgentProfileSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            direct_runnable: self.run.direct_runnable,
        }
    }
}

fn default_agent_tool_max_calls_per_run() -> usize {
    DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN
}

fn default_agent_mcp_result_inline_char_limit() -> usize {
    DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT
}

fn default_agent_tool_unfolded_turns() -> usize {
    DEFAULT_AGENT_TOOL_UNFOLDED_TURNS
}

fn default_agent_skill_max_read_chars_per_call() -> usize {
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL
}

fn default_agent_skill_max_read_chars_per_run() -> usize {
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN
}

fn default_agent_model_max_retries() -> usize {
    DEFAULT_AGENT_MODEL_MAX_RETRIES
}

fn default_agent_model_retry_interval_ms() -> u64 {
    DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS
}

fn default_agent_initial_chat_history_messages() -> i64 {
    DEFAULT_AGENT_INITIAL_CHAT_HISTORY_MESSAGES
}

fn default_agent_include_activated_world_info() -> bool {
    true
}

fn default_agent_delegation_allowed_callers() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_agent_delegation_max_concurrent_invocations() -> usize {
    DEFAULT_AGENT_DELEGATION_MAX_CONCURRENT_INVOCATIONS
}

fn default_agent_delegation_max_invocations_per_run() -> usize {
    DEFAULT_AGENT_DELEGATION_MAX_INVOCATIONS_PER_RUN
}

fn default_agent_delegation_result_budget_tokens() -> usize {
    DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS
}

fn default_agent_handoff_max_depth() -> usize {
    DEFAULT_AGENT_HANDOFF_MAX_DEPTH
}

impl Default for AgentModelRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_AGENT_MODEL_MAX_RETRIES,
            interval_ms: DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS,
        }
    }
}

impl Default for AgentContextPolicy {
    fn default() -> Self {
        Self {
            initial_chat_history_messages: DEFAULT_AGENT_INITIAL_CHAT_HISTORY_MESSAGES,
            include_activated_world_info: true,
            world_info: AgentWorldInfoPolicy::default(),
        }
    }
}

impl Default for AgentDelegationPolicy {
    fn default() -> Self {
        Self {
            can_delegate: false,
            can_handoff: false,
            callable: false,
            allow_as_subagent: false,
            allow_as_handoff_target: false,
            allow_nested_delegation: false,
            allowed_callers: default_agent_delegation_allowed_callers(),
            description_for_agents: None,
            max_concurrent_invocations: DEFAULT_AGENT_DELEGATION_MAX_CONCURRENT_INVOCATIONS,
            max_invocations_per_run: DEFAULT_AGENT_DELEGATION_MAX_INVOCATIONS_PER_RUN,
            result_budget_tokens: DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS,
            max_handoff_depth: DEFAULT_AGENT_HANDOFF_MAX_DEPTH,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AgentProfileDefinition, DEFAULT_AGENT_DELEGATION_MAX_CONCURRENT_INVOCATIONS,
        DEFAULT_AGENT_DELEGATION_MAX_INVOCATIONS_PER_RUN,
        DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS, DEFAULT_AGENT_HANDOFF_MAX_DEPTH,
        DEFAULT_AGENT_INITIAL_CHAT_HISTORY_MESSAGES, DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT,
        DEFAULT_AGENT_MODEL_MAX_RETRIES, DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS,
        DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL, DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN,
        DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN, DEFAULT_AGENT_TOOL_UNFOLDED_TURNS,
    };
    use crate::models::agent::plan::DEFAULT_AGENT_PLAN_BETA;

    #[test]
    fn profile_definition_applies_schema_defaults_for_optional_fields() {
        assert_eq!(DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS, 8_000);
        assert_eq!(DEFAULT_AGENT_HANDOFF_MAX_DEPTH, 8);

        let profile: AgentProfileDefinition = serde_json::from_value(json!({
            "schemaVersion": 1,
            "kind": "tauritavern.agentProfile",
            "id": "minimal",
            "displayName": "Minimal",
            "preset": {
                "mode": "currentPromptSnapshot"
            },
            "model": {
                "mode": "currentPromptSnapshot"
            },
            "run": {
                "presentation": "foreground"
            },
            "tools": {
                "allow": ["workspace.write_file", "workspace.commit", "workspace.finish"],
                "maxRounds": 80
            },
            "skills": {
                "visible": ["*"]
            },
            "workspace": {
                "visibleRoots": ["output"],
                "writableRoots": ["output"]
            },
            "plan": {
                "mode": "none"
            },
            "output": {
                "artifacts": [{
                    "id": "main",
                    "path": "output/main.md",
                    "kind": "markdown",
                    "target": "messageBody"
                }]
            }
        }))
        .expect("profile with optional fields omitted");

        assert!(!profile.preset.required);
        assert!(!profile.run.stream);
        assert!(profile.run.direct_runnable);
        assert_eq!(
            profile.run.model_retry.max_retries,
            DEFAULT_AGENT_MODEL_MAX_RETRIES
        );
        assert_eq!(
            profile.run.model_retry.interval_ms,
            DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS
        );
        assert_eq!(
            profile.context.initial_chat_history_messages,
            DEFAULT_AGENT_INITIAL_CHAT_HISTORY_MESSAGES
        );
        assert!(profile.context.include_activated_world_info);
        assert!(!profile.delegation.can_delegate);
        assert!(!profile.delegation.can_handoff);
        assert!(!profile.delegation.callable);
        assert!(!profile.delegation.allow_as_subagent);
        assert!(!profile.delegation.allow_as_handoff_target);
        assert!(!profile.delegation.allow_nested_delegation);
        assert_eq!(profile.delegation.allowed_callers, vec!["*"]);
        assert!(profile.delegation.description_for_agents.is_none());
        assert_eq!(
            profile.delegation.max_concurrent_invocations,
            DEFAULT_AGENT_DELEGATION_MAX_CONCURRENT_INVOCATIONS
        );
        assert_eq!(
            profile.delegation.max_invocations_per_run,
            DEFAULT_AGENT_DELEGATION_MAX_INVOCATIONS_PER_RUN
        );
        assert_eq!(
            profile.delegation.result_budget_tokens,
            DEFAULT_AGENT_DELEGATION_RESULT_BUDGET_TOKENS
        );
        assert_eq!(
            profile.delegation.max_handoff_depth,
            DEFAULT_AGENT_HANDOFF_MAX_DEPTH
        );
        assert!(profile.instructions.agent_system_prompt.is_none());
        assert!(profile.tools.deny.is_empty());
        assert!(profile.tools.tool_descriptions.is_empty());
        assert_eq!(
            profile.tools.max_calls_per_run,
            DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN
        );
        assert_eq!(
            profile.tools.mcp_result_inline_char_limit,
            DEFAULT_AGENT_MCP_RESULT_INLINE_CHAR_LIMIT
        );
        assert!(profile.tools.max_calls_per_tool.is_empty());
        assert_eq!(
            profile.tools.unfolded_tool_turns,
            DEFAULT_AGENT_TOOL_UNFOLDED_TURNS
        );
        assert!(profile.skills.deny.is_empty());
        assert_eq!(
            profile.skills.max_read_chars_per_call,
            DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL
        );
        assert_eq!(
            profile.skills.max_read_chars_per_run,
            DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN
        );
        assert_eq!(profile.plan.beta, DEFAULT_AGENT_PLAN_BETA);
        assert!(profile.plan.nodes.is_empty());
    }
}
