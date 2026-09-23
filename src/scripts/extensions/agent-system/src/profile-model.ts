import {
    AGENT_DELEGATION_TOOLS,
    AGENT_HANDOFF_TOOLS,
    AGENT_SUBAGENT_TOOLS,
    DEFAULT_PROFILE_ID,
    KNOWN_TOOLS,
    RUNTIME_ONLY_TOOLS,
    WORKSPACE_ROOTS,
} from './constants';
import { clone } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import { AGENT_MODEL_REQUIRES_CONFIGURATION } from '../../../tauritavern/agent/agent-profile-portable.js';
import {
    DEFAULT_AGENT_CONTEXT_POLICY,
    normalizeAgentContextPolicy,
} from '../../../tauritavern/agent/agent-context-policy.js';
import {
    stateAccessEntriesFromRows,
    stateAccessRowsFromPolicy,
    type StateAccessRow,
} from './profile-state-access';
import {
    normalizeRecallPolicy,
    recallDraftFromPolicy,
    type AgentRecallDraft,
} from './profile-recall';
import { normalizeToolDescriptions } from './profile-tool-descriptions';

const DEFAULT_MCP_RESULT_INLINE_CHAR_LIMIT = 50_000;
const DEFAULT_UNFOLDED_TOOL_TURNS = 2;

type AgentProfile = TauriTavernAgentProfileDefinition;

/**
 * Transient numeric input state: an empty field remains '' until save-time
 * normalization converts it to a number.
 */
export type AgentProfileDraftNumber = number | '';

/**
 * One World Info entry this Agent's context policy makes an exception for.
 *
 * Named by book and uid — the pair the scan keys entries by — because a comment
 * is a title a human wrote and may repeat.
 */
export type WorldInfoEntryRule = {
    book: string;
    uid: number;
    inject: boolean;
};

export type AgentProfileDraftDelegation = Omit<AgentProfile['delegation'],
    'maxConcurrentInvocations' | 'maxInvocationsPerRun' | 'maxHandoffDepth'> & {
        maxConcurrentInvocations: AgentProfileDraftNumber;
        maxInvocationsPerRun: AgentProfileDraftNumber;
        maxHandoffDepth: AgentProfileDraftNumber;
        allowedCallersCsv?: string;
    };

/**
 * UI editor draft. Stays separate from the canonical
 * TauriTavernAgentProfileDefinition: it carries CSV mirrors of list fields
 * and ''-valued transient numeric inputs that only normalize at save time.
 */
export type AgentProfileDraft = Omit<
    AgentProfile,
    'run' | 'context' | 'delegation' | 'tools' | 'skills' | 'stateAccess' | 'recall'
> & {
    /** Edited as rows; `profileForEdit` fills the defaults a row shows. */
    stateAccess?: { entries?: StateAccessRow[] };
    /** Two switches and a CSV mirror; `profileForEdit` fills what the editor shows. */
    recall?: AgentRecallDraft;
    run: Omit<AgentProfile['run'], 'modelRetry'> & {
        modelRetry: {
            maxRetries: AgentProfileDraftNumber;
            intervalMs: AgentProfileDraftNumber;
        };
    };
    context: {
        initialChatHistoryMessages: AgentProfileDraftNumber;
        includeActivatedWorldInfo: boolean;
        /** Edited as a list of entries from the chat's world books. */
        worldInfo?: {
            entries?: ReadonlyArray<WorldInfoEntryRule>;
            subagentInherits?: boolean;
        };
    };
    delegation: AgentProfileDraftDelegation;
    tools: Omit<AgentProfile['tools'],
        'maxRounds' | 'maxCallsPerRun' | 'mcpResultInlineCharLimit' | 'unfoldedToolTurns'> & {
        maxRounds: AgentProfileDraftNumber;
        maxCallsPerRun: AgentProfileDraftNumber;
        mcpResultInlineCharLimit: AgentProfileDraftNumber;
        unfoldedToolTurns: AgentProfileDraftNumber;
    };
    skills: Omit<AgentProfile['skills'], 'maxReadCharsPerCall' | 'maxReadCharsPerRun'> & {
        maxReadCharsPerCall: AgentProfileDraftNumber;
        maxReadCharsPerRun: AgentProfileDraftNumber;
        visibleCsv?: string;
        denyCsv?: string;
    };
};

/** String(value || '') for JSON-scalar inputs; objects have no useful text form. */
function looseString(value: unknown): string {
    if (!value) {
        return '';
    }
    if (typeof value === 'string') {
        return value;
    }
    if (typeof value === 'number' || typeof value === 'boolean') {
        return String(value);
    }
    return '';
}

export function normalizeProfileId(value: unknown): string {
    return looseString(value)
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, '-')
        .replace(/^-+|-+$/g, '')
        .slice(0, 128);
}

function parseCsv(value: unknown): string[] {
    return looseString(value)
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);
}

function joinCsv(values: unknown): string {
    return Array.isArray(values) ? values.join(', ') : '';
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/**
 * The context policy a Profile stores.
 *
 * The same normalization the run uses, minus a field that has nothing to say: a
 * Profile with no World Info exception stores none, rather than storing an empty
 * list that the host would read as the default anyway.
 */
function contextPolicyForSave(context: AgentProfileDraft['context']): AgentProfile['context'] {
    const normalized = normalizeAgentContextPolicy(context);
    const { entries, subagentInherits } = normalized.worldInfo;

    return entries.length === 0 && !subagentInherits
        ? {
            initialChatHistoryMessages: normalized.initialChatHistoryMessages,
            includeActivatedWorldInfo: normalized.includeActivatedWorldInfo,
        }
        : normalized;
}


function normalizePresetBinding(value: unknown): AgentProfile['preset'] {
    const binding = isPlainObject(value) ? { ...value } : {};
    const mode = (looseString(binding.mode) || 'currentPromptSnapshot').trim() || 'currentPromptSnapshot';
    if (mode === 'currentPromptSnapshot' || mode === 'none') {
        return {
            mode,
            required: false,
        };
    }

    if (mode !== 'ref') {
        throw new Error(`preset.mode is unsupported: ${mode}`);
    }

    const ref = isPlainObject(binding.ref) ? binding.ref : {};
    return {
        mode: 'ref',
        ref: {
            apiId: looseString(ref.apiId).trim(),
            name: looseString(ref.name).trim(),
        },
        required: Boolean(binding.required),
    };
}

function normalizeModelBinding(value: unknown): AgentProfile['model'] {
    const binding = isPlainObject(value) ? { ...value } : {};
    const mode = (looseString(binding.mode) || 'currentPromptSnapshot').trim() || 'currentPromptSnapshot';
    if (mode === 'currentPromptSnapshot') {
        return {
            mode: 'currentPromptSnapshot',
        };
    }
    if (mode === AGENT_MODEL_REQUIRES_CONFIGURATION) {
        return {
            mode: AGENT_MODEL_REQUIRES_CONFIGURATION,
        };
    }

    if (mode !== 'connectionRef') {
        throw new Error(`model.mode is unsupported: ${mode}`);
    }

    return {
        mode: 'connectionRef',
        connectionRef: looseString(binding.connectionRef).trim(),
        modelId: looseString(binding.modelId).trim(),
    };
}

function normalizeRunPolicy(value: unknown): AgentProfile['run'] {
    const policy = isPlainObject(value) ? { ...value } : {};
    const presentation = (looseString(policy.presentation) || 'foreground').trim() || 'foreground';
    if (presentation !== 'foreground' && presentation !== 'background') {
        throw new Error(`run.presentation is unsupported: ${presentation}`);
    }
    const stream = policy.stream ?? false;
    if (typeof stream !== 'boolean') {
        throw new Error('run.stream must be a boolean');
    }
    const directRunnable = policy.directRunnable !== false;
    const modelRetry = isPlainObject(policy.modelRetry) ? policy.modelRetry : {};

    return {
        presentation: directRunnable ? presentation : 'background',
        stream,
        directRunnable,
        modelRetry: {
            maxRetries: Number(modelRetry.maxRetries ?? 3),
            intervalMs: Number(modelRetry.intervalMs ?? 3000),
        },
    };
}

function defaultDelegationPolicy(): AgentProfile['delegation'] {
    return {
        canDelegate: false,
        canHandoff: false,
        callable: false,
        allowAsSubagent: false,
        allowAsHandoffTarget: false,
        allowNestedDelegation: false,
        allowedCallers: ['*'],
        descriptionForAgents: null,
        maxConcurrentInvocations: 3,
        maxInvocationsPerRun: 8,
        resultBudgetTokens: 8000,
        maxHandoffDepth: 8,
    };
}

function normalizeDelegationPolicy(value: unknown): AgentProfile['delegation'] {
    const defaults = defaultDelegationPolicy();
    const policy = isPlainObject(value) ? { ...value } : {};
    const allowedCallers = Object.prototype.hasOwnProperty.call(policy, 'allowedCallersCsv')
        ? parseCsv(policy.allowedCallersCsv)
        : (Array.isArray(policy.allowedCallers)
            ? policy.allowedCallers.map((caller: unknown) => looseString(caller).trim()).filter(Boolean)
            : defaults.allowedCallers);
    const description = looseString(policy.descriptionForAgents).trim();

    return {
        canDelegate: Boolean(policy.canDelegate),
        canHandoff: Boolean(policy.canHandoff),
        callable: Boolean(policy.callable),
        allowAsSubagent: Boolean(policy.allowAsSubagent),
        allowAsHandoffTarget: Boolean(policy.allowAsHandoffTarget),
        allowNestedDelegation: Boolean(policy.allowNestedDelegation),
        allowedCallers,
        descriptionForAgents: description || null,
        maxConcurrentInvocations: Number(policy.maxConcurrentInvocations ?? defaults.maxConcurrentInvocations),
        maxInvocationsPerRun: Number(policy.maxInvocationsPerRun ?? defaults.maxInvocationsPerRun),
        resultBudgetTokens: Number(policy.resultBudgetTokens ?? defaults.resultBudgetTokens),
        maxHandoffDepth: Number(policy.maxHandoffDepth ?? defaults.maxHandoffDepth),
    };
}

export function normalizeDelegationToolAllowList(
    allowList: unknown,
    delegationPolicy?: { canDelegate?: unknown; canHandoff?: unknown } | null,
    preferredOrder: readonly string[] = [...AGENT_DELEGATION_TOOLS, ...KNOWN_TOOLS],
): string[] {
    const delegation = delegationPolicy || defaultDelegationPolicy();
    const runtimeOnly = new Set(RUNTIME_ONLY_TOOLS);
    const allow = new Set((Array.isArray(allowList) ? allowList : [])
        .filter((tool): tool is string => typeof tool === 'string' && !runtimeOnly.has(tool)));

    if (delegation.canDelegate) {
        for (const tool of AGENT_SUBAGENT_TOOLS) {
            allow.add(tool);
        }
    } else {
        allow.delete('builtin:agent.delegate');
        allow.delete('builtin:agent.await');
    }

    if (delegation.canHandoff) {
        for (const tool of AGENT_HANDOFF_TOOLS) {
            allow.add(tool);
        }
    } else {
        allow.delete('builtin:agent.handoff');
    }

    if (!delegation.canDelegate && !delegation.canHandoff) {
        allow.delete('builtin:agent.list');
    }

    const orderedSet = new Set(preferredOrder);
    return [
        ...preferredOrder.filter((tool) => allow.has(tool)),
        ...[...allow].filter((tool) => !orderedSet.has(tool)),
    ];
}

function applyDelegationToolPolicy(profile: AgentProfileDraft): void {
    profile.tools.allow = normalizeDelegationToolAllowList(
        profile.tools?.allow,
        profile.delegation,
        [
            ...AGENT_DELEGATION_TOOLS,
            ...KNOWN_TOOLS,
        ],
    );
}

export function defaultProfile(id: string = DEFAULT_PROFILE_ID): AgentProfile {
    const profileId = normalizeProfileId(id) || DEFAULT_PROFILE_ID;
    const profile: AgentProfile = {
        schemaVersion: 3,
        kind: 'tauritavern.agentProfile',
        id: profileId,
        displayName: profileId === DEFAULT_PROFILE_ID ? tr('defaultWriter') : tr('newAgentProfile'),
        description: profileId === DEFAULT_PROFILE_ID ? tr('defaultWriterDescription') : '',
        preset: {
            mode: 'currentPromptSnapshot',
            required: false,
        },
        model: {
            mode: 'currentPromptSnapshot',
        },
        run: {
            presentation: 'foreground',
            stream: true,
            directRunnable: true,
            modelRetry: {
                maxRetries: 3,
                intervalMs: 3000,
            },
        },
        context: {
            ...DEFAULT_AGENT_CONTEXT_POLICY,
        },
        delegation: defaultDelegationPolicy(),
        instructions: {
            agentSystemPrompt: null,
        },
        tools: {
            allow: [...KNOWN_TOOLS],
            deny: [],
            toolDescriptions: {},
            maxRounds: 80,
            maxCallsPerRun: 80,
            mcpResultInlineCharLimit: DEFAULT_MCP_RESULT_INLINE_CHAR_LIMIT,
            unfoldedToolTurns: DEFAULT_UNFOLDED_TOOL_TURNS,
            maxCallsPerTool: {},
        },
        skills: {
            visible: ['*'],
            deny: [],
            maxReadCharsPerCall: 20000,
            maxReadCharsPerRun: 80000,
        },
        workspace: {
            visibleRoots: [...WORKSPACE_ROOTS],
            writableRoots: [...WORKSPACE_ROOTS],
        },
        plan: {
            mode: 'none',
            beta: true,
            nodes: [],
        },
        output: {
            artifacts: [
                {
                    id: 'main',
                    path: 'output/main.md',
                    kind: 'markdown',
                    target: 'messageBody',
                    required: true,
                    assemblyOrder: 0,
                },
            ],
        },
    };
    return profile;
}

export function normalizeProfileForSave(profile: AgentProfileDraft): TauriTavernAgentProfileDefinition {
    const normalized = clone(profile);
    migrateToolPolicyToV3(normalized);
    const visibleCsv = Object.prototype.hasOwnProperty.call(normalized.skills, 'visibleCsv')
        ? normalized.skills.visibleCsv
        : joinCsv(normalized.skills.visible);
    const denyCsv = Object.prototype.hasOwnProperty.call(normalized.skills, 'denyCsv')
        ? normalized.skills.denyCsv
        : joinCsv(normalized.skills.deny);

    normalized.id = normalizeProfileId(normalized.id);
    normalized.displayName = looseString(normalized.displayName).trim();
    normalized.description = looseString(normalized.description).trim();
    normalized.schemaVersion = 3;
    normalized.preset = normalizePresetBinding(normalized.preset);
    normalized.model = normalizeModelBinding(normalized.model);
    normalized.run = normalizeRunPolicy(normalized.run);
    normalized.context = contextPolicyForSave(normalized.context);
    normalized.delegation = normalizeDelegationPolicy(normalized.delegation);
    normalized.tools.maxRounds = Number(normalized.tools.maxRounds);
    normalized.tools.maxCallsPerRun = Number(normalized.tools.maxCallsPerRun);
    normalized.tools.unfoldedToolTurns = Number(normalized.tools.unfoldedToolTurns);
    normalized.tools.toolDescriptions = normalizeToolDescriptions(normalized.tools.toolDescriptions);
    normalized.skills.maxReadCharsPerCall = Number(normalized.skills.maxReadCharsPerCall);
    normalized.skills.maxReadCharsPerRun = Number(normalized.skills.maxReadCharsPerRun);
    normalized.instructions.agentSystemPrompt = looseString(normalized.instructions.agentSystemPrompt).trim() || null;
    normalized.skills.visible = parseCsv(visibleCsv);
    normalized.skills.deny = parseCsv(denyCsv);
    delete normalized.skills.visibleCsv;
    delete normalized.skills.denyCsv;
    applyDelegationToolPolicy(normalized);
    const [firstArtifact] = normalized.output.artifacts;
    if (!firstArtifact) {
        throw new Error('output.artifacts must contain the main artifact');
    }
    const artifact: TauriTavernAgentProfileDefinition['output']['artifacts'][number] = {
        ...firstArtifact,
        id: 'main',
        target: 'messageBody',
        required: true,
        assemblyOrder: 0,
    };
    normalized.output.artifacts = [artifact];
    normalized.stateAccess = {
        entries: stateAccessEntriesFromRows(normalized.stateAccess?.entries ?? []),
    };
    normalized.recall = normalizeRecallPolicy(normalized.recall);
    // The normalizers above rewrote every draft-only field (CSV mirrors,
    // ''-valued numeric inputs) into the canonical shape.
    return normalized as TauriTavernAgentProfileDefinition;
}

export function profileForEdit(profile: TauriTavernAgentProfileDefinition): AgentProfileDraft {
    const draft = clone(profile) as AgentProfileDraft;
    migrateToolPolicyToV3(draft);
    draft.preset = normalizePresetBinding(draft.preset);
    draft.model = normalizeModelBinding(draft.model);
    draft.run = normalizeRunPolicy(draft.run);
    draft.context = normalizeAgentContextPolicy(draft.context);
    draft.delegation = normalizeDelegationPolicy(draft.delegation);
    draft.delegation.allowedCallersCsv = joinCsv(draft.delegation.allowedCallers);
    draft.tools.toolDescriptions = normalizeToolDescriptions(draft.tools.toolDescriptions);
    draft.skills.visibleCsv = joinCsv(draft.skills.visible);
    draft.skills.denyCsv = joinCsv(draft.skills.deny);
    draft.stateAccess = { entries: stateAccessRowsFromPolicy(profile.stateAccess) };
    draft.recall = recallDraftFromPolicy(profile.recall);
    return draft;
}

function migrateToolPolicyToV3(profile: AgentProfileDraft): void {
    const version = Number(profile.schemaVersion || 1);
    profile.tools.mcpResultInlineCharLimit = Number(
        profile.tools.mcpResultInlineCharLimit ?? DEFAULT_MCP_RESULT_INLINE_CHAR_LIMIT,
    );
    profile.tools.unfoldedToolTurns = Number(
        profile.tools.unfoldedToolTurns ?? DEFAULT_UNFOLDED_TOOL_TURNS,
    );
    if (version === 3) {
        return;
    }
    if (version !== 1 && version !== 2) {
        throw new Error(`profile.schemaVersion is unsupported: ${version}`);
    }
    const canonical = (name: string): string => `builtin:${name}`;
    profile.tools.allow = (profile.tools.allow || []).map(canonical);
    profile.tools.deny = (profile.tools.deny || []).map(canonical);
    profile.tools.toolDescriptions = Object.fromEntries(
        Object.entries(profile.tools.toolDescriptions || {}).map(([name, value]) => [canonical(name), value]),
    );
    profile.tools.maxCallsPerTool = Object.fromEntries(
        Object.entries(profile.tools.maxCallsPerTool || {}).map(([name, value]) => [canonical(name), value]),
    );
    profile.schemaVersion = 3;
}
