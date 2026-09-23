import { expect, test } from '@rstest/core';

import { createStateFileOps } from './state-config-file-ops';
import type { StateDeclaration } from './state-config-model';
import { scriptModulesOf } from './state-config-ops';

const tr = (key: string, params: Record<string, unknown> = {}): string => [
    key,
    ...Object.entries(params).map(([name, value]) => (
        `${name}=${typeof value === 'string' ? value : JSON.stringify(value)}`
    )),
].join(' ');

/**
 * The two ways a file reaches a draft, without the editor around them.
 *
 * Both end in the same place — a changed draft or a sentence saying why not —
 * so what is worth testing is which of the two happened.
 */
function opsWorld(pickFilePath: ((extensions: readonly string[]) => Promise<string | null>) | null = null) {
    const state = {
        draft: { fields: [] } as StateDeclaration,
        commits: [] as Array<{ error?: string; notice?: string }>,
    };
    const ops = createStateFileOps({
        currentDraft: () => state.draft,
        pickFilePath,
        applyDraft: (change) => {
            state.draft = change(state.draft);
        },
        commit: (patch) => {
            state.commits.push(patch);
        },
        isDisposed: () => false,
        tr,
    });
    return { ops, state };
}

test('a module is stored under the name of the file it came from', () => {
    const { ops, state } = opsWorld();

    ops.importScriptModule('pick.js', 'export default () => 1;');

    expect(scriptModulesOf(state.draft)).toEqual({ 'pick.js': 'export default () => 1;' });
    expect(state.commits.at(-1)?.notice).toContain('stateDeclarationScriptImported');
});

test('a file name that cannot be a module name is refused, and the draft is untouched', () => {
    const { ops, state } = opsWorld();

    ops.importScriptModule('my script.js', 'export default () => 1;');

    expect(scriptModulesOf(state.draft)).toEqual({});
    expect(state.commits.at(-1)?.error).toContain('stateDeclarationScriptNameInvalid');
});

test('a second module under a name already in use is refused', () => {
    const { ops, state } = opsWorld();
    ops.importScriptModule('pick.js', 'first');
    const afterFirst = state.draft;

    ops.importScriptModule('pick.js', 'second');

    expect(state.draft).toBe(afterFirst);
    expect(state.commits.at(-1)?.error).toContain('stateDeclarationScriptExists');
});

test('choosing a vocabulary file points the limits at the path it returned', async () => {
    const { ops, state } = opsWorld(() => Promise.resolve('/tmp/vocab.json'));

    await ops.chooseVocabularyFile();

    expect(state.draft.limits?.tokenizer).toBe('file:/tmp/vocab.json');
});

test('a window with no file dialog says so instead of doing nothing', async () => {
    const { ops, state } = opsWorld(null);

    await ops.chooseVocabularyFile();

    expect(state.commits.at(-1)?.error).toContain('stateLimitsTokenizerPickUnavailable');
});

test('a cancelled dialog leaves the vocabulary where it was', async () => {
    const { ops, state } = opsWorld(() => Promise.resolve(null));
    state.draft = { fields: [], limits: { tokenizer: 'glm' } };

    await ops.chooseVocabularyFile();

    expect(state.draft.limits?.tokenizer).toBe('glm');
});
