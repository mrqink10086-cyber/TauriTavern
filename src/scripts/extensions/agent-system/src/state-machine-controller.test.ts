import { afterEach, expect, test } from '@rstest/core';

import { createMachineConfigController, machineDraftIsDirty, type MachineConfigControllerDeps } from './state-machine-controller';
import type { MachineRunDto, MachineSpec } from './state-machine-model';
import { tr } from './PanelTestWorld';

function machineWorld() {
    const machines = new Map<string, MachineSpec>();
    const state = {
        saves: [] as Array<{ name: string; machine: MachineSpec }>,
        evaluations: [] as Array<{ active?: string[] | undefined; fields?: Record<string, string[]> | undefined }>,
        confirmations: [] as string[],
        confirm: true,
        errors: [] as unknown[],
        downloads: [] as Array<{ fileName: string; text: Promise<string> }>,
    };
    const deps: MachineConfigControllerDeps = {
        listMachines: () => Promise.resolve([...machines.keys()]),
        getMachine: (name) => {
            const found = machines.get(name);
            return found ? Promise.resolve(structuredClone(found)) : Promise.reject(new Error(`missing machine ${name}`));
        },
        saveMachine: (name, machine) => {
            state.saves.push({ name, machine });
            machines.set(name, structuredClone(machine));
            return Promise.resolve();
        },
        deleteMachine: (name) => {
            machines.delete(name);
            return Promise.resolve();
        },
        evaluate: (input) => {
            state.evaluations.push({ active: input.active, fields: input.fields });
            const run: MachineRunDto = {
                evaluation: {
                    active: ['night'],
                    applied: [{ index: 0, id: null, from: ['day'], to: ['night'] }],
                    skipped: [],
                    writes: [{ key: '环境/时间', values: ['夜'] }],
                    events: ['scene/ended'],
                    hooks: [],
                },
                errors: [],
            };
            return Promise.resolve(run);
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
    return { deps, machines, state };
}

const DISPOSABLES: Array<{ dispose: () => void }> = [];

/** The box's text, read back as the document it claims to be. */
function printedDocument(controller: ReturnType<typeof createMachineConfigController>): MachineSpec {
    return JSON.parse(controller.getSnapshot().draftJson) as MachineSpec;
}


afterEach(() => {
    DISPOSABLES.splice(0).forEach((controller) => controller.dispose());
});

/**
 * A new machine starts from the example; these tests build their own, so they
 * empty the draft the way the row-delete buttons would.
 */
function clearDraft(controller: ReturnType<typeof createMachineConfigController>): void {
    const draft = controller.getSnapshot().draft;
    if (!draft) {
        return;
    }
    for (let index = draft.transitions.length - 1; index >= 0; index -= 1) {
        controller.removeTransition(index);
    }
    for (let index = draft.states.length - 1; index >= 0; index -= 1) {
        controller.removeState(index);
    }
    controller.updateInitial('');
}

async function createAndSave(controller: ReturnType<typeof createMachineConfigController>, name: string): Promise<void> {
    controller.setNewName(name);
    await controller.createMachine();
    clearDraft(controller);
    controller.updateInitial('day');
    controller.addState();
    controller.updateState(0, { id: 'day', labelText: '白天' });
    controller.addState();
    controller.updateState(1, { id: 'night', terminal: true });
    controller.addTransition();
    controller.updateTransition(0, { fromText: 'day', toText: 'night' });
    controller.addCondition(0);
    controller.updateCondition(0, 0, { fieldText: '环境/时间', op: 'eq', valueText: '23:00' });
    controller.addAction(0);
    controller.updateAction(0, 0, { kind: 'emit', targetText: 'scene/ended' });
    await controller.save();
}

test('a new machine starts from the example instead of an empty editor', async () => {
    const { deps } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();

    controller.setNewName('scene-flow');
    await controller.createMachine();

    expect(controller.getSnapshot().notice).toContain('machineExampleLoaded');
    expect(controller.getSnapshot().draft?.states.length).toBeGreaterThan(0);
    expect(machineDraftIsDirty(controller.getSnapshot())).toBe(true);
});

test('a machine is created, saved as a normalized spec, and reloads identically', async () => {
    const { deps, machines, state } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();

    await createAndSave(controller, 'scene-flow');

    expect(state.saves).toHaveLength(1);
    const stored = machines.get('scene-flow');
    expect(stored?.initial).toEqual(['day']);
    expect(stored?.states.map((s) => s.id)).toEqual(['day', 'night']);
    expect(stored?.states[1]?.terminal).toBe(true);
    expect(stored?.transitions[0]?.conditions[0]).toEqual({
        source: 'field', field: '环境/时间', op: 'eq', value: '23:00',
    });
    expect(stored?.hooks ?? null).toBeNull();
    expect(controller.getSnapshot().notice).toContain('scene-flow');

    // Reload through a fresh selection: the stored spec is what comes back.
    controller.setNewName('other');
    await controller.createMachine();
    await controller.selectMachine('scene-flow');
    expect(controller.getSnapshot().draft?.initialText).toBe('day');
    expect(controller.getSnapshot().draft?.states.map((s) => s.id)).toEqual(['day', 'night']);
    expect(machineDraftIsDirty(controller.getSnapshot())).toBe(false);
});

test('switching away from a dirty draft asks, and a refusal keeps the draft', async () => {
    const { deps, state } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();

    controller.setNewName('scene-flow');
    await controller.createMachine();
    controller.updateInitial('day');
    expect(machineDraftIsDirty(controller.getSnapshot())).toBe(true);

    state.confirm = false;
    controller.setNewName('other');
    await controller.createMachine();
    expect(state.confirmations).toHaveLength(1);
    expect(controller.getSnapshot().selectedName).toBe('scene-flow');
    expect(controller.getSnapshot().draft?.initialText).toBe('day');

    state.confirm = true;
    await controller.createMachine();
    expect(controller.getSnapshot().selectedName).toBe('other');
});

test('the preview sends assumed inputs and shows the deterministic outcome', async () => {
    const { deps, state } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();

    controller.setNewName('scene-flow');
    await controller.createMachine();
    clearDraft(controller);
    controller.updateInitial('day');
    controller.addState();
    controller.updateState(0, { id: 'day' });

    controller.setPreviewActive('day');
    controller.addPreviewField();
    controller.updatePreviewField(0, { keyText: '环境/时间', valuesText: '23:00' });
    await controller.runPreview();

    expect(state.evaluations).toEqual([{
        active: ['day'],
        fields: { '环境/时间': ['23:00'] },
    }]);
    const preview = controller.getSnapshot().preview;
    expect(preview?.evaluation.active).toEqual(['night']);
    expect(preview?.evaluation.applied[0]?.to).toEqual(['night']);
    expect(preview?.evaluation.writes[0]?.key).toBe('环境/时间');
    expect(preview?.evaluation.events).toEqual(['scene/ended']);
    expect(preview?.errors).toEqual([]);
});

test('binding the selected machine writes it, then re-reads where it landed', async () => {
    const world = machineWorld();
    const toggles: Array<{ scope: string; name: string }> = [];
    let boundName = '';
    const controller = createMachineConfigController({
        ...world.deps,
        readBinding: () => Promise.resolve({
            chatAvailable: true,
            entityScope: null,
            chatName: boundName,
            entityName: '',
        }),
        toggleBinding: (scope, name) => {
            toggles.push({ scope, name });
            boundName = name;
            return Promise.resolve(true);
        },
    });
    DISPOSABLES.push(controller);
    await createAndSave(controller, 'scene-flow');

    await controller.bindSelectedTo('chat');

    expect(toggles).toEqual([{ scope: 'chat', name: 'scene-flow' }]);
    // The panel shows what the binding call actually did, not what it asked for.
    expect(controller.getSnapshot().binding?.chatName).toBe('scene-flow');
});

test('the JSON box prints the draft, and a pasted document replaces it', async () => {
    const { deps } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    controller.setNewName('flow');
    await controller.createMachine();

    // The box shows the document the save would write, not the editor's CSV text.
    expect(printedDocument(controller).states).toHaveLength(2);

    controller.updateState(0, { id: 'day', labelText: 'DAY' });
    expect(printedDocument(controller).states[0]).toEqual({ id: 'day', label: 'DAY' });

    controller.setDraftJson(JSON.stringify({
        initial: ['day'],
        states: [{ id: 'day' }, { id: 'night' }],
        transitions: [],
    }));
    controller.applyDraftJson();

    expect(controller.getSnapshot().jsonError).toBe('');
    expect(controller.getSnapshot().draft?.states.map((entry) => entry.id)).toEqual(['day', 'night']);
});

test('a paste that is not a document is reported without touching the draft', async () => {
    const { deps } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await controller.init();
    controller.setNewName('flow');
    await controller.createMachine();
    const before = controller.getSnapshot().draft;

    controller.setDraftJson('{ not json');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).not.toBe('');
    expect(controller.getSnapshot().draft).toBe(before);

    controller.setDraftJson('"day"');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).toContain('jsonDocumentMustBeObject');
    expect(controller.getSnapshot().draft).toBe(before);
});

test('a machine leaves as a file and comes back as one', async () => {
    const { deps, state } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await createAndSave(controller, 'scene-flow');

    await controller.exportMachine();

    const download = state.downloads.at(0);
    expect(download?.fileName).toBe('scene-flow.machine.json');
    const text = (await download?.text) ?? '';

    // A second editor, on a machine that never saw this one.
    const other = machineWorld();
    const otherController = createMachineConfigController(other.deps);
    DISPOSABLES.push(otherController);
    await otherController.init();
    await otherController.importMachine(text);

    expect(other.machines.has('scene-flow')).toBe(true);
    expect(otherController.getSnapshot().selectedName).toBe('scene-flow');
    expect(other.machines.get('scene-flow')?.transitions[0]?.conditions[0]).toEqual({
        source: 'field', field: '环境/时间', op: 'eq', value: '23:00',
    });
});

test('importing over an existing machine asks first', async () => {
    const { deps, machines, state } = machineWorld();
    const controller = createMachineConfigController(deps);
    DISPOSABLES.push(controller);
    await createAndSave(controller, 'scene-flow');
    const file = JSON.stringify({
        kind: 'tauritavern.state-machine',
        version: 1,
        name: 'scene-flow',
        machine: { initial: ['dusk'], states: [{ id: 'dusk' }], transitions: [] },
    });

    state.confirm = false;
    await controller.importMachine(file);
    expect(machines.get('scene-flow')?.initial).toEqual(['day']);

    state.confirm = true;
    await controller.importMachine(file);
    expect(machines.get('scene-flow')?.initial).toEqual(['dusk']);
});
