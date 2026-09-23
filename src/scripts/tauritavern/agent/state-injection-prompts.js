// @ts-check

/**
 * State injection into the assembled prompt.
 *
 * The split is deliberate: the host renders the slice (it owns the state
 * document, the declaration and the Profile's access), and this module places
 * it with the same extension-prompt machinery world book entries already use,
 * so placement reuses the machinery that decides where things land in the
 * message array.
 *
 * Blocks are registered as extension prompts before prompt assembly runs, and
 * flushed afterwards. Extension prompts are global, so a block that is not
 * flushed would leak into every later generation, including normal ones.
 */

import { extension_prompt_roles, extension_prompt_types } from '../../extension-prompts.js';

/** All keys this module owns; anything else in `extension_prompts` is not ours. */
const STATE_INJECTION_KEY_PREFIX = 'tauritavern_state_injection_';

/**
 * The extension-prompt key one block owns.
 *
 * Two blocks can share a slot only when they share its depth too (the host
 * merges by slot and depth), so slot and depth together identify a block.
 *
 * @param {{ slot: string; depth: number }} block
 * @returns {string}
 */
export function stateInjectionPromptKey(block) {
    return block.slot === 'atDepth'
        ? `${STATE_INJECTION_KEY_PREFIX}depth_${Number(block.depth) || 0}`
        : `${STATE_INJECTION_KEY_PREFIX}${block.slot}`;
}

/**
 * Drop every block this module placed, leaving other extension prompts alone.
 *
 * @param {Record<string, any>} extensionPrompts
 * @returns {number} how many keys were removed
 */
export function flushStateInjectionPrompts(extensionPrompts) {
    let removed = 0;
    for (const key of Object.keys(extensionPrompts || {})) {
        if (key.startsWith(STATE_INJECTION_KEY_PREFIX)) {
            delete extensionPrompts[key];
            removed += 1;
        }
    }
    return removed;
}

/**
 * Place the host's blocks into the prompt.
 *
 * Throws on a slot this build cannot place: the host and the frontend share the
 * slot vocabulary, so an unknown one means the two sides disagree about it, and
 * silently dropping the state would be the failure mode this whole design is
 * meant to remove.
 *
 * @param {Array<{ slot: string; depth: number; text: string }>} blocks
 * @param {{ extensionPrompts: Record<string, any>; setExtensionPrompt: (key: string, value: string, position: number, depth: number, scan?: boolean, role?: number) => void }} deps
 * @returns {number} how many blocks were placed
 */
export function applyStateInjectionBlocks(blocks, deps) {
    flushStateInjectionPrompts(deps.extensionPrompts);

    let placed = 0;
    for (const block of Array.isArray(blocks) ? blocks : []) {
        const text = String(block?.text ?? '');
        if (!text) {
            continue;
        }
        const slot = String(block?.slot ?? '');
        const depth = Number(block?.depth ?? 0) || 0;
        deps.setExtensionPrompt(
            stateInjectionPromptKey({ slot, depth }),
            text,
            stateInjectionSlotPosition(slot),
            slot === 'atDepth' ? depth : 0,
            false,
            extension_prompt_roles.SYSTEM,
        );
        placed += 1;
    }
    return placed;
}

/**
 * @param {string} slot
 * @returns {number}
 */
function stateInjectionSlotPosition(slot) {
    switch (slot) {
        case 'before':
            return extension_prompt_types.BEFORE_PROMPT;
        case 'after':
            return extension_prompt_types.IN_PROMPT;
        case 'atDepth':
            return extension_prompt_types.IN_CHAT;
        default:
            throw new Error(`state.injection_slot_unsupported: the host sent an unknown slot \`${slot}\``);
    }
}

/**
 * Ask the host for the slice this Profile may see.
 *
 * A missing block list is treated as no state rather than an error: the host
 * answers with a state id and zero blocks when a chat has committed no state
 * yet, and that is a fact about the chat, not a failure.
 *
 * The declaration travels with the request because a field's own default
 * decides injection when the Profile is silent, so the host cannot render the
 * slice from the access policy alone.
 *
 * @param {{ profileId?: string | null; chatRef: any; stableChatId: string; declaration?: any; safeInvoke: (command: string, args?: any) => Promise<any> }} input
 * @returns {Promise<Array<{ slot: string; depth: number; text: string }>>}
 */
export async function loadStateInjectionBlocks(input) {
    const profileId = String(input.profileId || '').trim();
    const result = await input.safeInvoke('get_state_injection', {
        dto: {
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            ...(profileId ? { profileId } : {}),
            ...(input.declaration ? { declaration: input.declaration } : {}),
        },
    });
    return Array.isArray(result?.blocks) ? result.blocks : [];
}
