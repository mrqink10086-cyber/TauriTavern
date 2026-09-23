// @ts-check

import { t } from '../../i18n.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { getRequestHeaders } from '../../../script.js';
import { SURFACE, applySurface } from '../../tauritavern/layout-kit.js';

/**
 * The scenes a character card carries, installed when the card is imported.
 *
 * A card that ships a scene is making a claim about how it should look, so the
 * import follows it all the way: the scene is stored, and the character is bound
 * to it. That is deliberately not the skills/profiles flow — those open a dialog
 * because the user may not want the asset; a scene that the card's own author
 * wrote for this character is the card's presentation, and a panel that stays
 * empty until someone finds a settings tab is the thing this path exists to
 * remove.
 *
 * The one question worth asking is asked: an existing scene under the same name
 * is not overwritten silently, because that name may hold the user's own work.
 */

const STATE_PACKAGE_KIND = 'tauritavern.state-declaration';
const STATE_PACKAGE_VERSION = 1;
const EMBEDDED_STATES_VERSION = 1;

const STATE_ROUTES = Object.freeze({
    list: '/api/state-declarations/list',
    get: '/api/state-declarations/get',
    save: '/api/state-declarations/save',
});

function getToastr() {
    return /** @type {any} */ (toastr);
}

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {Record<string, any>}
 */
function requirePlainObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }

    return /** @type {Record<string, any>} */ (value);
}

/**
 * Compare two scenes without caring about key order.
 *
 * The card holds whatever the exporting build wrote; the store holds what the
 * save call normalized. Same document, different order, and the difference must
 * not look like a conflict.
 *
 * @param {unknown} value
 * @returns {string}
 */
function stableStringify(value) {
    if (value === null || typeof value !== 'object') {
        return JSON.stringify(value) ?? 'null';
    }

    if (Array.isArray(value)) {
        return `[${value.map(stableStringify).join(',')}]`;
    }

    const record = /** @type {Record<string, unknown>} */ (value);
    return `{${Object.keys(record).sort()
        .map(key => `${JSON.stringify(key)}:${stableStringify(record[key])}`)
        .join(',')}}`;
}

/**
 * @param {string} url
 * @param {unknown} body
 * @returns {Promise<any>}
 */
async function postStateRoute(url, body) {
    const response = await fetch(url, {
        method: 'POST',
        headers: getRequestHeaders(),
        body: JSON.stringify(body ?? {}),
    });
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(details || response.statusText || `HTTP ${response.status}`);
    }

    return await response.json();
}

/**
 * @returns {Promise<string[]>}
 */
async function listStateDeclarationNames() {
    const names = await postStateRoute(STATE_ROUTES.list, {});
    return Array.isArray(names) ? names.map(name => String(name)) : [];
}

/**
 * @param {string} name
 * @returns {Promise<any>}
 */
async function loadStateDeclaration(name) {
    return await postStateRoute(STATE_ROUTES.get, { name });
}

/**
 * @param {string} name
 * @param {unknown} declaration
 */
async function saveStateDeclaration(name, declaration) {
    await postStateRoute(STATE_ROUTES.save, { name, declaration });
}

/**
 * @param {unknown} item
 * @param {number} index
 * @returns {{ name: string; declaration: Record<string, any> }}
 */
function sceneFromItem(item, index) {
    const label = `stateDeclarations.items[${index}]`;
    const record = requirePlainObject(item, label);
    const scene = requirePlainObject(record.scene, `${label}.scene`);

    if (scene.kind !== STATE_PACKAGE_KIND) {
        throw new Error(`${label}.scene is not a scene file`);
    }

    const version = Number(scene.version);
    if (!Number.isInteger(version) || version < 1 || version > STATE_PACKAGE_VERSION) {
        throw new Error(`${label}.scene has an unsupported version: ${scene.version}`);
    }

    const declaration = requirePlainObject(scene.declaration, `${label}.scene.declaration`);
    if (!Array.isArray(declaration.fields)) {
        throw new Error(`${label}.scene.declaration has no field list`);
    }

    const name = String(scene.name || '').trim();
    if (!name) {
        throw new Error(`${label}.scene has no name to store it under`);
    }

    return { name, declaration };
}

/**
 * The scenes embedded in one character payload.
 *
 * Both carriers are checked, the same way the profile and Skill readers do it:
 * a card written by an older build puts `tauritavern` under `data.extensions`,
 * and one written by hand may put it at the top.
 *
 * @param {any} character
 * @returns {{ name: string; declaration: Record<string, any> }[]}
 */
export function extractCharacterEmbeddedScenes(character) {
    const embedded = character?.data?.extensions?.tauritavern?.stateDeclarations
        ?? character?.extensions?.tauritavern?.stateDeclarations;
    if (embedded === null || embedded === undefined) {
        return [];
    }

    const payload = requirePlainObject(embedded, 'stateDeclarations');
    if (Number(payload.version) !== EMBEDDED_STATES_VERSION) {
        throw new Error(`Unsupported embedded scene schema version: ${payload.version}`);
    }
    if (!Array.isArray(payload.items)) {
        throw new Error('Embedded scene items must be an array');
    }

    return payload.items.map((item, index) => sceneFromItem(item, index));
}

/**
 * Ask before replacing a scene that is already stored under this name.
 *
 * @param {string} name
 * @returns {Promise<boolean>} whether to replace the stored scene
 */
async function confirmSceneReplace(name) {
    const content = document.createElement('div');
    content.classList.add('tt-scene-import-confirm');
    const lead = document.createElement('p');
    lead.textContent = t`A scene named "${name}" already exists and differs from the one this card carries.`;
    const detail = document.createElement('p');
    detail.textContent = t`Replacing it overwrites the stored scene. Keeping it leaves that scene in place.`;
    content.append(lead, detail);

    const popup = new Popup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: t`Replace`,
        cancelButton: t`Keep existing`,
        wide: false,
    });
    popup.dlg.classList.add('tt-scene-import-confirm-popup');
    applySurface(popup.dlg, SURFACE.FullscreenWindow);

    return await popup.show() === POPUP_RESULT.AFFIRMATIVE;
}

/**
 * Whether a scene brings code that runs in the page.
 *
 * A scene's shared scripts have always travelled with it, but those run in the
 * sandbox and only ever answer with data. An event handler does not: it runs
 * with the panel's own element as `this`, inside the page, which is a thing to
 * agree to before a card installs it rather than after.
 *
 * @param {any} declaration
 * @returns {boolean}
 */
function declarationHasHandler(declaration) {
    const panels = declaration?.panels?.panels;
    if (!Array.isArray(panels)) {
        return false;
    }
    return panels.some((panel) => nodesHaveHandler(panel?.markup?.nodes));
}

/**
 * @param {any[]} nodes
 * @returns {boolean}
 */
function nodesHaveHandler(nodes) {
    if (!Array.isArray(nodes)) {
        return false;
    }
    return nodes.some((node) => {
        if (!node || typeof node !== 'object') {
            return false;
        }
        if (node.kind === 'element') {
            const names = [...Object.keys(node.attrs ?? {}), ...Object.keys(node.boundAttrs ?? {})];
            if (names.some((name) => /^on[a-z]+$/u.test(String(name)))) {
                return true;
            }
            return nodesHaveHandler(node.children);
        }
        if (node.kind === 'if') {
            return nodesHaveHandler(node.then) || nodesHaveHandler(node.else);
        }
        if (node.kind === 'each') {
            return nodesHaveHandler(node.body);
        }
        return false;
    });
}

/**
 * @param {string} sourceLabel
 * @returns {Promise<boolean>} whether to install anyway
 */
async function confirmSceneHandlers(sourceLabel) {
    const content = document.createElement('div');
    content.classList.add('tt-scene-import-confirm');
    const lead = document.createElement('p');
    lead.textContent = t`Scenes from ${sourceLabel} carry event handlers.`;
    const detail = document.createElement('p');
    detail.textContent = t`A handler runs inside this page, with the panel's own element as its target. A shared script does not — it only returns data. Install these only if you trust where this card came from.`;
    content.append(lead, detail);

    const popup = new Popup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: t`Install anyway`,
        cancelButton: t`Do not install`,
        wide: false,
    });
    popup.dlg.classList.add('tt-scene-import-confirm-popup');
    applySurface(popup.dlg, SURFACE.FullscreenWindow);

    return await popup.show() === POPUP_RESULT.AFFIRMATIVE;
}

/**
 * Store one scene, unless the store already holds the very same one.
 *
 * @param {{ name: string; declaration: Record<string, any> }} scene
 * @param {Set<string>} storedNames
 * @returns {Promise<'installed' | 'unchanged' | 'kept'>}
 */
async function installScene(scene, storedNames) {
    if (!storedNames.has(scene.name)) {
        await saveStateDeclaration(scene.name, scene.declaration);
        storedNames.add(scene.name);
        return 'installed';
    }

    const stored = await loadStateDeclaration(scene.name);
    if (stableStringify(stored) === stableStringify(scene.declaration)) {
        return 'unchanged';
    }

    if (!await confirmSceneReplace(scene.name)) {
        return 'kept';
    }

    await saveStateDeclaration(scene.name, scene.declaration);
    return 'installed';
}

/**
 * Bind the imported character to the scene the card leads with.
 *
 * The page module is loaded at runtime rather than bundled: a private copy would
 * write its own settings object, not the one the rest of the app reads.
 *
 * @param {string} avatarFileName
 * @param {string} sceneName
 * @returns {Promise<boolean>}
 */
async function bindSceneToCharacter(avatarFileName, sceneName) {
    try {
        const url = '/scripts/power-user.js';
        const api = await import(/* webpackIgnore: true */ url);
        if (typeof api?.bindStateDeclarationToCharacter !== 'function') {
            return false;
        }

        return await api.bindStateDeclarationToCharacter(avatarFileName, sceneName) === true;
    } catch (error) {
        console.error('Binding the imported scene to its character failed', error);
        return false;
    }
}

/**
 * @param {{ avatarFileName: string; label: string; character: any; scenes: any[] }} options
 */
async function installScenes({ avatarFileName, label, character, scenes }) {
    const sourceLabel = String(label || character?.name || character?.data?.name || t`Imported character`);

    // Asked once for the whole card, before anything is stored: a refused import
    // should leave the store exactly as it was.
    if (scenes.some((scene) => declarationHasHandler(scene?.declaration))) {
        if (!await confirmSceneHandlers(sourceLabel)) {
            return;
        }
    }

    const storedNames = new Set(await listStateDeclarationNames());
    const installed = [];
    let keptAny = false;

    for (const scene of scenes) {
        const outcome = await installScene(scene, storedNames);
        if (outcome === 'installed') {
            installed.push(scene.name);
        }
        if (outcome === 'kept') {
            keptAny = true;
        }
    }

    // The card's own order is the author's order: the first scene is the one the
    // character is meant to open with.
    const boundName = scenes[0].name;
    const bound = await bindSceneToCharacter(avatarFileName, boundName);

    if (installed.length > 0) {
        getToastr().success(
            bound
                ? t`Installed ${installed.length} scene(s) from ${sourceLabel} and bound "${boundName}" to this character`
                : t`Installed ${installed.length} scene(s) from ${sourceLabel}; bind one to see it`,
            t`Scene installed`,
        );
    } else {
        getToastr().info(
            bound
                ? t`${sourceLabel} already carried these scenes; "${boundName}" is bound to this character`
                : t`${sourceLabel} already carried these scenes`,
            t`Scene installed`,
        );
    }

    if (keptAny) {
        getToastr().warning(t`One or more scenes were left as they were.`, t`Scene installed`);
    }
}

/**
 * @param {unknown} error
 */
function reportSceneError(error) {
    console.error('Agent scene embedded import failed for character', error);
    const message = error instanceof Error ? error.message : String(error || t`Unknown error`);
    getToastr().error(message, t`Scene import failed`);
}

/**
 * Install every scene an imported character carries, and bind the first one.
 *
 * Never throws: the import itself already succeeded, and a card that carries a
 * broken scene must not turn that into a failed import.
 *
 * @param {{ avatarFileName: string; label: string; loadCharacter: () => Promise<any> }} options
 */
export async function maybeInstallCharacterEmbeddedScenes({ avatarFileName, label, loadCharacter }) {
    try {
        if (typeof loadCharacter !== 'function') {
            throw new Error('loadCharacter is required');
        }

        const character = await loadCharacter();
        const scenes = extractCharacterEmbeddedScenes(character);
        if (scenes.length === 0) {
            return;
        }

        await installScenes({ avatarFileName, label, character, scenes });
    } catch (error) {
        reportSceneError(error);
    }
}
