import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveStateDeclarationBinding } from '../src/scripts/state-declaration-binding-policy.js';

/**
 * The policy is scope-agnostic: it walks the candidate list in order and takes
 * the first name that still exists. Only `chat` and `character` are exercised
 * here — the `group` scope the candidate list can carry comes from the shared
 * theme entity helper, and the Agent has no group mode today, so nothing
 * depends on it resolving.
 */
test('state declaration binding takes the first candidate that still exists', () => {
    const candidates = [
        { scope: 'chat', name: 'Chat plan' },
        { scope: 'character', name: 'Character plan' },
    ];

    assert.equal(resolveStateDeclarationBinding(candidates, ['Chat plan', 'Character plan']), 'Chat plan');
    assert.equal(resolveStateDeclarationBinding(candidates, ['Character plan']), 'Character plan');
});

test('state declaration binding returns null when nothing resolves', () => {
    const bound = [
        { scope: 'chat', name: 'Deleted plan' },
        { scope: 'character', name: 'Other deleted plan' },
    ];
    assert.equal(resolveStateDeclarationBinding(bound, []), null);

    const unbound = [{ scope: 'chat', name: undefined }, { scope: 'character', name: '' }];
    assert.equal(resolveStateDeclarationBinding(unbound, ['Anything']), null);

    assert.equal(resolveStateDeclarationBinding([], ['Anything']), null);
});
