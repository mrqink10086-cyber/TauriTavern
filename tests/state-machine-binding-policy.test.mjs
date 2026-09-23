import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveStateMachineBinding } from '../src/scripts/state-machine-binding-policy.js';

/**
 * The policy mirrors the state declaration binding: walk the candidates in
 * order and take the first name that still exists. Only `chat` and `character`
 * are exercised here — the `group` scope comes from the shared theme entity
 * helper, and the Agent has no group mode today, so nothing depends on it.
 */
test('state machine binding takes the first candidate that still exists', () => {
    const candidates = [
        { scope: 'chat', name: 'Chat flow' },
        { scope: 'character', name: 'Character flow' },
    ];

    assert.equal(resolveStateMachineBinding(candidates, ['Chat flow', 'Character flow']), 'Chat flow');
    assert.equal(resolveStateMachineBinding(candidates, ['Character flow']), 'Character flow');
});

test('state machine binding returns null when nothing resolves', () => {
    const bound = [
        { scope: 'chat', name: 'Deleted flow' },
        { scope: 'character', name: 'Other deleted flow' },
    ];
    assert.equal(resolveStateMachineBinding(bound, []), null);

    const unbound = [{ scope: 'chat', name: undefined }, { scope: 'character', name: '' }];
    assert.equal(resolveStateMachineBinding(unbound, ['Anything']), null);

    assert.equal(resolveStateMachineBinding([], ['Anything']), null);
});
