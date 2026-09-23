/**
 * The scene the default character card ships with.
 *
 * Default content is installed as a plain file copy, so a card cannot bring a
 * scene into a fresh install by itself, and the shipped PNG carries no
 * `tauritavern` extension at all. What replaces that is one startup pass that
 * does what importing such a card would have done: store the scene, write it
 * into the card, and bind the character to it.
 *
 * The scene is generated from `seraphinaDeclaration()` rather than baked into
 * the card, so the example the editor offers and the one the card carries cannot
 * drift apart — and a card exported from here carries the same scene onward.
 *
 * The pass is versioned, and the mark is written only once the work is done: a
 * user who deletes the scene or unbinds it keeps it that way, while a card that
 * is not installed yet (a user can delete it) is retried on the next start.
 */

import { readEmbeddedStatePackage, portableEmbeddedState } from './embedded-asset-packages';
import { upsertScene } from './embedded-scene-assets';
import { requireSillyTavernContext } from './host-api';
import { translateAgentSystem as tr } from './i18n';
import { loadSettings, patchSettings, type AgentSystemSettings } from './settings-store';
import { getStateDeclaration, listStateDeclarations, saveStateDeclaration } from './state-config-api';
import type { StateDeclaration } from './state-config-model';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';
import { SERAPHINA_SCENE_NAME, seraphinaDeclaration } from './state-examples-seraphina';
import { bindStateDeclarationToCharacter, readStateDeclarationBindingForAvatar } from './state-binding';

export const DEFAULT_SCENE_AVATAR = 'default_Seraphina.png';
export const DEFAULT_SCENE_SEED_VERSION = 2;

const SEED_ROUTES = Object.freeze({
    getCharacter: '/api/characters/get',
    mergeAttributes: '/api/characters/merge-attributes',
});

export type DefaultSceneSeedOutcome =
    /** The pass has already run; nothing was asked of the backend. */
    | 'already-seeded'
    /** The default card is not installed, so there is nothing to attach to. */
    | 'card-missing'
    /** The name is taken by a declaration the user wrote; everything was left alone. */
    | 'kept-user-scene'
    | 'seeded';

export type DefaultSceneSeedDeps = {
    loadSettings: () => Promise<AgentSystemSettings>;
    markSeeded: () => Promise<void>;
    listDeclarations: () => Promise<string[]>;
    getDeclaration: (name: string) => Promise<StateDeclaration>;
    saveDeclaration: (name: string, declaration: StateDeclaration) => Promise<void>;
    /** The card as the backend holds it, or `null` when it is not installed. */
    loadCard: (avatar: string) => Promise<unknown>;
    writeCardScenes: (avatar: string, scenes: unknown) => Promise<void>;
    readCardBinding: (avatar: string) => Promise<string>;
    bindToCharacter: (avatar: string, name: string) => Promise<boolean>;
};

/**
 * Key order is not part of a document, so it is not part of "the same one".
 *
 * Both sides are compared here after different journeys — one through the
 * editor's save, one through the backend's store — and a difference in key order
 * must not read as a difference in content.
 */
function stableStringify(value: unknown): string {
    if (value === null || typeof value !== 'object') {
        return JSON.stringify(value) ?? 'null';
    }
    if (Array.isArray(value)) {
        return `[${value.map(stableStringify).join(',')}]`;
    }
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort()
        .map((key) => `${JSON.stringify(key)}:${stableStringify(record[key])}`)
        .join(',')}}`;
}

/** The scenes a card carries, from the extension block a card keeps its own payload in. */
function cardScenes(card: unknown): unknown {
    const record = card as { data?: { extensions?: { tauritavern?: { stateDeclarations?: unknown } } } } | null;
    return record?.data?.extensions?.tauritavern?.stateDeclarations;
}

export async function seedDefaultSeraphinaScene(deps: DefaultSceneSeedDeps): Promise<DefaultSceneSeedOutcome> {
    const settings = await deps.loadSettings();
    if (Number(settings?.defaultSceneSeedVersion || 0) >= DEFAULT_SCENE_SEED_VERSION) {
        return 'already-seeded';
    }

    const card = await deps.loadCard(DEFAULT_SCENE_AVATAR);
    if (!card) {
        return 'card-missing';
    }

    // Normalized on the way in, exactly as the editor's own save normalizes: the
    // store is what the backend validates, and it refuses a draft's raw source
    // fields by name.
    const document = normalizeStateDeclarationForSave(seraphinaDeclaration());

    if ((await deps.listDeclarations()).includes(SERAPHINA_SCENE_NAME)) {
        const stored = await deps.getDeclaration(SERAPHINA_SCENE_NAME);
        if (stableStringify(stored) !== stableStringify(document)) {
            // This name is the user's now. The pass still counts as done, or it
            // would ask the same question on every start.
            console.warn(
                `[AgentSystem] "${SERAPHINA_SCENE_NAME}" already holds a different declaration;`
                + ' the shipped example was left out.',
            );
            await deps.markSeeded();
            return 'kept-user-scene';
        }
    } else {
        await deps.saveDeclaration(SERAPHINA_SCENE_NAME, document);
    }

    await deps.writeCardScenes(DEFAULT_SCENE_AVATAR, upsertScene(
        readEmbeddedStatePackage(cardScenes(card)),
        portableEmbeddedState(SERAPHINA_SCENE_NAME, document),
    ));

    // A binding the user set is not this pass's to replace — the same rule the
    // import path follows for a chat it did not open.
    if (!String(await deps.readCardBinding(DEFAULT_SCENE_AVATAR) || '').trim()) {
        await deps.bindToCharacter(DEFAULT_SCENE_AVATAR, SERAPHINA_SCENE_NAME);
    }

    await deps.markSeeded();
    return 'seeded';
}

type SeedHostContext = {
    getRequestHeaders: () => HeadersInit;
    characters?: Array<{ avatar?: unknown }>;
};

function hostContext(): SeedHostContext {
    return requireSillyTavernContext() as SeedHostContext;
}

async function postJson(url: string, body: unknown): Promise<Response> {
    return await fetch(url, {
        method: 'POST',
        headers: hostContext().getRequestHeaders(),
        body: JSON.stringify(body ?? {}),
    });
}

async function requireOk(response: Response, url: string): Promise<Response> {
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(`${url}: ${details || response.statusText || `HTTP ${response.status}`}`);
    }
    return response;
}

async function loadDefaultCard(avatar: string): Promise<unknown> {
    const response = await postJson(SEED_ROUTES.getCharacter, { avatar_url: avatar });
    if (response.status === 404) {
        return null;
    }
    return await (await requireOk(response, SEED_ROUTES.getCharacter)).json();
}

async function writeCardScenes(avatar: string, stateDeclarations: unknown): Promise<void> {
    await requireOk(await postJson(SEED_ROUTES.mergeAttributes, {
        avatar,
        data: { extensions: { tauritavern: { stateDeclarations } } },
    }), SEED_ROUTES.mergeAttributes);
}

/**
 * Put the rewritten card back in the page's own list.
 *
 * The asset panel reads a card's extension off that list, and it would show the
 * card as it was before this pass. A list that has not been read yet needs
 * nothing: whatever reads it next reads the file that is already updated.
 */
async function refreshCardInMemory(avatar: string): Promise<void> {
    const characters = hostContext().characters;
    if (!Array.isArray(characters) || !characters.some((entry) => entry?.avatar === avatar)) {
        return;
    }
    const url = '/script.js';
    const script = await import(/* webpackIgnore: true */ url) as {
        getOneCharacter?: (avatar: string) => Promise<unknown>;
    };
    await script.getOneCharacter?.(avatar);
}

/**
 * Run the pass against the real backend, then redraw the card if it is on screen.
 *
 * Never throws and never reports to the user: this is background content work at
 * startup, and a failure here must not look like a broken app. Not marking the
 * pass on failure is what makes the next start try again.
 */
export async function runDefaultSceneSeed(): Promise<void> {
    try {
        const settings = await loadSettings();
        const outcome = await seedDefaultSeraphinaScene({
            loadSettings: () => Promise.resolve(settings),
            markSeeded: async () => {
                await patchSettings(settings, { defaultSceneSeedVersion: DEFAULT_SCENE_SEED_VERSION });
            },
            listDeclarations: listStateDeclarations,
            getDeclaration: getStateDeclaration,
            saveDeclaration: saveStateDeclaration,
            loadCard: loadDefaultCard,
            writeCardScenes,
            readCardBinding: readStateDeclarationBindingForAvatar,
            bindToCharacter: bindStateDeclarationToCharacter,
        });
        if (outcome === 'seeded') {
            await refreshCardInMemory(DEFAULT_SCENE_AVATAR);
        }
    } catch (error) {
        console.warn(tr('defaultSceneSeedFailed'), error);
    }
}
