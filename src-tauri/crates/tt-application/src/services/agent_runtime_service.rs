use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, oneshot, watch};

use crate::dto::agent_dto::{
    AgentListToolsResultDto, AgentPromptAssemblyBrokerRequestDto, AgentToolCatalogDiagnosticDto,
    AgentToolCatalogItemDto,
};
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::AgentModelGateway;
use crate::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, materialize_agent_system_prompt,
};
use crate::services::agent_tools::{
    AgentToolDispatcher, BuiltinAgentToolRegistry, compile_invocation_tool_snapshot,
    project_agent_model_tools,
};
use crate::services::llm_connection_service::LlmConnectionService;
use crate::services::mcp_service::{McpModelToolDiagnostic, McpService};
use crate::services::prompt_assembly_service::PromptAssemblyService;
use crate::services::recall_service::RecallService;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentInvocation, AgentInvocationExitPolicy, AgentModelRequest, AgentModelTool,
};
use tt_domain::models::skill::SkillIndexEntry;
use tt_domain::models::tool::{
    InvocationToolSnapshot, ToolCatalog, ToolChoice, ToolSnapshotId, ToolTurnContract,
};
use tt_ports::repositories::agent_invocation_repository::AgentInvocationRepository;
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::tokenizer_repository::TokenizerRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::skill_script::SkillScriptEngine;

mod artifacts;

mod checkpoint;
mod commit;
mod commit_ledger;
mod continuation;
mod delegation;
mod error_payload;
mod executor;
mod guidance;
mod input_context;
mod invocation;
mod journal;
mod lifecycle;
mod loop_runner;
mod markdown;
mod model_response_store;
mod model_retry;
mod model_stream_projection;
mod model_turn_display;
mod prompt_assembly;
mod prompt_snapshot;
pub(crate) mod recall;
pub(crate) mod world_info;
mod revision;
mod scheduler;
mod skill_scope;
mod state_edit;
mod state_injection;
mod state_machine;
mod state_panel;
mod state_predicate;
mod task_details;
mod timeline_projection;

use crate::services::state_runtime::resolve_state_value_measure;
use tt_domain::models::state::{CostUnit, StateDeclaration, count_chars};
mod tool_execution;
mod tool_snapshot;

#[cfg(test)]
mod tests;

pub use model_stream_projection::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, AgentRunLiveReasoning,
    ModelAttemptGeneration, ToolCallProjection,
};
pub use state_edit::{StateEditDto, StateProseEditDto};
pub use state_injection::StateInjectionDto;
pub use state_panel::StatePanelDto;
pub use state_predicate::StatePredicateEntryDto;
use scheduler::ActiveRunHandle;

pub(super) type AgentCancelReceiver = watch::Receiver<bool>;

pub(super) struct PendingHostChatCommit {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<HostChatCommitResult, String>>,
}

pub(super) struct HostChatCommitResult {
    pub(super) message_id: Option<String>,
}

pub(super) struct PendingHostPromptAssembly {
    pub(super) run_id: String,
    pub(super) request: AgentPromptAssemblyBrokerRequestDto,
    pub(super) sender: oneshot::Sender<Result<HostPromptAssemblyResult, String>>,
}

pub(super) struct HostPromptAssemblyResult {
    pub(super) prompt_snapshot: serde_json::Value,
    pub(super) frozen_run_input_snapshot: Option<serde_json::Value>,
    pub(super) generation_intent: Option<serde_json::Value>,
    pub(super) assembly: Option<serde_json::Value>,
}

pub(super) struct PendingPersistentStateMetadataUpdate {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<(), String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparedInvocation {
    #[serde(skip)]
    frozen_macros: Arc<tt_domain::frozen_macros::FrozenMacros>,
    invocation: AgentInvocation,
    delegation_task_id: Option<String>,
    profile: ResolvedAgentProfile,
    tool_snapshot: InvocationToolSnapshot,
    tool_turn: ToolTurnContract,
    request: AgentModelRequest,
    effective_skills: Vec<SkillIndexEntry>,
}

struct PreparedInvocationTools {
    snapshot: InvocationToolSnapshot,
    turn: ToolTurnContract,
    model_tools: Vec<AgentModelTool>,
    diagnostics: Vec<McpModelToolDiagnostic>,
}

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    invocation_repository: Arc<dyn AgentInvocationRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    model_gateway: Arc<dyn AgentModelGateway>,
    profile_service: Arc<AgentProfileService>,
    llm_connection_service: Arc<LlmConnectionService>,
    prompt_assembly_service: Arc<PromptAssemblyService>,
    skill_service: Arc<SkillService>,
    mcp_service: Arc<McpService>,
    script_engine: Arc<dyn SkillScriptEngine>,
    recall_service: Arc<RecallService>,
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, Arc<ActiveRunHandle>>>,
    run_lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    /// Serializes the read-modify-write behind `transition_status`.
    ///
    /// Separate from `run_lifecycle_lock`, which the cancel path already holds
    /// when it writes a status: a tokio mutex is not reentrant, and taking the
    /// same one twice would deadlock.
    run_status_lock: tokio::sync::Mutex<()>,
    active_chat_commits: RwLock<HashMap<String, PendingHostChatCommit>>,
    active_prompt_assemblies: RwLock<HashMap<String, PendingHostPromptAssembly>>,
    active_persistent_state_metadata_updates:
        RwLock<HashMap<String, PendingPersistentStateMetadataUpdate>>,
    /// The vocabulary a scene counts its values with, when it asks for tokens.
    ///
    /// Set once by the composition root rather than arriving through `new`:
    /// counting in tokens is one scene's option, and every construction site —
    /// the app and each contract test — would otherwise have to name a
    /// tokenizer it never uses. Absent means token ceilings cannot be honoured,
    /// which is reported rather than papered over.
    state_tokenizer: Arc<std::sync::OnceLock<Arc<dyn TokenizerRepository>>>,
}

impl AgentRuntimeService {
    #[expect(
        clippy::too_many_arguments,
        reason = "composition boundary keeps concrete runtime dependencies explicit"
    )]
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        invocation_repository: Arc<dyn AgentInvocationRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
        llm_connection_service: Arc<LlmConnectionService>,
        prompt_assembly_service: Arc<PromptAssemblyService>,
        mcp_service: Arc<McpService>,
        skill_script_engine: Arc<dyn SkillScriptEngine>,
        recall_service: Arc<RecallService>,
    ) -> Self {
        let tool_registry = BuiltinAgentToolRegistry::all();
        // One cell, two readers: a tool writes state through the dispatcher, and
        // the completion pass writes it through this service, and both have to
        // count a value the same way.
        let state_tokenizer: Arc<std::sync::OnceLock<Arc<dyn TokenizerRepository>>> =
            Arc::new(std::sync::OnceLock::new());
        let tool_dispatcher = AgentToolDispatcher::new(
            run_repository.clone(),
            chat_repository.clone(),
            group_chat_repository.clone(),
            workspace_repository.clone(),
            skill_service.clone(),
            skill_script_engine.clone(),
            Arc::clone(&state_tokenizer),
        );
        Self {
            run_repository,
            invocation_repository,
            workspace_repository,
            chat_repository,
            group_chat_repository,
            model_gateway,
            profile_service,
            llm_connection_service,
            prompt_assembly_service,
            skill_service,
            mcp_service,
            // The panel reads state and runs a picture set's condition script;
            // that script runs in the same sandbox the skills and the machine
            // hooks use, so there is one place a script can execute.
            script_engine: skill_script_engine,
            recall_service,
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
            run_lifecycle_lock: Arc::new(tokio::sync::Mutex::new(())),
            run_status_lock: tokio::sync::Mutex::new(()),
            active_chat_commits: RwLock::new(HashMap::new()),
            active_prompt_assemblies: RwLock::new(HashMap::new()),
            active_persistent_state_metadata_updates: RwLock::new(HashMap::new()),
            state_tokenizer,
        }
    }

    /// Hand the runtime the vocabulary a scene may count its values with.
    ///
    /// Called once by the composition root. A second call is ignored rather than
    /// an error: the value is a capability, and the first answer is the one
    /// every write is already using.
    pub fn set_state_tokenizer(&self, tokenizer: Arc<dyn TokenizerRepository>) {
        let _ = self.state_tokenizer.set(tokenizer);
    }

    /// The vocabulary, when the host wired one in.
    pub(crate) fn state_tokenizer(&self) -> Option<&Arc<dyn TokenizerRepository>> {
        self.state_tokenizer.get()
    }

    /// Check a declaration's initial values in the unit its scene counts in.
    ///
    /// Run before a declaration is stored. An initial value is seeded into the
    /// document and injected from there, so one over the scene's ceiling would
    /// never pass a write and would never be noticed either — which is why this
    /// check cannot be left to the character rule the declaration service can do
    /// on its own.
    pub async fn validate_state_initial_values(
        &self,
        declaration: &StateDeclaration,
        tokenizer_model: Option<&str>,
    ) -> Result<(), ApplicationError> {
        let mut errors = declaration.initial_value_shape_errors();

        let has_initial_values = declaration
            .fields
            .iter()
            .any(|field| !field.initial.is_empty());
        if declaration.limits.unit == CostUnit::Chars {
            errors.extend(declaration.initial_value_ceiling_errors(&|value| Ok(count_chars(value))));
        } else if has_initial_values {
            // Only a scene that carries initial values has something that cannot
            // be checked without a vocabulary, so only that scene hears about a
            // missing one. Every other token scene saves without naming a
            // model, and its writes are measured when they happen.
            let measure =
                resolve_state_value_measure(declaration, tokenizer_model, self.state_tokenizer())
                    .await?;
            errors.extend(declaration.initial_value_ceiling_errors(&|value| measure.count(value)));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ApplicationError::ValidationError(format!(
                "state_declaration.invalid_initial: {}",
                errors.join("; ")
            )))
        }
    }

    pub fn run_lifecycle_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.run_lifecycle_lock)
    }

    pub fn tool_catalog(&self) -> &ToolCatalog {
        self.tool_registry.catalog()
    }

    pub async fn tool_catalog_items(&self) -> Result<AgentListToolsResultDto, ApplicationError> {
        let mut tools = self
            .tool_registry
            .catalog()
            .iter()
            .map(|descriptor| {
                let title = descriptor.title.clone().ok_or_else(|| {
                    ApplicationError::InternalError(format!(
                        "agent.tool_title_required: builtin tool `{}` has no title",
                        descriptor.id
                    ))
                })?;
                let description = descriptor.description.clone().ok_or_else(|| {
                    ApplicationError::InternalError(format!(
                        "agent.tool_description_required: builtin tool `{}` has no description",
                        descriptor.id
                    ))
                })?;
                Ok(AgentToolCatalogItemDto {
                    id: descriptor.id.clone(),
                    native_name: descriptor.id.native_name().to_string(),
                    title,
                    description,
                    input_schema: descriptor.input_schema.clone(),
                    output_schema: descriptor.output_schema.clone(),
                    annotations: descriptor.annotations.clone(),
                    source: "builtin".to_string(),
                    registration_id: None,
                    server_display_name: None,
                    permission: None,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        let mcp = self.mcp_service.list_permitted_model_tools_cached().await?;
        tools.extend(mcp.tools.into_iter().map(|tool| {
            AgentToolCatalogItemDto {
                id: tool.descriptor.id.clone(),
                native_name: tool.descriptor.id.native_name().to_string(),
                title: tool
                    .descriptor
                    .title
                    .clone()
                    .unwrap_or_else(|| tool.descriptor.id.native_name().to_string()),
                description: tool.descriptor.description.clone().unwrap_or_default(),
                input_schema: tool.descriptor.input_schema.clone(),
                output_schema: tool.descriptor.output_schema.clone(),
                annotations: tool.descriptor.annotations.clone(),
                source: "mcp".to_string(),
                registration_id: Some(tool.registration_id.to_string()),
                server_display_name: Some(tool.server_display_name),
                permission: Some(tool.permission),
            }
        }));
        Ok(AgentListToolsResultDto {
            tools,
            diagnostics: mcp
                .diagnostics
                .into_iter()
                .map(|diagnostic| AgentToolCatalogDiagnosticDto {
                    tool_id: diagnostic.tool_id,
                    code: diagnostic.code,
                    message: diagnostic.message,
                })
                .collect(),
        })
    }

    pub async fn visible_model_tools(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> Result<Vec<AgentModelTool>, ApplicationError> {
        Ok(self
            .prepare_invocation_tools(
                profile,
                AgentInvocationExitPolicy::RunFinishAllowed,
                "profile_preview",
            )
            .await?
            .model_tools)
    }

    async fn prepare_invocation_tools(
        &self,
        profile: &ResolvedAgentProfile,
        exit_policy: AgentInvocationExitPolicy,
        snapshot_id: &str,
    ) -> Result<PreparedInvocationTools, ApplicationError> {
        let selected = profile
            .tools
            .allow
            .iter()
            .filter(|id| !id.is_builtin() && !profile.tools.deny.iter().any(|denied| denied == *id))
            .cloned()
            .collect::<Vec<_>>();
        let mut mcp = self
            .mcp_service
            .resolve_permitted_model_tools_cached(&selected)
            .await?;
        let mut override_diagnostics = Vec::new();
        mcp.tools.retain_mut(|tool| {
            let Some(override_) = profile.tools.tool_descriptions.get(&tool.descriptor.id) else {
                return true;
            };
            match tool.descriptor.apply_description_override(override_) {
                Ok(()) => true,
                Err(error) => {
                    override_diagnostics.push(McpModelToolDiagnostic {
                        tool_id: Some(tool.descriptor.id.clone()),
                        code: "mcp.agent_tool_override_invalid".to_string(),
                        message: error.to_string(),
                    });
                    false
                }
            }
        });
        mcp.diagnostics.extend(override_diagnostics);
        let snapshot = compile_invocation_tool_snapshot(
            &self.tool_registry,
            profile,
            exit_policy,
            ToolSnapshotId::parse(snapshot_id.to_string())?,
            &mcp.tools,
        )?;
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto)?;
        let model_tools = project_agent_model_tools(&snapshot, &turn)?;
        Ok(PreparedInvocationTools {
            snapshot,
            turn,
            model_tools,
            diagnostics: mcp.diagnostics,
        })
    }

    pub async fn resolve_agent_system_prompt(
        &self,
        profile_id: Option<&str>,
    ) -> Result<String, ApplicationError> {
        let profile = self
            .profile_service
            .resolve_profile_for_preview(AgentProfileResolveInput {
                profile_id,
                tool_catalog: self.tool_registry.catalog(),
            })
            .await?;
        let visible_tools = self.visible_model_tools(&profile).await?;

        Ok(materialize_agent_system_prompt(&visible_tools, &profile))
    }
}
