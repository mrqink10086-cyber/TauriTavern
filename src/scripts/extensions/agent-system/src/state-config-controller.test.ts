import { expect, test } from '@rstest/core';

import type { StateDeclaration } from './state-config-model';
import { createStateConfigController, stateConfigDraftIsDirty } from './state-config-controller';
import type { StateImageTarget } from './state-config-ops';

const tr = (key: string, params: Record<string, unknown> = {}): string => [
    key,
    ...Object.entries(params).map(([name, value]) => `${name}=${typeof value === 'string' ? value : JSON.stringify(value)}`),
].join(' ');

function createWorld(declarations: Record<string, StateDeclaration> = {}) {
    const stored = new Map(Object.entries(declarations).map(([name, value]) => [name, structuredClone(value)]));
    const saves: Array<{ name: string; declaration: StateDeclaration }> = [];
    const confirmations: string[] = [];
    let confirmAnswer = true;

    return {
        stored,
        saves,
        confirmations,
        setConfirmAnswer(answer: boolean) {
            confirmAnswer = answer;
        },
        deps: {
            listDeclarations: () => Promise.resolve([...stored.keys()]),
            getDeclaration: (name: string) => {
                const declaration = stored.get(name);
                return declaration
                    ? Promise.resolve(structuredClone(declaration))
                    : Promise.reject(new Error(`missing declaration ${name}`));
            },
            saveDeclaration: (name: string, declaration: StateDeclaration) => {
                stored.set(name, structuredClone(declaration));
                saves.push({ name, declaration: structuredClone(declaration) });
                return Promise.resolve();
            },
            deleteDeclaration: (name: string) => {
                stored.delete(name);
                return Promise.resolve();
            },
            confirmAction: (message: string) => {
                confirmations.push(message);
                return Promise.resolve(confirmAnswer);
            },
            notifyError: () => undefined,
            notifySuccess: () => undefined,
            downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
            pickFilePath: null,
            readBinding: () => Promise.resolve(null),
            toggleBinding: () => Promise.resolve(false),
            tr,
        },
    };
}

function declarationWithBackground(): StateDeclaration {
    return {
        fields: [{ pattern: '环境/日期', label: 'DATE' }],
        panels: {
            panels: [{
                title: 'Environment',
                match: '环境/**',
                background: { candidates: [{ source: '/backgrounds/room.png' }] },
            }],
        },
    };
}

test('a list comparison is stored as values, not as one string', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    const target = { kind: 'background', panelIndex: 0 } as const;
    controller.addImageCandidate(target);
    controller.setImageCandidateSource(target, 1, '/backgrounds/night.png');
    controller.setImageCandidateCondition(target, 1, {
        field: '环境/时间',
        op: 'in',
        valueText: '夜晚, 清晨',
    });
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.background?.candidates[1]).toEqual({
        source: '/backgrounds/night.png',
        when: { source: 'field', field: '环境/时间', op: 'in', values: ['夜晚', '清晨'] },
    });
});

test('a picture edit keeps the parts of the set it did not touch', async () => {
    // The set is rebuilt whole on every keystroke, so a part the edit forgets is
    // gone the next time somebody types. `fit` and the condition script are both
    // parts a rebuild went through without carrying.
    const world = createWorld({
        scene: {
            fields: [{ pattern: '环境/日期', label: 'DATE' }],
            panels: {
                panels: [{
                    title: 'Environment',
                    match: '环境/**',
                    background: {
                        candidates: [{ source: '/backgrounds/room.png' }],
                        fit: 'contain',
                        conditionScript: { script: 'export default () => 0;' },
                    },
                }],
            },
        },
    });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    const target = { kind: 'background', panelIndex: 0 } as const;
    controller.setImageCandidateSource(target, 0, '/backgrounds/hall.png');
    expect(controller.getSnapshot().draft?.panels?.panels[0]?.background).toEqual({
        candidates: [{ source: '/backgrounds/hall.png' }],
        fit: 'contain',
        conditionScript: { script: 'export default () => 0;' },
    });

    // Choosing what an absent fit already means stores nothing, and choosing the
    // departure again brings it back.
    controller.setImageFit(target, 'cover');
    expect(controller.getSnapshot().draft?.panels?.panels[0]?.background?.fit).toBeUndefined();

    controller.setImageFit(target, 'contain');
    await controller.save();
    expect(world.saves[0]?.declaration.panels?.panels[0]?.background?.fit).toBe('contain');
});

test('a single comparison keeps its value and drops the list', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setImageCandidateCondition({ kind: 'background', panelIndex: 0 }, 0, {
        field: '环境/时间',
        op: 'eq',
        valueText: '夜晚',
    });
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.background?.candidates[0]?.when).toEqual({
        source: 'field',
        field: '环境/时间',
        op: 'eq',
        value: '夜晚',
    });
});

test('a blank row the user has not filled in does not count as a change', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(false);

    controller.addField();
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(false);

    controller.updateField(1, { pattern: '环境/地点' });
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(true);
});

test('switching declarations asks before the edits are thrown away', async () => {
    const world = createWorld({
        first: declarationWithBackground(),
        second: { fields: [{ pattern: '环境/地点', label: 'PLACE' }] },
    });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    expect(controller.getSnapshot().selectedName).toBe('first');
    controller.updateField(0, { label: 'DAY' });

    world.setConfirmAnswer(false);
    await controller.selectDeclaration('second');
    expect(controller.getSnapshot().selectedName).toBe('first');
    expect(world.confirmations).toHaveLength(1);

    world.setConfirmAnswer(true);
    await controller.selectDeclaration('second');
    expect(controller.getSnapshot().selectedName).toBe('second');
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(false);
});

test('the tab can be opened again without losing unsaved edits', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.updateField(0, { label: 'DAY' });
    await controller.init();

    expect(controller.getSnapshot().selectedName).toBe('scene');
    expect(controller.getSnapshot().draft?.fields[0]?.label).toBe('DAY');
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(true);
});

test('a new declaration starts from the example and can be saved under a new name', async () => {
    const world = createWorld({});
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewName(' scene ');
    await controller.createDeclaration();
    expect(controller.getSnapshot().selectedName).toBe('scene');
    expect(controller.getSnapshot().notice).toContain('stateDeclarationExampleLoaded');
    expect(controller.getSnapshot().draft?.fields.length).toBeGreaterThan(0);
    expect(controller.getSnapshot().draft?.panels?.panels.length).toBeGreaterThan(0);
    // Nothing is stored under this name yet, so the example starts dirty.
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(true);

    controller.updateField(0, { pattern: '环境/日期', label: 'DATE' });
    await controller.save();

    expect(controller.getSnapshot().names).toEqual(['scene']);
    expect(stateConfigDraftIsDirty(controller.getSnapshot())).toBe(false);
    expect(world.stored.get('scene')?.fields[0]).toEqual({
        pattern: '环境/日期',
        label: 'DATE',
    });
});

test('renaming a row keeps the values it starts out holding', async () => {
    const world = createWorld({
        scene: { fields: [{ pattern: '环境/日期', label: 'DATE', initial: ['春天 · 第 3 天'] }] },
    });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.updateField(0, { pattern: '环境/时刻', label: 'TIME' });
    await controller.save();

    expect(world.stored.get('scene')?.fields[0]).toEqual({
        pattern: '环境/时刻',
        label: 'TIME',
        initial: ['春天 · 第 3 天'],
    });
});

test('a declaration that was created but not saved survives a refresh', async () => {
    // The tab re-runs init on activation, and the new name is not stored yet:
    // the draft must not be mistaken for one the backend deleted.
    const world = createWorld({});
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewName('draft');
    await controller.createDeclaration();
    expect(controller.getSnapshot().selectedName).toBe('draft');

    await controller.init();

    expect(controller.getSnapshot().selectedName).toBe('draft');
    expect(controller.getSnapshot().draft?.fields.length).toBeGreaterThan(0);
});

test('a name that already exists is refused before anything is overwritten', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewName('scene');
    await controller.createDeclaration();

    expect(controller.getSnapshot().selectedName).toBe('scene');
    expect(controller.getSnapshot().error).toContain('stateDeclarationExists');
    expect(controller.getSnapshot().draft?.panels?.panels[0]?.title).toBe('Environment');
});

test('removing the last picture drops the set instead of storing it empty', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.removeImageCandidate({ kind: 'background', panelIndex: 0 }, 0);
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]).not.toHaveProperty('background');
});

test('a theme survives the edits that are about something else', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setThemeCss('.tt-state-field { color: red }');
    // Both ways the panel config gets rebuilt: panels, and shared modules.
    controller.updatePanel(0, { title: 'Environment and Weather' });
    controller.setNewScriptName('scene.js');
    controller.createScriptModule();
    controller.setScriptModuleSource('scene.js', 'export default () => ({ index: 0 });');
    await controller.save();

    const stored = world.saves[0]?.declaration.panels;
    expect(stored?.css).toContain('.tt-state-root .tt-state-field');
    expect(stored?.scripts).toEqual({ 'scene.js': 'export default () => ({ index: 0 });' });
});

test('a theme that cannot be compiled stops the save instead of storing half of it', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setThemeCss('@import url(/backgrounds/theme.css);');
    await controller.save();

    expect(world.saves).toHaveLength(0);
    expect(controller.getSnapshot().error).toContain('stateDeclarationSaveBlocked');
});

test('a stored template is printed back for editing, and saves as it was', async () => {
    const world = createWorld({
        scene: {
            fields: [{ pattern: '环境/日期', label: 'DATE' }],
            panels: {
                panels: [{
                    title: '环境',
                    match: '环境/**',
                    markup: { nodes: [{ kind: 'value', key: '环境/日期' }] },
                }],
            },
        },
    });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    // A document carries the compiled tree; the editor edits the text.
    expect(controller.getSnapshot().draft?.panels?.panels[0]?.templateSource)
        .toBe('{{value 环境/日期}}');

    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.markup)
        .toEqual({ nodes: [{ kind: 'value', key: '环境/日期' }] });
});

test('a template that cannot be compiled stops the save', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setPanelTemplate(0, '<script>alert(1)</script>');
    await controller.save();

    expect(world.saves).toHaveLength(0);
    expect(controller.getSnapshot().error).toContain('stateDeclarationSaveBlocked');
});

const background: StateImageTarget = { kind: 'background', panelIndex: 0 };

test('a condition script travels with the set it belongs to', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setImageConditionScript(background, 'export default () => ({ index: 0 });');
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.background?.conditionScript).toEqual({
        script: 'export default () => ({ index: 0 });',
    });
});

test('a blank script is dropped, because that is what "no script" means', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setImageConditionScript(background, '   ');
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.background).not.toHaveProperty('conditionScript');
});

test('editing a picture keeps the set script', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setImageConditionScript(background, 'export default () => ({ index: 0 });');
    controller.addImageCandidate(background);
    controller.setImageCandidateSource(background, 1, '/backgrounds/garden.png');
    await controller.save();

    const stored = world.saves[0]?.declaration.panels?.panels[0]?.background;
    expect(stored?.conditionScript).toEqual({ script: 'export default () => ({ index: 0 });' });
    expect(stored?.candidates.map((candidate) => candidate.source)).toEqual([
        '/backgrounds/room.png',
        '/backgrounds/garden.png',
    ]);
});

test('a shared module is stored with the declaration it belongs to', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewScriptName('scene.js');
    controller.createScriptModule();
    controller.setScriptModuleSource('scene.js', 'export function pick({ state }) { return { index: 0 }; }');
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.scripts).toEqual({
        'scene.js': 'export function pick({ state }) { return { index: 0 }; }',
    });
});

test('a module name an entry script could not import is refused before it is added', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewScriptName('scripts/scene.js');
    controller.createScriptModule();

    expect(controller.getSnapshot().error).toContain('stateDeclarationScriptNameInvalid');
    expect(controller.getSnapshot().draft?.panels?.scripts).toBeUndefined();
});

test('a module with no source yet is dropped on save, like any blank row', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewScriptName('scene.js');
    controller.createScriptModule();
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.scripts).toBeUndefined();
});

test('editing a panel keeps the shared modules', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    controller.setNewScriptName('scene.js');
    controller.createScriptModule();
    controller.setScriptModuleSource('scene.js', 'export default () => ({ index: 0 });');
    controller.updatePanel(0, { title: 'Environment and Weather' });
    controller.addPanelField(0);
    await controller.save();

    expect(world.saves[0]?.declaration.panels?.scripts).toEqual({
        'scene.js': 'export default () => ({ index: 0 });',
    });
    expect(world.saves[0]?.declaration.panels?.panels[0]?.title).toBe('Environment and Weather');
});

test('a combination authored as JSON survives a save', async () => {
    // The editor does not render combinations, so it must leave one alone.
    const world = createWorld({
        scene: {
            fields: [],
            panels: {
                panels: [{
                    title: 'Environment',
                    match: '环境/**',
                    background: {
                        candidates: [{
                            source: '/backgrounds/rainy.png',
                            when: {
                                compose: {
                                    all: [
                                        { source: 'field', field: '环境/时间', op: 'eq', value: '夜晚' },
                                        { source: 'field', field: '环境/天气', op: 'eq', value: '下雨' },
                                    ],
                                },
                            },
                        }],
                    },
                }],
            },
        },
    });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    await controller.save();

    expect(world.saves[0]?.declaration.panels?.panels[0]?.background?.candidates[0]?.when).toEqual({
        compose: {
            all: [
                { source: 'field', field: '环境/时间', op: 'eq', value: '夜晚' },
                { source: 'field', field: '环境/天气', op: 'eq', value: '下雨' },
            ],
        },
    });
});

test('binding the selection writes it, then re-reads where it landed', async () => {
    const world = createWorld({ scene: declarationWithBackground() });
    const toggles: Array<{ scope: string; name: string }> = [];
    let boundName = '';
    const controller = createStateConfigController({
        ...world.deps,
        readBinding: () => Promise.resolve({
            chatAvailable: true,
            entityScope: 'character' as const,
            chatName: boundName,
            entityName: '',
        }),
        toggleBinding: (scope, name) => {
            toggles.push({ scope, name });
            boundName = name;
            return Promise.resolve(true);
        },
    });
    await controller.init();

    expect(controller.getSnapshot().selectedName).toBe('scene');
    expect(controller.getSnapshot().binding?.chatName).toBe('');

    await controller.bindSelectedTo('chat');

    expect(toggles).toEqual([{ scope: 'chat', name: 'scene' }]);
    // The panel shows what the binding call actually did, not what it asked for.
    expect(controller.getSnapshot().binding?.chatName).toBe('scene');
});

test('binding without a selection does nothing, and an unreadable runtime says so', async () => {
    const world = createWorld({});
    let toggles = 0;
    const controller = createStateConfigController({
        ...world.deps,
        toggleBinding: () => {
            toggles += 1;
            return Promise.resolve(true);
        },
    });
    await controller.init();
    await controller.bindSelectedTo('chat');

    expect(toggles).toBe(0);
    // The fixture resolves null: a window that cannot read the binding must not
    // claim it has one.
    expect(controller.getSnapshot().binding).toBeNull();
});
