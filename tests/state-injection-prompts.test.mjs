import assert from 'node:assert/strict';
import { test } from 'node:test';

import { extension_prompt_types } from '../src/scripts/extension-prompts.js';
import {
    applyStateInjectionBlocks,
    flushStateInjectionPrompts,
    loadStateInjectionBlocks,
    stateInjectionPromptKey,
} from '../src/scripts/tauritavern/agent/state-injection-prompts.js';

function recorder(extensionPrompts = {}) {
    const written = [];
    return {
        extensionPrompts,
        written,
        setExtensionPrompt(key, value, position, depth, scan = false, role = 0) {
            extensionPrompts[key] = { value, position, depth, scan, role };
            written.push({ key, value, position, depth });
        },
    };
}

test('blocks land at the position their slot names', () => {
    const target = recorder();

    const placed = applyStateInjectionBlocks(
        [
            { slot: 'before', depth: 0, text: '角色/角色甲/外貌: 短发' },
            { slot: 'after', depth: 0, text: '环境/日期: 2026/09/10' },
            { slot: 'atDepth', depth: 4, text: '环境/时间: 下午' },
        ],
        target,
    );

    assert.equal(placed, 3);
    assert.deepEqual(
        target.written.map((entry) => [entry.position, entry.depth]),
        [
            [extension_prompt_types.BEFORE_PROMPT, 0],
            [extension_prompt_types.IN_PROMPT, 0],
            [extension_prompt_types.IN_CHAT, 4],
        ],
    );
});

test('a deeper block is a different key from a shallower one', () => {
    assert.notEqual(
        stateInjectionPromptKey({ slot: 'atDepth', depth: 4 }),
        stateInjectionPromptKey({ slot: 'atDepth', depth: 6 }),
    );
});

test('flushing leaves every extension prompt this module does not own', () => {
    const target = recorder();
    target.extensionPrompts['2_floating_prompt'] = { value: 'author note' };
    applyStateInjectionBlocks([{ slot: 'after', depth: 0, text: '环境/日期: 2026/09/10' }], target);

    const removed = flushStateInjectionPrompts(target.extensionPrompts);

    assert.equal(removed, 1);
    assert.deepEqual(Object.keys(target.extensionPrompts), ['2_floating_prompt']);
});

test('reapplying replaces the previous slice instead of stacking it', () => {
    const target = recorder();
    applyStateInjectionBlocks(
        [
            { slot: 'after', depth: 0, text: '环境/日期: 2026/09/10' },
            { slot: 'atDepth', depth: 4, text: '环境/时间: 下午' },
        ],
        target,
    );

    applyStateInjectionBlocks([{ slot: 'after', depth: 0, text: '环境/日期: 2026/09/11' }], target);

    assert.deepEqual(Object.keys(target.extensionPrompts).length, 1);
    assert.equal(target.extensionPrompts['tauritavern_state_injection_after'].value, '环境/日期: 2026/09/11');
});

test('a block with no text is not placed', () => {
    const target = recorder();

    assert.equal(applyStateInjectionBlocks([{ slot: 'after', depth: 0, text: '' }], target), 0);
    assert.deepEqual(target.written, []);
});

test('a slot this build cannot place stops instead of dropping state', () => {
    const target = recorder();

    assert.throws(
        () => applyStateInjectionBlocks([{ slot: 'anTop', depth: 0, text: '环境/日期: 2026/09/10' }], target),
        /state\.injection_slot_unsupported/,
    );
});

test('the host is asked for the slice of the running profile in this chat', async () => {
    const calls = [];
    const safeInvoke = async (command, args) => {
        calls.push({ command, args });
        return { stateId: 'state-1', blocks: [{ slot: 'after', depth: 0, text: '环境/日期: 2026/09/10' }] };
    };
    const chatRef = { kind: 'character', characterId: 'alice', fileName: 'alice.png' };

    const blocks = await loadStateInjectionBlocks({
        profileId: ' default-writer ',
        chatRef,
        stableChatId: 'stable-1',
        safeInvoke,
    });

    assert.deepEqual(calls, [{
        command: 'get_state_injection',
        args: { dto: { chatRef, stableChatId: 'stable-1', profileId: 'default-writer' } },
    }]);
    assert.equal(blocks.length, 1);
});

test('a chat with no committed state yields no blocks', async () => {
    const blocks = await loadStateInjectionBlocks({
        chatRef: { kind: 'group', chatId: 'group-1' },
        stableChatId: 'stable-2',
        safeInvoke: async () => ({ stateId: null, blocks: [] }),
    });

    assert.deepEqual(blocks, []);
});

test('an unexpected host answer is read as no state, not as blocks', async () => {
    const blocks = await loadStateInjectionBlocks({
        chatRef: { kind: 'group', chatId: 'group-1' },
        stableChatId: 'stable-3',
        safeInvoke: async () => ({ stateId: 'state-2', blocks: 'nonsense' }),
    });

    assert.deepEqual(blocks, []);
});
