use tt_domain::models::agent::ArtifactTarget;

/// 跨运行保留的记事根。判定可见性 / 可写性时用它，不再写字面量。
pub(super) const PERSISTENT_ROOT: &str = "persist";
/// 状态根：每楼一份聚合状态文档，只能经 `state.update` 改写。
///
/// `pub(crate)`：技能脚本要用它把状态根从自己的工作区视图里排除。
pub(crate) const STATE_ROOT: &str = "state";

pub(super) const WORKSPACE_ROOT_UNIVERSE: [&str; 6] =
    ["output", "scratch", "plan", "summaries", PERSISTENT_ROOT, STATE_ROOT];

/// 必须在 `WORKSPACE_ROOT_UNIVERSE` 内。这些根在 Run 结束时发布为持久版本，并按聊天绑定到消息。
pub(super) const PERSISTENT_WORKSPACE_ROOTS: [&str; 2] = [PERSISTENT_ROOT, STATE_ROOT];
pub(super) const MESSAGE_BODY_ARTIFACT_TARGET: ArtifactTarget = ArtifactTarget::MessageBody;
pub(super) const AGENT_AWAIT_TOOL: &str = "agent.await";
pub(super) const AGENT_DELEGATE_TOOL: &str = "agent.delegate";
pub(super) const AGENT_HANDOFF_TOOL: &str = "agent.handoff";
pub(super) const AGENT_LIST_TOOL: &str = "agent.list";
pub(super) const TASK_RETURN_TOOL: &str = "task.return";
