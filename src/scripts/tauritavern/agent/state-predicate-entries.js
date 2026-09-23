// @ts-check

/**
 * Predicate entries injected into the assembled prompt.
 *
 * A predicate set is bound to the chat the way a declaration is, and the host
 * evaluates it against the chat's newest committed state: the answer is the
 * entries that made it. This module places that answer with the same
 * extension-prompt machinery the state slice uses, under its own key prefix —
 * two prefixes are what keep a state block and a predicate block from
 * overwriting each other.
 *
 * Blocks are registered before prompt assembly runs and flushed afterwards.
 * Extension prompts are global, so a block that is not flushed would leak into
 * every later generation, including normal ones.
 *
 * Placement is `atDepth` near the end of the chat, the same home the state slice
 * defaults to: a block that changes with the state must not sit in the cached
 * prefix, or every generation would pay to re-read it.
 */

import { extension_prompt_roles, extension_prompt_types } from '../../extension-prompts.js';
// The binding policy is one piece of logic shared by every kind of state asset;
// only the candidate list differs, and that comes from `power-user.js`.
import { resolveStateDeclarationBinding as resolveBoundName } from '../../state-declaration-binding-policy.js';

/**
 * One place a predicate set can be bound, in priority order.
 *
 * @typedef {{scope: 'chat'|'character'|'group', name: string|undefined}} BindingCandidate
 */

/** All keys this module owns; anything else in `extension_prompts` is not ours. */
const STATE_PREDICATE_KEY_PREFIX = 'tauritavern_state_predicate_';

/** Where the entries land: close to the end, out of the cached prefix. */
const PREDICATE_INJECT_DEPTH = 4;

/**
 * The extension-prompt key one entry owns.
 *
 * Entries are keyed by their own id rather than by depth: they all share one
 * depth, and a key shared by two entries would silently keep only the last.
 *
 * @param {string} entryId
 * @returns {string}
 */
export function statePredicatePromptKey(entryId) {
    return `${STATE_PREDICATE_KEY_PREFIX}${entryId}`;
}

/**
 * Drop every block this module placed, leaving other extension prompts alone.
 *
 * @param {Record<string, any>} extensionPrompts
 * @returns {number} how many keys were removed
 */
export function flushStatePredicatePrompts(extensionPrompts) {
    let removed = 0;
    for (const key of Object.keys(extensionPrompts || {})) {
        if (key.startsWith(STATE_PREDICATE_KEY_PREFIX)) {
            delete extensionPrompts[key];
            removed += 1;
        }
    }
    return removed;
}

/**
 * Place the entries the host selected.
 *
 * @param {Array<{ entryId: string; content: string }>} entries
 * @param {{ extensionPrompts: Record<string, any>; setExtensionPrompt: (key: string, value: string, position: number, depth: number, scan?: boolean, role?: number) => void }} deps
 * @returns {number} how many entries were placed
 */
export function applyStatePredicateEntries(entries, deps) {
    flushStatePredicatePrompts(deps.extensionPrompts);

    let placed = 0;
    for (const entry of Array.isArray(entries) ? entries : []) {
        const text = String(entry?.content ?? '');
        const entryId = String(entry?.entryId ?? '').trim();
        if (!text || !entryId) {
            continue;
        }
        deps.setExtensionPrompt(
            statePredicatePromptKey(entryId),
            text,
            extension_prompt_types.IN_CHAT,
            PREDICATE_INJECT_DEPTH,
            false,
            extension_prompt_roles.SYSTEM,
        );
        placed += 1;
    }
    return placed;
}

/**
 * Ask the host which entries this chat's state selects.
 *
 * The binding is resolved here with the same policy every state asset uses —
 * chat header metadata first, then the character/group binding owned by
 * `power-user.js` — and a candidate only counts when its set still exists. Any
 * failure path answers with no entries: a missing block is a fact about the
 * chat, not an error to raise into the generation.
 *
 * A scene keeps its conditionally injected text with its fields, so a
 * declaration that carries a predicate set answers on its own: the named binding
 * is resolved only when the declaration has none.
 *
 * @param {{ chatRef: any; stableChatId: string; declaration?: any; safeInvoke: (command: string, args?: any) => Promise<any> }} input
 * @returns {Promise<Array<{ entryId: string; content: string }>>}
 */
export async function loadStatePredicateEntries(input) {
    const carried = input.declaration?.predicates;
    if (carried) {
        return await requestEntries(input, { set: carried });
    }

    /** @type {BindingCandidate[]} */
    let candidates = [];
    try {
        const powerUser = await import('../../power-user.js');
        candidates = /** @type {BindingCandidate[]} */ (powerUser.getStatePredicateBindingCandidates?.() ?? []);
    } catch (error) {
        console.warn('[state-predicates] binding candidates are unavailable; this generation runs without predicate entries', error);
        return [];
    }
    if (!Array.isArray(candidates) || candidates.length === 0) {
        return [];
    }

    let availableNames;
    try {
        availableNames = await input.safeInvoke('list_state_predicate_sets');
    } catch (error) {
        console.warn('[state-predicates] listing predicate sets failed; this generation runs without predicate entries', error);
        return [];
    }
    if (!Array.isArray(availableNames)) {
        return [];
    }

    const setName = resolveBoundName(candidates, availableNames);
    if (!setName) {
        return [];
    }

    return await requestEntries(input, { name: setName }, setName);
}

/**
 * Ask the host for the entries of one predicate set.
 *
 * A set that travelled inside the declaration is sent whole; a set bound on its
 * own is sent by name, so the host reads what was saved. A failure answers with
 * no entries: a missing block is a fact about the generation, not an error to
 * raise into it.
 *
 * @param {{ chatRef: any; stableChatId: string; safeInvoke: (command: string, args?: any) => Promise<any> }} input
 * @param {{ name?: string; set?: any }} target
 * @param {string} [label] what to name in the warning
 * @returns {Promise<Array<{ entryId: string; content: string }>>}
 */
async function requestEntries(input, target, label) {
    try {
        const result = await input.safeInvoke('get_state_predicate_entries', {
            dto: {
                chatRef: input.chatRef,
                stableChatId: input.stableChatId,
                ...target,
            },
        });
        return Array.isArray(result?.entries) ? result.entries : [];
    } catch (error) {
        console.warn(
            `[state-predicates] resolving entries of '${label || 'the declaration'}' failed; this generation runs without predicate entries`,
            error,
        );
        return [];
    }
}
