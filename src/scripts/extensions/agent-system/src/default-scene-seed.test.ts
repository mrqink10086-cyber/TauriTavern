import { expect, test } from '@rstest/core';

import type { AgentSystemSettings } from './settings-store';
import { DEFAULT_SCENE_AVATAR, DEFAULT_SCENE_SEED_VERSION, seedDefaultSeraphinaScene } from './default-scene-seed';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';
import type { StateDeclaration } from './state-config-model';
import { toStatePackage, type StatePackage } from './state-package';
import { SERAPHINA_SCENE_NAME, seraphinaDeclaration } from './state-examples-seraphina';

type EmbeddedSceneItem = { scene: StatePackage };

/** The document the pass stores, built the way the pass builds it. */
function shippedDocument(): StateDeclaration {
    return normalizeStateDeclarationForSave(seraphinaDeclaration());
}

function cardCarrying(name: string, declaration: StateDeclaration): unknown {
    return {
        data: {
            extensions: {
                tauritavern: {
                    stateDeclarations: { version: 1, items: [{ scene: toStatePackage(name, declaration) }] },
                },
            },
        },
    };
}

function createWorld(options: {
    seedVersion?: number;
    card?: unknown;
    stored?: StateDeclaration;
    bound?: string;
} = {}) {
    const calls: string[] = [];
    const written: unknown[] = [];
    const saved: StateDeclaration[] = [];
    const bound: string[] = [];
    const settings = { defaultSceneSeedVersion: options.seedVersion ?? 0 } as AgentSystemSettings;

    return {
        calls,
        written,
        saved,
        bound,
        settings,
        deps: {
            loadSettings: () => Promise.resolve(settings),
            markSeeded: () => {
                calls.push('markSeeded');
                settings.defaultSceneSeedVersion = DEFAULT_SCENE_SEED_VERSION;
                return Promise.resolve();
            },
            listDeclarations: () => Promise.resolve(
                options.stored ? [SERAPHINA_SCENE_NAME] : [],
            ),
            getDeclaration: () => {
                if (!options.stored) {
                    return Promise.reject(new Error(`nothing stored under ${SERAPHINA_SCENE_NAME}`));
                }
                return Promise.resolve(structuredClone(options.stored));
            },
            saveDeclaration: (_name: string, declaration: StateDeclaration) => {
                calls.push('saveDeclaration');
                saved.push(structuredClone(declaration));
                return Promise.resolve();
            },
            loadCard: () => Promise.resolve(options.card === undefined ? {} : options.card),
            writeCardScenes: (_avatar: string, scenes: unknown) => {
                calls.push('writeCardScenes');
                written.push(scenes);
                return Promise.resolve();
            },
            readCardBinding: () => Promise.resolve(options.bound ?? ''),
            bindToCharacter: (_avatar: string, name: string) => {
                calls.push('bindToCharacter');
                bound.push(name);
                return Promise.resolve(true);
            },
        },
    };
}

test('a pass that already ran touches nothing', async () => {
    const world = createWorld({ seedVersion: DEFAULT_SCENE_SEED_VERSION, card: null });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('already-seeded');
    expect(world.calls).toEqual([]);
});

test('a missing default card is left for the next start', async () => {
    const world = createWorld({ card: null });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('card-missing');
    // The mark is what would have made the retry impossible.
    expect(world.calls).toEqual([]);
    expect(world.settings.defaultSceneSeedVersion).toBe(0);
});

test('the shipped scene is stored, written into the card and bound', async () => {
    const world = createWorld();
    const document = shippedDocument();

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('seeded');
    expect(world.calls).toEqual(['saveDeclaration', 'writeCardScenes', 'bindToCharacter', 'markSeeded']);
    expect(world.saved[0]).toEqual(document);
    expect(world.bound).toEqual([SERAPHINA_SCENE_NAME]);

    const items = (world.written[0] as { items: EmbeddedSceneItem[] }).items;
    expect(items.map((item) => item.scene.name)).toEqual([SERAPHINA_SCENE_NAME]);
    expect(items[0]?.scene.declaration.fields?.length).toBeGreaterThan(0);
});

test('a card that already carries a scene keeps it alongside the shipped one', async () => {
    const world = createWorld({
        card: cardCarrying('their-scene', { fields: [{ pattern: '线索', label: 'CLUE' }] }),
    });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('seeded');
    const items = (world.written[0] as { items: EmbeddedSceneItem[] }).items;
    expect(items.map((item) => item.scene.name)).toEqual(['their-scene', SERAPHINA_SCENE_NAME]);
});

test('a stored scene that differs only in key order is not stored twice', async () => {
    const reordered = Object.fromEntries(
        Object.entries(shippedDocument()).reverse(),
    ) as StateDeclaration;
    const world = createWorld({ stored: reordered });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('seeded');
    expect(world.calls).toEqual(['writeCardScenes', 'bindToCharacter', 'markSeeded']);
});

test('a namesake the user wrote is left alone, and the pass does not come back', async () => {
    const world = createWorld({
        stored: { fields: [{ pattern: '我的/字段', label: 'MINE' }] },
    });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('kept-user-scene');
    expect(world.calls).toEqual(['markSeeded']);
    expect(world.written).toEqual([]);
    expect(world.bound).toEqual([]);
});

test('a binding the user set is not replaced', async () => {
    const world = createWorld({ bound: 'their-scene' });

    expect(await seedDefaultSeraphinaScene(world.deps)).toBe('seeded');
    expect(world.calls).toEqual(['saveDeclaration', 'writeCardScenes', 'markSeeded']);
    expect(world.bound).toEqual([]);
});

test('the pass only ever targets the shipped default card', async () => {
    expect(DEFAULT_SCENE_AVATAR).toBe('default_Seraphina.png');
    const requested: string[] = [];
    const world = createWorld();
    await seedDefaultSeraphinaScene({
        ...world.deps,
        loadCard: (avatar: string) => {
            requested.push(avatar);
            return Promise.resolve({});
        },
    });

    expect(requested).toEqual([DEFAULT_SCENE_AVATAR]);
});
