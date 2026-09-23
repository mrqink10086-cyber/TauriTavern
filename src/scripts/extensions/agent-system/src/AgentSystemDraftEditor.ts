import { clone, prettyJson } from './host-api';
import {
    defaultProfile,
    normalizeProfileForSave,
    profileForEdit,
    type AgentProfileDraft,
    type AgentProfileDraftNumber,
} from './profile-model';
import { recallControls, type RecallField } from './profile-recall';
import { worldInfoControls } from './profile-context-world-info';
import {
    applyAddStateAccessRow,
    applyAddStateAccessRows,
    applyCallableAsHandoffTarget,
    applyCallableAsSubAgent,
    applyCanDelegate,
    applyCanHandoff,
    applyModelMode,
    applyModelTarget,
    applyPresetMode,
    applyPresetName,
    applyRemoveStateAccessRow,
    applyResetToolDescriptionOverride,
    applyResetToolPropertyDescriptionOverride,
    applyRunPresentation,
    applyToolAllowed,
    applyToolDescriptionOverride,
    applyToolPropertyDescriptionOverride,
    applyUpdateStateAccessRow,
    applyWorkspaceRootVisible,
    applyWorkspaceRootWritable,
    nextProfileId,
    profilePresentationMemoryKey,
    type AgentPresentationMemory,
} from './profile-draft-ops';
import type { StateAccessRow } from './profile-state-access';
import type { ChatStateDeclaration } from './chat-state-declaration';
import { splitKeyPatterns } from './state-config-model';
import {
    isBuiltinProfile,
    type AgentSystemPanelControllerDeps,
    type AgentSystemPanelSnapshot,
} from './AgentSystemPanelContract';
import { toolHasDescriptionOverride } from './AgentSystemPanelView';

/**
 * Shared seam between the panel session controller and the draft editor:
 * everything the editor needs from session state, nothing more.
 */
export type AgentSystemDraftEditorContext = {
    deps: AgentSystemPanelControllerDeps;
    getSnapshot: () => AgentSystemPanelSnapshot;
    commit: (patch: Partial<AgentSystemPanelSnapshot>) => void;
    isDisposed: () => boolean;
    presentationMemory: AgentPresentationMemory;
    seedMainAgentPresentation: (draft: AgentProfileDraft) => void;
    editModeSyncPatch: (
        draft: AgentProfileDraft,
    ) => Pick<AgentSystemPanelSnapshot, 'profileEditMode' | 'activeProfileSectionId'>;
    toolIdsForDraft: (draft: AgentProfileDraft) => string[];
    catalogToolIds: () => string[];
    clearedRuntimeState: () => Pick<AgentSystemPanelSnapshot,
        'resolvedAgentSystemPrompt' | 'profilePreviewError' | 'profileHealth' | 'profileDiagnosticError' | 'profileRuntimeStateJson'>;
    bumpProfileSelectionToken: () => void;
    resetLastLoadedProfileJson: () => void;
};

export type AgentSystemDraftEditor = {
    setIdentityField: (field: 'id' | 'displayName' | 'description', value: string) => void;
    setAgentSystemPrompt: (value: string) => void;
    setPresetMode: (mode: string) => void;
    setPresetName: (name: string) => void;
    setModelMode: (mode: string) => void;
    setModelTarget: (targetId: string) => void;
    setCanDelegate: (enabled: boolean) => void;
    setCanHandoff: (enabled: boolean) => void;
    setRunPresentation: (presentation: string) => void;
    setRunStream: (enabled: boolean) => void;
    setCallableAsSubAgent: (enabled: boolean) => void;
    setCallableAsHandoffTarget: (enabled: boolean) => void;
    setDelegationDescription: (value: string) => void;
    setAllowedCallersCsv: (value: string) => void;
    setDelegationLimit: (
        field: 'maxConcurrentInvocations' | 'maxInvocationsPerRun' | 'maxHandoffDepth',
        value: AgentProfileDraftNumber,
    ) => void;
    setPlanMode: (mode: string) => void;
    setToolsLimitField: (
        field: 'maxRounds' | 'maxCallsPerRun' | 'mcpResultInlineCharLimit' | 'unfoldedToolTurns',
        value: AgentProfileDraftNumber,
    ) => void;
    setModelRetryField: (field: 'maxRetries' | 'intervalMs', value: AgentProfileDraftNumber) => void;
    setContextHistoryMessages: (value: AgentProfileDraftNumber) => void;
    setContextIncludeWorldInfo: (checked: boolean) => void;
    setSkillsCsvField: (field: 'visibleCsv' | 'denyCsv', value: string) => void;
    setSkillsLimitField: (field: 'maxReadCharsPerCall' | 'maxReadCharsPerRun', value: AgentProfileDraftNumber) => void;
    setWorkspaceRootVisible: (root: string, visible: boolean) => void;
    setWorkspaceRootWritable: (root: string, writable: boolean) => void;
    /** Add one row, or one row per key pattern in a pasted comma-separated list. */
    addStateAccessRows: (patternsCsv?: string) => void;
    expandStateAccessFromDeclaration: () => Promise<void>;
    expandStateAccessFromChatDeclaration: () => Promise<void>;
    removeStateAccessRow: (index: number) => void;
    updateStateAccessRow: (index: number, patch: Partial<StateAccessRow>) => void;
    setRecallField: (field: RecallField, value: boolean | string) => void;
    setWorldInfoSubagentInherits: (inherits: boolean) => void;
    setWorldInfoEntry: (entry: { world?: unknown; uid?: unknown }, carried: boolean) => void;
    clearWorldInfoRules: () => void;
    setOutputArtifactField: (field: 'path' | 'kind', value: string) => void;
    selectTool: (toolId: string) => void;
    toggleToolAllowed: (toolId: string, enabled: boolean) => Promise<void>;
    setToolDescriptionOverride: (toolId: string, value: string) => void;
    setToolPropertyDescriptionOverride: (toolId: string, property: string, value: string) => void;
    resetToolDescriptionOverride: (toolId: string) => void;
    resetToolPropertyDescriptionOverride: (toolId: string, property: string) => void;
    setDraftJson: (value: string) => void;
    refreshDraftJson: () => void;
    applyDraftJson: () => void;
    newProfile: () => void;
    copyProfile: () => void;
};

/**
 * All profile draft edits. Every action clones the current draft, applies a
 * pure operation and commits one new immutable snapshot; binding-affecting
 * edits additionally invalidate the displayed runtime state.
 */
export function createAgentSystemDraftEditor(context: AgentSystemDraftEditorContext): AgentSystemDraftEditor {
    const { deps } = context;

    function memoryKey(draft: AgentProfileDraft): string {
        return profilePresentationMemoryKey(draft.id, context.getSnapshot().editingProfileId);
    }

    function editDraft(mutate: (draft: AgentProfileDraft) => void): void {
        const draft = clone(context.getSnapshot().draft);
        mutate(draft);
        context.commit({ draft });
    }

    function editDraftInvalidatingRuntimeState(mutate: (draft: AgentProfileDraft) => void): void {
        const draft = clone(context.getSnapshot().draft);
        mutate(draft);
        context.bumpProfileSelectionToken();
        context.commit({ draft, ...context.clearedRuntimeState() });
    }

    function isBuiltin(): boolean {
        return isBuiltinProfile(context.getSnapshot().draft);
    }

    /**
     * Append one access row per pattern. An empty list adds one blank row, so a
     * click on "Add Row" still produces something to edit.
     */
    function appendStateAccessRows(patterns: readonly string[]): void {
        editDraft((draft) => {
            if (patterns.length === 0) {
                applyAddStateAccessRow(draft);
                return;
            }
            applyAddStateAccessRows(draft, [...patterns]);
        });
    }

    function replaceDraft(draft: AgentProfileDraft, editingProfileId: string): void {
        context.resetLastLoadedProfileJson();
        context.seedMainAgentPresentation(draft);
        context.bumpProfileSelectionToken();
        context.commit({
            editingProfileId,
            draft,
            externalProfileChangePending: false,
            ...context.editModeSyncPatch(draft),
            draftJson: prettyJson(normalizeProfileForSave(draft)),
            ...context.clearedRuntimeState(),
        });
    }

    return {
        setIdentityField(field, value) {
            editDraft((draft) => {
                draft[field] = value;
            });
        },
        setAgentSystemPrompt(value) {
            if (isBuiltin()) {
                return;
            }
            editDraftInvalidatingRuntimeState((draft) => {
                draft.instructions.agentSystemPrompt = value;
            });
        },
        setPresetMode(mode) {
            if (isBuiltin()) {
                return;
            }
            editDraftInvalidatingRuntimeState((draft) => {
                applyPresetMode(draft, mode, context.getSnapshot().presetOptions);
            });
        },
        setPresetName(name) {
            if (isBuiltin()) {
                return;
            }
            editDraftInvalidatingRuntimeState((draft) => {
                applyPresetName(draft, name);
            });
        },
        setModelMode(mode) {
            if (isBuiltin()) {
                return;
            }
            editDraftInvalidatingRuntimeState((draft) => {
                applyModelMode(draft, mode, context.getSnapshot().modelTargets);
            });
        },
        setModelTarget(targetId) {
            if (isBuiltin()) {
                return;
            }
            editDraftInvalidatingRuntimeState((draft) => {
                applyModelTarget(draft, context.getSnapshot().modelTargets, targetId);
            });
        },
        setCanDelegate(enabled) {
            if (isBuiltin()) {
                return;
            }
            editDraft((draft) => {
                applyCanDelegate(draft, enabled, context.catalogToolIds());
            });
        },
        setCanHandoff(enabled) {
            if (isBuiltin()) {
                return;
            }
            editDraft((draft) => {
                applyCanHandoff(draft, enabled, context.catalogToolIds());
            });
        },
        setRunPresentation(presentation) {
            if (isBuiltin()) {
                return;
            }
            editDraft((draft) => {
                applyRunPresentation(draft, presentation, context.presentationMemory, memoryKey(draft));
            });
        },
        setRunStream(enabled) {
            if (isBuiltin()) {
                return;
            }
            editDraft((draft) => {
                draft.run.stream = enabled;
            });
        },
        setCallableAsSubAgent(enabled) {
            if (isBuiltin()) {
                return;
            }
            const draft = clone(context.getSnapshot().draft);
            applyCallableAsSubAgent(draft, enabled, context.presentationMemory, memoryKey(draft));
            context.commit({ draft, ...context.editModeSyncPatch(draft) });
        },
        setCallableAsHandoffTarget(enabled) {
            if (isBuiltin()) {
                return;
            }
            const draft = clone(context.getSnapshot().draft);
            const restored = applyCallableAsHandoffTarget(draft, enabled, context.presentationMemory, memoryKey(draft));
            context.commit(restored ? { draft, ...context.editModeSyncPatch(draft) } : { draft });
        },
        setDelegationDescription(value) {
            editDraft((draft) => {
                draft.delegation.descriptionForAgents = value;
            });
        },
        setAllowedCallersCsv(value) {
            editDraft((draft) => {
                draft.delegation.allowedCallersCsv = value;
            });
        },
        setDelegationLimit(field, value) {
            editDraft((draft) => {
                draft.delegation[field] = value;
            });
        },
        setPlanMode(mode) {
            if (mode !== 'none') {
                throw new Error(`Unsupported Agent plan mode: ${mode}`);
            }
            editDraft((draft) => {
                draft.plan.mode = mode;
            });
        },
        setToolsLimitField(field, value) {
            editDraft((draft) => {
                draft.tools[field] = value;
            });
        },
        setModelRetryField(field, value) {
            editDraft((draft) => {
                draft.run.modelRetry[field] = value;
            });
        },
        setContextHistoryMessages(value) {
            editDraft((draft) => {
                draft.context.initialChatHistoryMessages = value;
            });
        },
        setContextIncludeWorldInfo(checked) {
            editDraft((draft) => {
                draft.context.includeActivatedWorldInfo = checked;
            });
        },
        setSkillsCsvField(field, value) {
            editDraft((draft) => {
                draft.skills[field] = value;
            });
        },
        setSkillsLimitField(field, value) {
            editDraft((draft) => {
                draft.skills[field] = value;
            });
        },
        setWorkspaceRootVisible(root, visible) {
            editDraft((draft) => {
                applyWorkspaceRootVisible(draft, root, visible);
            });
        },
        setWorkspaceRootWritable(root, writable) {
            editDraft((draft) => {
                applyWorkspaceRootWritable(draft, root, writable);
            });
        },
        addStateAccessRows(patternsCsv = '') {
            appendStateAccessRows(splitKeyPatterns(patternsCsv));
        },
        async expandStateAccessFromDeclaration() {
            if (isBuiltin()) {
                return;
            }
            const choice = context.getSnapshot().stateDeclarationChoice.trim();
            if (!choice) {
                return;
            }
            const patterns: string[] = [];
            try {
                const declaration = await deps.loadStateDeclaration(choice);
                const existing = new Set(
                    (context.getSnapshot().draft.stateAccess?.entries ?? []).map((row) => row.pattern),
                );
                // One row per declared field key. Rows already in the grid and
                // repeats inside the declaration are skipped, so expanding twice
                // cannot stack duplicate rows the backend would refuse as overlaps.
                for (const field of declaration.fields) {
                    const pattern = String(field?.pattern ?? '').trim();
                    if (pattern.length > 0 && !existing.has(pattern) && !patterns.includes(pattern)) {
                        patterns.push(pattern);
                    }
                }
            } catch (error) {
                deps.notifyError(error);
                return;
            }
            if (patterns.length === 0) {
                deps.notifyWarning(deps.tr('stateAccessDeclarationEmpty'));
                return;
            }
            appendStateAccessRows(patterns);
        },
        async expandStateAccessFromChatDeclaration() {
            if (isBuiltin()) {
                return;
            }
            let resolved: ChatStateDeclaration | null = null;
            try {
                resolved = await deps.resolveChatStateDeclaration();
            } catch (error) {
                deps.notifyError(error);
                return;
            }
            if (!resolved) {
                deps.notifyWarning(deps.tr('stateAccessChatDeclarationMissing'));
                return;
            }
            const existing = new Set(
                (context.getSnapshot().draft.stateAccess?.entries ?? []).map((row) => row.pattern),
            );
            const patterns: string[] = [];
            for (const field of resolved.fields) {
                const pattern = String(field?.pattern ?? '').trim();
                if (pattern.length > 0 && !existing.has(pattern) && !patterns.includes(pattern)) {
                    patterns.push(pattern);
                }
            }
            if (patterns.length === 0) {
                deps.notifyWarning(deps.tr('stateAccessDeclarationEmpty'));
                return;
            }
            appendStateAccessRows(patterns);
            context.commit({ chatDeclarationName: resolved.name });
        },
        removeStateAccessRow(index) {
            editDraft((draft) => {
                applyRemoveStateAccessRow(draft, index);
            });
        },
        updateStateAccessRow(index, patch) {
            editDraft((draft) => {
                applyUpdateStateAccessRow(draft, index, patch);
            });
        },
        ...recallControls(editDraft),
        ...worldInfoControls(editDraft),
        setOutputArtifactField(field, value) {
            editDraft((draft) => {
                const [artifact] = draft.output.artifacts;
                if (!artifact) {
                    throw new Error('Agent profile is missing output.artifacts[0]');
                }
                artifact[field] = value;
            });
        },
        selectTool(toolId) {
            context.commit({ selectedToolId: toolId });
        },
        async toggleToolAllowed(toolId, enabled) {
            if (!enabled && toolHasDescriptionOverride(context.getSnapshot().draft, toolId)) {
                // The checkbox is controlled: declining the confirmation
                // simply skips the commit, leaving it checked.
                if (!await deps.confirmAction(deps.tr('removeToolDescriptionOnDisableConfirm', { tool: toolId }))) {
                    return;
                }
                if (context.isDisposed()) {
                    return;
                }
            }
            editDraft((draft) => {
                if (!enabled) {
                    applyResetToolDescriptionOverride(draft, toolId);
                }
                applyToolAllowed(draft, toolId, enabled, new Set(context.catalogToolIds()), context.getSnapshot().toolIds);
            });
        },
        setToolDescriptionOverride(toolId, value) {
            editDraft((draft) => {
                applyToolDescriptionOverride(draft, toolId, value);
            });
        },
        setToolPropertyDescriptionOverride(toolId, property, value) {
            editDraft((draft) => {
                applyToolPropertyDescriptionOverride(draft, toolId, property, value);
            });
        },
        resetToolDescriptionOverride(toolId) {
            editDraft((draft) => {
                applyResetToolDescriptionOverride(draft, toolId);
            });
        },
        resetToolPropertyDescriptionOverride(toolId, property) {
            editDraft((draft) => {
                applyResetToolPropertyDescriptionOverride(draft, toolId, property);
            });
        },
        setDraftJson(value) {
            context.commit({ draftJson: value });
        },
        refreshDraftJson() {
            context.commit({ draftJson: prettyJson(normalizeProfileForSave(context.getSnapshot().draft)) });
        },
        applyDraftJson() {
            const parsed = JSON.parse(context.getSnapshot().draftJson) as TauriTavernAgentProfileDefinition;
            const draft = profileForEdit(parsed);
            context.seedMainAgentPresentation(draft);
            context.bumpProfileSelectionToken();
            context.commit({
                draft,
                toolIds: context.toolIdsForDraft(draft),
                editingProfileId: parsed.id,
                ...context.editModeSyncPatch(draft),
                ...context.clearedRuntimeState(),
            });
        },
        newProfile() {
            const id = nextProfileId(context.getSnapshot().profiles, 'agent-profile');
            replaceDraft(profileForEdit(defaultProfile(id)), id);
        },
        copyProfile() {
            const snapshot = context.getSnapshot();
            const id = nextProfileId(snapshot.profiles, `${snapshot.draft.id}-copy`);
            const copy = normalizeProfileForSave(snapshot.draft);
            copy.id = id;
            copy.displayName = deps.tr('copyDisplayName', { name: copy.displayName });
            replaceDraft(profileForEdit(copy), id);
        },
    };
}
