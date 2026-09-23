// @ts-check

import {
    loadAgentContextPolicy,
    normalizeAgentContextPolicy,
} from '../../../scripts/tauritavern/agent/agent-context-policy.js';
import {
    loadResolvedAgentSystemPrompt,
    normalizeAgentSystemPrompt,
} from '../../../scripts/tauritavern/agent/agent-system-prompt.js';
import {
    buildSettingsWithCurrentModelConnectionSnapshot,
    normalizeFrozenRunInputSnapshot,
} from '../../../scripts/tauritavern/agent/frozen-run-input-snapshot.js';
import { resolveStateDeclarationBinding } from '../../../scripts/state-declaration-binding-policy.js';
import { resolveStateMachineBinding } from '../../../scripts/state-machine-binding-policy.js';

const LEGACY_DRY_RUN_SOURCE = 'legacy-generate-dry-run';

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any>; profileId?: string; agentContextPolicy?: Record<string, any>; agentSystemPrompt?: string }} input
 * @returns {Promise<{ currentPromptSnapshotSeed: any; frozenRunInputSnapshot: any; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshotSeed(input = {}) {
    const generationType = normalizeGenerationType(input.generationType);
    const generateOptions = normalizeGenerateOptions(input.generateOptions);
    const agentContextPolicy = input.agentContextPolicy
        ? normalizeAgentContextPolicy(input.agentContextPolicy)
        : await loadAgentContextPolicy(input.profileId);
    const agentSystemPrompt = Object.prototype.hasOwnProperty.call(input, 'agentSystemPrompt')
        ? normalizeAgentSystemPrompt(input.agentSystemPrompt)
        : await loadResolvedAgentSystemPrompt(input.profileId);
    const script = await import('../../../script.js');

    if (script.main_api !== 'openai') {
        throw new Error('agent.chat_completion_required: Agent runtime requires the OpenAI/chat-completion frontend path');
    }

    const { generateData } = await captureAgentDryRun(script, generationType, {
        ...generateOptions,
        agentMode: true,
        agentContextPolicy,
        agentSystemPrompt,
    });
    const messages = generateData?.prompt;
    assertMessagesReady(messages);
    assertNoExternalToolTurns(messages);
    const frozenRunInputSnapshot = normalizeFrozenRunInputSnapshot(generateData.frozenRunInputSnapshot);

    return {
        currentPromptSnapshotSeed: {
            generationType,
            contextPolicy: agentContextPolicy,
            messages: structuredClone(messages),
            jsonSchema: generateOptions.jsonSchema ?? null,
            worldInfoActivation: frozenRunInputSnapshot.worldInfoActivation ?? null,
        },
        frozenRunInputSnapshot,
        generationIntent: {
            source: LEGACY_DRY_RUN_SOURCE,
            generationType,
        },
    };
}

/**
 * @param {{ generationType?: string; generateOptions?: Record<string, any>; profileId?: string; agentContextPolicy?: Record<string, any>; agentSystemPrompt?: string }} input
 * @returns {Promise<{ promptSnapshot: { contextPolicy: any; chatCompletionPayload: any; worldInfoActivation?: any; stateDeclaration?: any }; frozenRunInputSnapshot: any; generationIntent: any }>}
 */
export async function buildAgentPromptSnapshot(input = {}) {
    return materializeCurrentPromptSnapshot(await buildAgentPromptSnapshotSeed(input));
}

/**
 * @param {any} input
 * @returns {Promise<any>}
 */
export async function materializeCurrentPromptSnapshot(input) {
    const seed = input?.currentPromptSnapshotSeed;
    if (!seed || typeof seed !== 'object' || Array.isArray(seed)) {
        throw new Error('agent.current_prompt_snapshot_seed_required: currentPromptSnapshotSeed must be an object');
    }
    const generationType = normalizeGenerationType(seed.generationType);
    const messages = seed.messages;
    assertMessagesReady(messages);
    assertNoExternalToolTurns(messages);
    const frozenRunInputSnapshot = normalizeFrozenRunInputSnapshot(input.frozenRunInputSnapshot);
    const openai = await import('../../../scripts/openai.js');
    const settings = await buildSettingsWithCurrentModelConnectionSnapshot(
        openai.oai_settings,
        frozenRunInputSnapshot.currentModelConnection,
    );
    const model = openai.getChatCompletionModel(settings);
    if (!model) {
        throw new Error('agent.model_required: current chat-completion source did not resolve a model');
    }

    const created = /** @type {any} */ (await openai.createGenerationParameters(
        settings,
        model,
        generationType,
        structuredClone(messages),
        {
            jsonSchema: seed.jsonSchema ?? null,
            agentMode: true,
        },
    ));
    const payload = created.generate_data;

    assertNoExternalTools(payload);
    assertNoExternalToolTurns(payload.messages);
    const stateDeclaration = await resolveCurrentStateDeclaration();
    const stateMachine = await resolveCurrentStateMachine(stateDeclaration);

    return {
        promptSnapshot: {
            contextPolicy: seed.contextPolicy,
            chatCompletionPayload: payload,
            ...(seed.worldInfoActivation ? { worldInfoActivation: seed.worldInfoActivation } : {}),
            ...(stateDeclaration ? { stateDeclaration } : {}),
            ...(stateMachine ? { stateMachine } : {}),
        },
        frozenRunInputSnapshot,
        generationIntent: {
            source: LEGACY_DRY_RUN_SOURCE,
            generationType,
            chatCompletionSource: payload.chat_completion_source,
            model: payload.model,
        },
    };
}

/**
 * Resolves the state declaration bound to the current context and loads its
 * JSON so the backend `state.update` tool can validate updates against it.
 *
 * Resolution follows the state declaration binding policy: chat header
 * metadata first, then the character/group binding owned by
 * `src/scripts/power-user.js`; a candidate only counts when its name still
 * exists in the saved declaration list (`list_state_declarations`). The
 * declared JSON is fetched with `get_state_declaration`.
 *
 * The result is only placed on the prompt snapshot when a declaration
 * actually resolved. Every failure path — host ABI missing, binding state
 * unreadable, empty declaration list, lookup rejected — logs a warning and
 * returns `undefined` so the run proceeds with the backend's default
 * declaration. This function never throws.
 *
 * @returns {Promise<any|undefined>}
 */
export async function resolveCurrentStateDeclaration() {
    const candidates = await loadStateDeclarationCandidates();
    if (candidates.length === 0) {
        return undefined;
    }

    const safeInvoke = getHostSafeInvoke();
    if (!safeInvoke) {
        console.warn('[state-declaration] host safeInvoke is unavailable; run continues without a state declaration');
        return undefined;
    }

    let availableNames;
    try {
        availableNames = await safeInvoke('list_state_declarations');
    } catch (error) {
        console.warn('[state-declaration] listing state declarations failed; run continues without a state declaration', error);
        return undefined;
    }
    if (!Array.isArray(availableNames)) {
        console.warn('[state-declaration] declaration list returned a non-array payload; run continues without a state declaration', availableNames);
        return undefined;
    }

    const selectedName = resolveStateDeclarationBinding(candidates, availableNames);
    if (!selectedName) {
        return undefined;
    }

    let declaration;
    try {
        declaration = await safeInvoke('get_state_declaration', { name: selectedName });
    } catch (error) {
        console.warn(`[state-declaration] loading declaration '${selectedName}' failed; run continues without a state declaration`, error);
        return undefined;
    }

    return declaration;
}

/**
 * Resolves the state machine bound to the current context and loads its spec so
 * the backend can resolve the model's move requests and advance the machine.
 *
 * Resolution follows the state machine binding policy: chat header metadata
 * first, then the character/group binding owned by `src/scripts/power-user.js`;
 * a candidate only counts when its name still exists in the saved machine list
 * (`list_state_machines`). The spec is fetched with `get_state_machine`.
 *
 * A scene keeps its stages with its fields, so a declaration that carries a
 * machine is the whole answer: the standalone binding is consulted only when the
 * declaration has none, which is how a configuration written before the
 * declaration could carry one keeps working.
 *
 * Like `resolveCurrentStateDeclaration`, every failure path — host ABI missing,
 * binding state unreadable, empty machine list, lookup rejected — logs a
 * warning and returns `undefined`, so the run proceeds exactly as it does on a
 * chat with no machine at all. This function never throws.
 *
 * @param {any} [declaration] the declaration resolved for this run, if any
 * @returns {Promise<any|undefined>}
 */
export async function resolveCurrentStateMachine(declaration) {
    if (declaration && typeof declaration === 'object' && declaration.machine) {
        return declaration.machine;
    }

    const candidates = await loadStateMachineCandidates();
    if (candidates.length === 0) {
        return undefined;
    }

    const safeInvoke = getHostSafeInvoke();
    if (!safeInvoke) {
        console.warn('[state-machine] host safeInvoke is unavailable; run continues without a state machine');
        return undefined;
    }

    let availableNames;
    try {
        availableNames = await safeInvoke('list_state_machines');
    } catch (error) {
        console.warn('[state-machine] listing state machines failed; run continues without a state machine', error);
        return undefined;
    }
    if (!Array.isArray(availableNames)) {
        console.warn('[state-machine] machine list returned a non-array payload; run continues without a state machine', availableNames);
        return undefined;
    }

    const selectedName = resolveStateMachineBinding(candidates, availableNames);
    if (!selectedName) {
        return undefined;
    }

    let machine;
    try {
        machine = await safeInvoke('get_state_machine', { name: selectedName });
    } catch (error) {
        console.warn(`[state-machine] loading machine '${selectedName}' failed; run continues without a state machine`, error);
        return undefined;
    }

    return machine;
}

/**
 * Loads the ordered binding candidates (chat -> character/group) from the
 * SillyTavern runtime state owned by `src/scripts/power-user.js`. The module
 * is imported dynamically because the prompt snapshot layer is reached from
 * the host bootstrap chain that loads `script.js` after itself.
 *
 * @returns {Promise<any[]>}
 */
async function loadStateDeclarationCandidates() {
    try {
        const powerUser = await import('../../../scripts/power-user.js');
        const candidates = powerUser.getStateDeclarationBindingCandidates();
        return Array.isArray(candidates) ? candidates : [];
    } catch (error) {
        console.warn('[state-declaration] binding candidates are unavailable; run continues without a state declaration', error);
        return [];
    }
}

/**
 * The state machine counterpart of `loadStateDeclarationCandidates`.
 *
 * @returns {Promise<any[]>}
 */
async function loadStateMachineCandidates() {
    try {
        const powerUser = await import('../../../scripts/power-user.js');
        const candidates = powerUser.getStateMachineBindingCandidates();
        return Array.isArray(candidates) ? candidates : [];
    } catch (error) {
        console.warn('[state-machine] binding candidates are unavailable; run continues without a state machine', error);
        return [];
    }
}

/**
 * @returns {((command: string, args?: any) => Promise<any>)|null}
 */
function getHostSafeInvoke() {
    const safeInvoke = window.__TAURITAVERN__?.invoke?.safeInvoke;
    return typeof safeInvoke === 'function' ? safeInvoke : null;
}

/**
 * @param {any} value
 * @returns {string}
 */
function normalizeGenerationType(value) {
    return String(value || 'normal').trim() || 'normal';
}

/**
 * @param {any} value
 * @returns {Record<string, any>}
 */
function normalizeGenerateOptions(value) {
    if (value == null) {
        return {};
    }
    if (typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('agent.generate_options_invalid: generateOptions must be an object');
    }
    return value;
}

/**
 * @param {any} script
 * @param {any} generationType
 * @param {any} generateOptions
 * @returns {Promise<{ generateData: any }>}
 */
async function captureAgentDryRun(script, generationType, generateOptions) {
    let generateData = null;
    /**
     * @param {any} capturedGenerateData
     * @param {any} dryRun
     */
    const generateListener = (capturedGenerateData, dryRun) => {
        if (dryRun === true) {
            generateData = capturedGenerateData;
        }
    };

    script.eventSource.on(script.event_types.GENERATE_AFTER_DATA, generateListener);
    try {
        await script.Generate(generationType, generateOptions, true);
    } finally {
        script.eventSource.removeListener(script.event_types.GENERATE_AFTER_DATA, generateListener);
    }

    if (!generateData || typeof generateData !== 'object' || Array.isArray(generateData)) {
        throw new Error('agent.prompt_snapshot_missing: dryRun did not emit generate_after_data');
    }

    return { generateData };
}

/**
 * @param {any} messages
 * @returns {void}
 */
function assertMessagesReady(messages) {
    if (!Array.isArray(messages)) {
        throw new Error('agent.prompt_snapshot_messages_required: dryRun did not produce chat-completion messages');
    }
}

/**
 * @param {any} payload
 * @returns {void}
 */
function assertNoExternalTools(payload) {
    const tools = payload?.tools;
    if (Array.isArray(tools) && tools.length > 0) {
        throw new Error('agent.external_tools_unsupported: Agent runtime owns the tool registry');
    }
    if (Object.prototype.hasOwnProperty.call(payload || {}, 'tool_choice')) {
        throw new Error('agent.external_tool_choice_unsupported: Agent runtime owns tool choice');
    }
}

/**
 * @param {any} messages
 * @returns {void}
 */
function assertNoExternalToolTurns(messages) {
    if (!Array.isArray(messages)) {
        return;
    }

    const hasToolTurn = messages.some((message) => {
        const role = String(message?.role || '').toLowerCase();
        return role === 'tool'
            || (Array.isArray(message?.tool_calls) && message.tool_calls.length > 0);
    });

    if (hasToolTurn) {
        throw new Error('agent.external_tool_turns_unsupported: prompt snapshot already contains tool turns');
    }
}
