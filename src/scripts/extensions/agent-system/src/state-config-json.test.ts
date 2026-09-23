import { expect, test } from '@rstest/core';

import { createStateConfigController } from './state-config-controller';
import type { StateDeclaration } from './state-config-model';

function tr(key: string, params: Record<string, unknown> = {}): string {
    return [key, ...Object.entries(params).map(([name, value]) => `${name}=${JSON.stringify(value) ?? ''}`)].join(' ');
}

/**
 * A controller over one stored scene, which is what the JSON box edits.
 *
 * The box is the only way to write a document of any size — three hundred rows
 * are not typed one at a time — so these tests are about the round trip and
 * about a broken paste leaving the draft alone.
 */
function createWorld(scene: StateDeclaration = { fields: [{ pattern: '环境/日期', label: 'DATE' }] }) {
    const stored = new Map<string, StateDeclaration>([['scene', structuredClone(scene)]]);
    return {
        stored,
        deps: {
            listDeclarations: () => Promise.resolve([...stored.keys()]),
            getDeclaration: (name: string) => {
                const found = stored.get(name);
                return found
                    ? Promise.resolve(structuredClone(found))
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
            confirmAction: () => Promise.resolve(true),
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

/** The box's text, read back as the document it claims to be. */
function printedDocument(controller: ReturnType<typeof createStateConfigController>): StateDeclaration {
    return JSON.parse(controller.getSnapshot().draftJson) as StateDeclaration;
}

test('the JSON box prints the draft, and a pasted document replaces it', async () => {
    const world = createWorld();
    const controller = createStateConfigController(world.deps);
    await controller.init();

    expect(printedDocument(controller).fields).toEqual([
        { pattern: '环境/日期', label: 'DATE' },
    ]);

    // An edit in the table re-prints the box, so a stale paste cannot be applied
    // over a change the user just made. A row that is still blank is not part of
    // the document, so the box stays at one field until the row is named.
    controller.addField();
    expect(printedDocument(controller).fields).toHaveLength(1);

    controller.updateField(1, { pattern: '环境/地点', label: 'PLACE' });
    expect(printedDocument(controller).fields).toHaveLength(2);

    controller.setDraftJson(JSON.stringify({ fields: [{ pattern: '线索', label: 'CLUE' }] }));
    controller.applyDraftJson();

    expect(controller.getSnapshot().jsonError).toBe('');
    expect(controller.getSnapshot().draft?.fields).toEqual([{ pattern: '线索', label: 'CLUE' }]);
});

test('a paste that is not a document is reported without touching the draft', async () => {
    const world = createWorld();
    const controller = createStateConfigController(world.deps);
    await controller.init();
    const before = controller.getSnapshot().draft;

    controller.setDraftJson('{ not json');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).not.toBe('');
    expect(controller.getSnapshot().draft).toBe(before);

    controller.setDraftJson('[1, 2]');
    controller.applyDraftJson();
    expect(controller.getSnapshot().jsonError).toContain('jsonDocumentMustBeObject');
    expect(controller.getSnapshot().draft).toBe(before);
});
