import { expect, test } from '@rstest/core';

import type { StateDeclaration } from './state-config-model';
import { createStateConfigController } from './state-config-controller';
import { printStatePackage } from './state-package';

const tr = (key: string, params: Record<string, unknown> = {}): string => [
    key,
    ...Object.entries(params).map(([name, value]) => (
        `${name}=${typeof value === 'string' ? value : JSON.stringify(value)}`
    )),
].join(' ');

/**
 * A machine that has never seen a scene, and the file that brings one over.
 *
 * The two directions go through the editor's own save, so what is worth testing
 * is the wiring: which file is written, what the name inside it means, and that
 * a refusal leaves the store alone.
 */
function createWorld(declarations: Record<string, StateDeclaration> = {}) {
    const stored = new Map(Object.entries(declarations).map(([name, value]) => [name, structuredClone(value)]));
    const downloads: Array<{ fileName: string; text: Promise<string> }> = [];
    const confirmations: string[] = [];
    let confirmAnswer = true;

    return {
        stored,
        downloads,
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
            downloadBlob: (blob: Blob, fileName: string) => {
                downloads.push({ fileName, text: blob.text() });
                return Promise.resolve({ mode: 'browser-download', completed: true });
            },
            pickFilePath: null,
            readBinding: () => Promise.resolve(null),
            toggleBinding: () => Promise.resolve(false),
            tr,
        },
    };
}

function sceneWithBackground(): StateDeclaration {
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

test('a scene leaves as a file and comes back as one', async () => {
    const world = createWorld({ scene: sceneWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();

    await controller.exportDeclaration();

    const download = world.downloads.at(0);
    expect(world.downloads).toHaveLength(1);
    expect(download?.fileName).toBe('scene.scene.json');
    const text = (await download?.text) ?? '';

    // A second editor, on a machine that never saw the scene.
    const other = createWorld();
    const otherController = createStateConfigController(other.deps);
    await otherController.init();
    await otherController.importDeclaration(text);

    expect(other.stored.has('scene')).toBe(true);
    expect(otherController.getSnapshot().selectedName).toBe('scene');
    expect(other.stored.get('scene')?.panels?.panels[0]?.background)
        .toEqual({ candidates: [{ source: '/backgrounds/room.png' }] });
});

test('an unreadable file is refused with a sentence, and nothing is stored', async () => {
    const world = createWorld();
    const controller = createStateConfigController(world.deps);
    await controller.init();

    await controller.importDeclaration('{"fields": []}');

    expect(world.stored.size).toBe(0);
    expect(controller.getSnapshot().error).toBe('stateDeclarationImport_not_a_package');
});

test('a file that could not be stored is refused before anything is written', async () => {
    // The theme is compiled on the way in, so a sheet that fetches a document is
    // refused here — and refused before the user is asked to overwrite anything.
    const world = createWorld({ scene: sceneWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();
    const file = printStatePackage('scene', {
        fields: [{ pattern: '环境/日期', label: 'DATE' }],
        panels: { panels: [], css: '@import url("https://example.com/theme.css");' },
    });

    await controller.importDeclaration(file);

    expect(controller.getSnapshot().error).toContain('stateDeclarationSaveBlocked');
    expect(world.confirmations).toEqual([]);
    expect(world.stored.get('scene')?.panels?.panels[0]?.title).toBe('Environment');
});

test('importing over an existing name asks first', async () => {
    const world = createWorld({ scene: sceneWithBackground() });
    const controller = createStateConfigController(world.deps);
    await controller.init();
    const file = printStatePackage('scene', {
        fields: [{ pattern: '环境/天气', label: 'WEATHER' }],
    });

    world.setConfirmAnswer(false);
    await controller.importDeclaration(file);
    expect(world.stored.get('scene')?.fields.map((field) => field.pattern)).toEqual(['环境/日期']);

    world.setConfirmAnswer(true);
    await controller.importDeclaration(file);
    expect(world.stored.get('scene')?.fields.map((field) => field.pattern)).toEqual(['环境/天气']);
});
