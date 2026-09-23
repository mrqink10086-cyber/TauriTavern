// @ts-check

import { t } from '../../i18n.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { getRequestHeaders } from '../../../script.js';
import { SURFACE, applySurface } from '../../tauritavern/layout-kit.js';

/**
 * The standalone state machines and conditional sets a card carries.
 *
 * These two are the same thing twice over — a named document, one item per name,
 * a binding per character — so they share one installer and differ only in the
 * table below. They stay separate from the scene carrier next door, whose item
 * is a whole exported file rather than a name and a document.
 *
 * A card that ships one is making a claim about how its story moves, so the
 * import follows it all the way: the document is stored, and the character is
 * bound to it. A namesake the user already wrote is never overwritten silently.
 */

const EMBEDDED_VERSION = 1;

/**
 * @param {string} base
 * @returns {{ list: string; get: string; save: string }}
 */
function routes(base) {
    return Object.freeze({
        list: `${base}/list`,
        get: `${base}/get`,
        save: `${base}/save`,
    });
}

/**
 * What differs between the two kinds, as one table.
 *
 * `describe` is the shape check a carried document has to pass before it is
 * stored: the store validates the semantics, but a card's own payload is read
 * here first so a broken one is reported against the item it came from.
 */
const KINDS = Object.freeze({
    machines: {
        carrierKey: 'stateMachines',
        itemField: 'machine',
        routes: routes('/api/state-machines'),
        bindExport: 'bindStateMachineToCharacter',
        noun: () => t`state machine`,
        plural: () => t`state machines`,
        /**
         * @param {Record<string, any>} document
         * @param {string} label
         */
        describe: (document, label) => {
            if (!Array.isArray(document.states) || !Array.isArray(document.transitions)) {
                throw new Error(`${label} has no stage list`);
            }
        },
    },
    predicates: {
        carrierKey: 'statePredicates',
        itemField: 'set',
        routes: routes('/api/state-predicates'),
        bindExport: 'bindStatePredicateToCharacter',
        noun: () => t`conditional set`,
        plural: () => t`conditional sets`,
        /**
         * @param {Record<string, any>} document
         * @param {string} label
         */
        describe: (document, label) => {
            if (!Array.isArray(document.groups) && !Array.isArray(document.constants)) {
                throw new Error(`${label} is not a conditional set`);
            }
        },
    },
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
 * Compare two documents without caring about key order.
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
async function postRoute(url, body) {
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
 * The documents of one kind embedded in a character payload.
 *
 * Both carriers are checked, the same way the profile and Skill readers do it: a
 * card written by an older build puts `tauritavern` under `data.extensions`, and
 * one written by hand may put it at the top.
 *
 * @param {any} character
 * @param {typeof KINDS[keyof typeof KINDS]} kind
 * @returns {{ name: string; document: Record<string, any> }[]}
 */
export function extractCharacterEmbeddedNamedAssets(character, kind) {
    const embedded = character?.data?.extensions?.tauritavern?.[kind.carrierKey]
        ?? character?.extensions?.tauritavern?.[kind.carrierKey];
    if (embedded === null || embedded === undefined) {
        return [];
    }

    const payload = requirePlainObject(embedded, kind.carrierKey);
    if (Number(payload.version) !== EMBEDDED_VERSION) {
        throw new Error(`Unsupported embedded ${kind.carrierKey} schema version: ${payload.version}`);
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(`Embedded ${kind.carrierKey} items must be an array`);
    }

    return payload.items.map((item, index) => {
        const label = `${kind.carrierKey}.items[${index}]`;
        const record = requirePlainObject(item, label);
        const name = String(record.name || '').trim();
        if (!name) {
            throw new Error(`${label} has no name to store it under`);
        }

        const document = requirePlainObject(record[kind.itemField], `${label}.${kind.itemField}`);
        kind.describe(document, `${label}.${kind.itemField}`);

        return { name, document };
    });
}

/**
 * Ask before replacing a document that is already stored under this name.
 *
 * @param {typeof KINDS[keyof typeof KINDS]} kind
 * @param {string} name
 * @returns {Promise<boolean>} whether to replace the stored document
 */
async function confirmReplace(kind, name) {
    const content = document.createElement('div');
    content.classList.add('tt-state-asset-import-confirm');
    const lead = document.createElement('p');
    lead.textContent = t`A ${kind.noun()} named "${name}" already exists and differs from the one this card carries.`;
    const detail = document.createElement('p');
    detail.textContent = t`Replacing it overwrites the stored ${kind.noun()}. Keeping it leaves that one in place.`;
    content.append(lead, detail);

    const popup = new Popup(content, POPUP_TYPE.CONFIRM, '', {
        okButton: t`Replace`,
        cancelButton: t`Keep existing`,
        wide: false,
    });
    popup.dlg.classList.add('tt-state-asset-import-confirm-popup');
    applySurface(popup.dlg, SURFACE.FullscreenWindow);

    return await popup.show() === POPUP_RESULT.AFFIRMATIVE;
}

/**
 * Bind the imported character to the document the card leads with.
 *
 * The page module is loaded at runtime rather than bundled: a private copy would
 * write its own settings object, not the one the rest of the app reads.
 *
 * @param {typeof KINDS[keyof typeof KINDS]} kind
 * @param {string} avatarFileName
 * @param {string} name
 * @returns {Promise<boolean>}
 */
async function bindToCharacter(kind, avatarFileName, name) {
    try {
        const url = '/scripts/power-user.js';
        const api = await import(/* webpackIgnore: true */ url);
        const bind = api?.[kind.bindExport];
        if (typeof bind !== 'function') {
            return false;
        }

        return await bind(avatarFileName, name) === true;
    } catch (error) {
        console.error(`Binding the imported ${kind.carrierKey} to its character failed`, error);
        return false;
    }
}

/**
 * @param {typeof KINDS[keyof typeof KINDS]} kind
 * @param {{ avatarFileName: string; label: string; character: any; items: any[] }} options
 */
async function installKind(kind, { avatarFileName, label, character, items }) {
    const sourceLabel = String(label || character?.name || character?.data?.name || t`Imported character`);
    const storedNames = new Set(
        (await postRoute(kind.routes.list, {}))
            .map((/** @type {unknown} */ name) => String(name)),
    );
    const installed = [];
    let keptAny = false;

    for (const item of items) {
        if (!storedNames.has(item.name)) {
            await postRoute(kind.routes.save, { name: item.name, [kind.itemField]: item.document });
            storedNames.add(item.name);
            installed.push(item.name);
            continue;
        }

        const stored = await postRoute(kind.routes.get, { name: item.name });
        if (stableStringify(stored) === stableStringify(item.document)) {
            continue;
        }
        if (!await confirmReplace(kind, item.name)) {
            keptAny = true;
            continue;
        }

        await postRoute(kind.routes.save, { name: item.name, [kind.itemField]: item.document });
        installed.push(item.name);
    }

    // The card's own order is the author's order: the first one is the document
    // the character is meant to open with.
    const boundName = items[0].name;
    const bound = await bindToCharacter(kind, avatarFileName, boundName);

    if (installed.length > 0) {
        getToastr().success(
            bound
                ? t`Installed ${installed.length} ${kind.plural()} from ${sourceLabel} and bound "${boundName}" to this character`
                : t`Installed ${installed.length} ${kind.plural()} from ${sourceLabel}; bind one to see it`,
            t`State asset installed`,
        );
    } else {
        getToastr().info(
            bound
                ? t`${sourceLabel} already carried these ${kind.plural()}; "${boundName}" is bound to this character`
                : t`${sourceLabel} already carried these ${kind.plural()}`,
            t`State asset installed`,
        );
    }

    if (keptAny) {
        getToastr().warning(
            t`One or more ${kind.plural()} were left as they were.`,
            t`State asset installed`,
        );
    }
}

/**
 * Install everything a card carries of both kinds, binding the first of each.
 *
 * Never throws: the import itself already succeeded, and a card that carries a
 * broken document must not turn that into a failed import.
 *
 * @param {{ avatarFileName: string; label: string; loadCharacter: () => Promise<any> }} options
 */
export async function maybeInstallCharacterEmbeddedNamedAssets({ avatarFileName, label, loadCharacter }) {
    try {
        if (typeof loadCharacter !== 'function') {
            throw new Error('loadCharacter is required');
        }

        const character = await loadCharacter();
        for (const kind of Object.values(KINDS)) {
            const items = extractCharacterEmbeddedNamedAssets(character, kind);
            if (items.length > 0) {
                await installKind(kind, { avatarFileName, label, character, items });
            }
        }
    } catch (error) {
        console.error('Agent state asset embedded import failed for character', error);
        const message = error instanceof Error ? error.message : String(error || t`Unknown error`);
        getToastr().error(message, t`State asset import failed`);
    }
}
