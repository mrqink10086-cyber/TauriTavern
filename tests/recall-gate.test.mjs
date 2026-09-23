import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
    matchesRecallSource,
    recallBlocksPresent,
    waitForRecallBlocks,
} from '../src/scripts/tauritavern/agent/recall-gate.js';

/** A clock the test moves by hand, so a bounded wait is not a real one. */
function fakeClock() {
    let now = 0;
    return {
        now: () => now,
        advance: (ms) => {
            now += ms;
        },
    };
}

function storageOf(entries) {
    let prompts = entries;
    return {
        read: () => prompts,
        set: (next) => {
            prompts = next;
        },
    };
}

test('a source matches its key exactly, or by prefix when it ends in a star', () => {
    assert.equal(matchesRecallSource('3_vectfox', '3_vectfox'), true);
    assert.equal(matchesRecallSource('3_vectfox_pos2', '3_vectfox*'), true);
    assert.equal(matchesRecallSource('3_vectfox', '3_vectfox*'), true);
    assert.equal(matchesRecallSource('3_vectfox_pos2', '3_vectfox'), false);
    assert.equal(matchesRecallSource('3_vectfoxish', '3_vectfox'), false);
    assert.equal(matchesRecallSource('', '3_vectfox'), false);
});

test('a block that is present but empty does not count as recalled', () => {
    const outcome = recallBlocksPresent({ '3_vectfox': { value: '   ' } }, ['3_vectfox']);

    assert.deepEqual(outcome.present, []);
    assert.deepEqual(outcome.missing, ['3_vectfox']);
});

test('a source already recalled is reported ready without waiting', async () => {
    let slept = 0;
    const outcome = await waitForRecallBlocks(['3_vectfox*'], {
        read: () => ({ '3_vectfox_pos0': { value: 'recalled' } }),
        sleep: async () => {
            slept += 1;
        },
    });

    assert.equal(outcome.ready, true);
    assert.deepEqual(outcome.present, ['3_vectfox_pos0']);
    assert.equal(slept, 0, 'nothing to wait for means no wait');
});

test('a block that arrives late is picked up before the freeze', async () => {
    const store = storageOf({});

    let waited = 0;
    const outcome = await waitForRecallBlocks(['3_vectfox'], {
        read: store.read,
        timeoutMs: 1000,
        intervalMs: 10,
        sleep: async (ms) => {
            waited += ms;
            if (waited >= 20) {
                store.set({ '3_vectfox': { value: 'recalled after two polls' } });
            }
        },
    });

    assert.equal(outcome.ready, true);
    assert.deepEqual(outcome.present, ['3_vectfox']);
    assert.ok(waited >= 20, `expected at least two polls, waited ${waited}`);
});

test('a block that never arrives gives up at the ceiling and says what is missing', async () => {
    const clock = fakeClock();
    const realNow = Date.now;
    Date.now = clock.now;

    const outcome = await waitForRecallBlocks(['3_vectfox'], {
        read: () => ({}),
        timeoutMs: 100,
        intervalMs: 25,
        sleep: async (ms) => {
            clock.advance(ms);
        },
    });

    Date.now = realNow;

    assert.equal(outcome.ready, false);
    assert.deepEqual(outcome.missing, ['3_vectfox']);
    assert.ok(outcome.waitedMs >= 100, `the wait has to be bounded, got ${outcome.waitedMs}ms`);
});

test('no configured sources means nothing to wait for', async () => {
    const outcome = await waitForRecallBlocks([], { read: () => ({}) });

    assert.equal(outcome.ready, true);
    assert.deepEqual(outcome.sources, []);
});
