export const DEFAULT_AGENT_CONTEXT_POLICY = Object.freeze({
    initialChatHistoryMessages: -1,
    includeActivatedWorldInfo: true,
    worldInfo: Object.freeze({ entries: Object.freeze([]) }),
});

/**
 * The per-entry overrides a Profile sets on the chat's World Info.
 *
 * A row is an exception, in both directions: `inject: false` keeps that entry out
 * of this Agent's prompt while the switch says entries are injected, and
 * `inject: true` lets one entry through while the switch says they are not. An
 * entry with no row keeps the switch — the reading the state access policy gets.
 *
 * Entries are named by book and uid, the pair the scan itself keys them by: a
 * comment is a human's title and may repeat.
 */
function normalizeWorldInfoEntryRules(entries) {
    if (!Array.isArray(entries)) {
        return [];
    }

    return entries
        .map((entry) => ({
            book: String(entry?.book ?? '').trim(),
            uid: Number(entry?.uid),
            inject: entry?.inject !== false,
        }))
        .filter((entry) => entry.book && Number.isFinite(entry.uid));
}

/** The key the scan names an entry by, and the key a rule is looked up by. */
export function worldInfoEntryKey(entry) {
    return `${String(entry?.world ?? '').trim()}.${Number(entry?.uid)}`;
}

/**
 * Whether the scan has to run for this policy.
 *
 * The switch alone is not the answer: a rule that lets an entry through means the
 * scan has something to find even while the switch says entries are not injected.
 */
export function worldInfoScanNeeded(policy) {
    const resolved = normalizeAgentContextPolicy(policy);
    return resolved.includeActivatedWorldInfo
        || resolved.worldInfo.entries.some((entry) => entry.inject);
}

/**
 * The predicate the World Info scan filters its entry table with.
 *
 * Applied before the scan: an entry this Agent may not be told about does not
 * consume the World Info budget and does not feed recursion, rather than being
 * injected and then removed. Returns `null` when nothing is configured, which
 * leaves the scan exactly as it was.
 */
export function worldInfoEntryFilter(policy) {
    const resolved = normalizeAgentContextPolicy(policy);
    if (resolved.includeActivatedWorldInfo && resolved.worldInfo.entries.length === 0) {
        return null;
    }

    const rules = new Map(
        resolved.worldInfo.entries.map((entry) => [`${entry.book}.${entry.uid}`, entry.inject]),
    );

    return (entry) => {
        const rule = rules.get(worldInfoEntryKey(entry));
        return rule === undefined ? resolved.includeActivatedWorldInfo : rule;
    };
}

/**
 * A context policy as it comes off a Profile, a draft, or a run input: JSON
 * whose shape is checked here rather than at every call site.
 *
 * @param {any} [value]
 */
export function normalizeAgentContextPolicy(value = {}) {
    const source = value || {};
    const initialChatHistoryMessages = Number(source.initialChatHistoryMessages ?? DEFAULT_AGENT_CONTEXT_POLICY.initialChatHistoryMessages);

    if (!Number.isInteger(initialChatHistoryMessages)) {
        throw new Error('agent.context_history_invalid: initialChatHistoryMessages must be negative for full history, zero for no initial history, or positive for a recent-message window');
    }

    return {
        initialChatHistoryMessages: initialChatHistoryMessages < 0 ? -1 : initialChatHistoryMessages,
        includeActivatedWorldInfo: source.includeActivatedWorldInfo !== false,
        worldInfo: {
            entries: normalizeWorldInfoEntryRules(source.worldInfo?.entries),
            // Carried even though this module's own caller only reads it for what
            // goes into the prompt: the frozen run input has to match the Profile
            // the host resolved, and the host compares them field for field.
            subagentInherits: source.worldInfo?.subagentInherits === true,
        },
    };
}

export function agentContextPolicyForProfile(profile) {
    return normalizeAgentContextPolicy(profile?.context);
}

/**
 * Whether that entry reaches this Agent, a row and the switch together.
 *
 * A row wins over the switch, in both directions; an entry with no row keeps
 * the switch. This is the reading a delegated invocation gets.
 */
export function worldInfoEntryCarriedBy(policy, entry) {
    const resolved = normalizeAgentContextPolicy(policy);
    const key = worldInfoEntryKey(entry);
    const rule = resolved.worldInfo.entries
        .find((candidate) => `${candidate.book}.${candidate.uid}` === key);
    return rule ? rule.inject : resolved.worldInfo.subagentInherits;
}

/**
 * The policy after one entry's decision, with a row that agrees with the
 * switch dropped.
 *
 * What a reader set is a decision about an entry, and a decision the switch
 * already makes is not worth storing — it would silently change meaning the
 * next time the switch is flipped.
 */
export function worldInfoPolicyWithEntry(policy, entry, carried) {
    const resolved = normalizeAgentContextPolicy(policy);
    const book = String(entry?.world ?? '').trim();
    const uid = Number(entry?.uid);
    if (!book || !Number.isFinite(uid)) {
        return resolved;
    }

    const key = `${book}.${uid}`;
    const entries = resolved.worldInfo.entries
        .filter((candidate) => `${candidate.book}.${candidate.uid}` !== key);
    if (carried !== resolved.worldInfo.subagentInherits) {
        entries.push({ book, uid, inject: carried !== false });
    }

    return { ...resolved, worldInfo: { ...resolved.worldInfo, entries } };
}

export async function loadAgentContextPolicy(profileId) {
    const normalizedProfileId = String(profileId || '').trim();
    if (!normalizedProfileId) {
        // Normalized rather than spread: the default's nested entry list is
        // shared, and a caller that edits what it gets back would edit the
        // default for everyone.
        return normalizeAgentContextPolicy(DEFAULT_AGENT_CONTEXT_POLICY);
    }

    const profileApi = window.__TAURITAVERN__?.api?.agent?.profiles;
    if (typeof profileApi?.load !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }

    const result = await profileApi.load({ profileId: normalizedProfileId });
    if (!result?.profile) {
        throw new Error(`agent.profile_not_found: Agent profile not found: ${normalizedProfileId}`);
    }

    return agentContextPolicyForProfile(result.profile);
}

export function applyInitialChatHistoryPolicy(coreChat, policy) {
    if (!Array.isArray(coreChat)) {
        throw new Error('agent.context_history_messages_invalid: messages must be an array');
    }

    const resolved = normalizeAgentContextPolicy(policy);
    if (resolved.initialChatHistoryMessages < 0) {
        return coreChat;
    }
    if (resolved.initialChatHistoryMessages === 0) {
        return [];
    }

    // SillyTavern's OpenAI PromptManager raw chat history is latest-first.
    // Positive Agent windows therefore keep the front of the array.
    return coreChat.slice(0, resolved.initialChatHistoryMessages);
}

export function materializeInitialChatHistoryMessages(coreChat, policy) {
    // PromptManager assembly mutates history while injecting prompts and
    // reversing into provider order; frozen Agent input must stay reusable.
    return structuredClone(applyInitialChatHistoryPolicy(coreChat, policy));
}
