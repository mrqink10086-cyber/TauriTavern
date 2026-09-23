import { afterEach, expect, test } from '@rstest/core';

import {
    createPredicateConfigController,
    predicateDraftIsDirty,
    type PredicateConfigControllerDeps,
} from './state-predicate-controller';
import type { PredicateEvaluationDto, StatePredicateSet } from './state-predicate-model';
import { DEFAULT_PREDICATE_NAME } from './state-examples';
import { tr } from './PanelTestWorld';

function predicateWorld(sets: Map<string, StatePredicateSet> = new Map()) {
    const state = {
        saves: [] as Array<{ name: string; set: StatePredicateSet }>,
        evaluations: [] as Array<{ set: StatePredicateSet; fields: Record<string, string[]> }>,
        confirmations: [] as string[],
        confirm: true,
        errors: [] as unknown[],
        downloads: [] as Array<{ fileName: string; text: Promise<string> }>,
    };
    const deps: PredicateConfigControllerDeps = {
        listSets: () => Promise.resolve([...sets.keys()]),
        getSet: (name) => {
            const found = sets.get(name);
            return found ? Promise.resolve(structuredClone(found)) : Promise.reject(new Error(`missing set ${name}`));
        },
        saveSet: (name, set) => {
            state.saves.push({ name, set });
            sets.set(name, structuredClone(set));
            return Promise.resolve();
        },
        deleteSet: (name) => {
            sets.delete(name);
            return Promise.resolve();
        },
        evaluate: (input) => {
            state.evaluations.push({ set: structuredClone(input.set), fields: input.fields ?? {} });
            const evaluation: PredicateEvaluationDto = {
                selected: [{ groupId: 'tone', entryId: 'close', content: '可以靠近。' }],
                skipped: [{ entryId: 'distant', reason: 'group_lost' }],
            };
            return Promise.resolve(evaluation);
        },
        confirmAction: (message) => {
            state.confirmations.push(message);
            return Promise.resolve(state.confirm);
        },
        notifyError: (error) => {
            state.errors.push(error);
        },
        notifySuccess: () => undefined,
        downloadBlob: (blob, fileName) => {
            state.downloads.push({ fileName, text: blob.text() });
            return Promise.resolve({ mode: 'browser-download', completed: true });
        },
        readBinding: () => Promise.resolve(null),
        toggleBinding: () => Promise.resolve(false),
        tr,
    };
    return { deps, sets, state };
}

const DISPOSABLES: Array<{ dispose: () => void }> = [];

/** The box's text, read back as the document it claims to be. */
function printedDocument(controller: ReturnType<typeof createPredicateConfigController>): StatePredicateSet {
    return JSON.parse(controller.getSnapshot().draftJson) as StatePredicateSet;
}


afterEach(() => {
    DISPOSABLES.splice(0).forEach((controller) => controller.dispose());
});

function controllerFor(deps: PredicateConfigControllerDeps) {
    const controller = createPredicateConfigController(deps);
    DISPOSABLES.push(controller);
    return controller;
}

test('a new set starts from the example and starts dirty', async () => {
    const { deps, state } = predicateWorld();
    const controller = controllerFor(deps);
    await controller.init();

    await controller.createSet();

    const snapshot = controller.getSnapshot();
    expect(snapshot.selectedName).toBe(DEFAULT_PREDICATE_NAME);
    expect(snapshot.draft?.groups).toHaveLength(1);
    expect(snapshot.draft?.constants).toHaveLength(1);
    expect(predicateDraftIsDirty(snapshot)).toBe(true);
    expect(state.confirmations).toHaveLength(0);
});

test('save sends the normalized document and settles the dirty flag', async () => {
    const { deps, state } = predicateWorld();
    const controller = controllerFor(deps);
    await controller.init();
    await controller.createSet();

    await controller.save();

    const [saved] = state.saves;
    expect(saved?.name).toBe(DEFAULT_PREDICATE_NAME);
    // The example's group and its standing entry both survive the round trip.
    expect(saved?.set.groups?.[0]?.entries?.map((entry) => entry.id)).toEqual(['distant', 'close']);
    expect(saved?.set.constants?.map((entry) => entry.id)).toEqual(['standing']);
    expect(predicateDraftIsDirty(controller.getSnapshot())).toBe(false);
});

test('a failed save is reported instead of claiming success', async () => {
    const { deps, state } = predicateWorld();
    deps.saveSet = () => Promise.reject(new Error('the backend refused'));
    const controller = controllerFor(deps);
    await controller.init();
    await controller.createSet();

    await controller.save();

    expect(controller.getSnapshot().error).toContain('the backend refused');
    expect(state.errors).toHaveLength(1);
});

test('the preview sends the draft and the assumed values, and stores nothing', async () => {
    const { deps, state } = predicateWorld();
    const controller = controllerFor(deps);
    await controller.init();
    await controller.createSet();

    controller.addPreviewField();
    controller.updatePreviewField(0, { keyText: '环境/时间', valuesText: '晚上, 深夜' });
    await controller.runPreview();

    const [evaluation] = state.evaluations;
    expect(evaluation?.fields).toEqual({ '环境/时间': ['晚上', '深夜'] });
    expect(state.saves).toHaveLength(0);
    const snapshot = controller.getSnapshot();
    expect(snapshot.preview?.selected.map((selected) => selected.entryId)).toEqual(['close']);
    expect(snapshot.preview?.skipped[0]?.reason).toBe('group_lost');
});

test('an edit inside a group lands on that group only', async () => {
    const { deps } = predicateWorld();
    const controller = controllerFor(deps);
    await controller.init();
    await controller.createSet();

    controller.addGroup();
    controller.updateGroup(1, { id: 'weather' });
    controller.addEntry({ kind: 'group', groupIndex: 1 });
    controller.updateEntry({ kind: 'group', groupIndex: 1 }, 0, { id: 'rainy', content: '下雨。' });
    controller.updateEntry({ kind: 'group', groupIndex: 1 }, 0, { sourceKind: 'state' });
    controller.updateEntryCondition({ kind: 'group', groupIndex: 1 }, 0, {
        fieldText: '环境/天气',
        op: 'eq',
        valueText: '雨',
    });

    const draft = controller.getSnapshot().draft;
    expect(draft?.groups?.[1]?.id).toBe('weather');
    expect(draft?.groups?.[1]?.entries?.[0]?.condition).toEqual({
        fieldText: '环境/天气',
        op: 'eq',
        valueText: '雨',
    });
    expect(draft?.groups?.[0]?.entries?.[0]?.id).toBe('distant');
});

test('switching a source to a state field without a field is flagged before saving', async () => {
    const { deps } = predicateWorld();
    const controller = controllerFor(deps);
    await controller.init();
    await controller.createSet();

    controller.updateEntry({ kind: 'constants' }, 0, { sourceKind: 'state' });
    controller.updateEntryCondition({ kind: 'constants' }, 0, { fieldText: '' });

    expect(controller.getSnapshot().issues.map((issue) => issue.code))
        .toContain('entryConditionIncomplete');
});

test('an unsaved draft asks before it is thrown away', async () => {
    const { deps, state } = predicateWorld(new Map([
        ['set-a', { groups: [], constants: [] }],
        ['set-b', { groups: [], constants: [] }],
    ]));
    const controller = controllerFor(deps);
    await controller.init();
    controller.addGroup();
    controller.updateGroup(0, { id: 'edited' });
    state.confirm = false;

    await controller.selectSet('set-b');

    expect(state.confirmations).toHaveLength(1);
    expect(controller.getSnapshot().selectedName).toBe('set-a');
});

test('the JSON box prints the draft, and a pasted document replaces it', async () => {
    const { deps } = predicateWorld();
    const controller = createPredicateConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    controller.setNewName('tones');
    await controller.createSet();

    // The example the tab opens with, as the document it would be stored as.
    expect(printedDocument(controller).groups).toHaveLength(1);

    controller.updateEntry({ kind: 'group', groupIndex: 0 }, 0, { content: '保持距离。' });
    expect(printedDocument(controller).groups?.[0]?.entries?.[0]?.content).toBe('保持距离。');

    controller.setDraftJson(JSON.stringify({ groups: [{ id: 'tone', entries: [{ id: 'close', content: '可以靠近。' }] }] }));
    controller.applyDraftJson();

    expect(controller.getSnapshot().jsonError).toBe('');
    expect(controller.getSnapshot().draft?.groups[0]?.entries[0]?.content).toBe('可以靠近。');
});

test('a paste that is not a document is reported without touching the draft', async () => {
    const { deps } = predicateWorld();
    const controller = createPredicateConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    controller.setNewName('tones');
    await controller.createSet();
    const before = controller.getSnapshot().draft;

    controller.setDraftJson('{ not json');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).not.toBe('');
    expect(controller.getSnapshot().draft).toBe(before);

    controller.setDraftJson('[]');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).toContain('jsonDocumentMustBeObject');
    expect(controller.getSnapshot().draft).toBe(before);
});

test('a set leaves as a file and comes back as one', async () => {
    const { deps, state } = predicateWorld();
    const controller = createPredicateConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    controller.setNewName('tones');
    await controller.createSet();
    await controller.save();

    await controller.exportPredicateSet();

    const download = state.downloads.at(0);
    expect(download?.fileName).toBe('tones.predicates.json');
    const text = (await download?.text) ?? '';

    // A second editor, on a machine that never saw this set.
    const other = predicateWorld();
    const otherController = createPredicateConfigController(other.deps);
    DISPOSABLES.push(otherController);
    await otherController.init();
    await otherController.importPredicateSet(text);

    expect(other.sets.has('tones')).toBe(true);
    expect(otherController.getSnapshot().selectedName).toBe('tones');
    expect(other.sets.get('tones')?.groups?.[0]?.entries?.map((entry) => entry.id))
        .toEqual(['distant', 'close']);
});

test('importing over an existing set asks first', async () => {
    const { deps, sets, state } = predicateWorld(new Map([['tones', { groups: [] }]]));
    const controller = createPredicateConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    const file = JSON.stringify({
        kind: 'tauritavern.state-predicates',
        version: 1,
        name: 'tones',
        set: { groups: [{ id: 'tone', entries: [{ id: 'close', content: '可以靠近。' }] }] },
    });

    state.confirm = false;
    await controller.importPredicateSet(file);
    expect(sets.get('tones')?.groups).toEqual([]);

    state.confirm = true;
    await controller.importPredicateSet(file);
    expect(sets.get('tones')?.groups?.[0]?.entries?.[0]?.id).toBe('close');
});
