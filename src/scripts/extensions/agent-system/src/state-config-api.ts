/**
 * The state declaration endpoints, as the editor uses them.
 *
 * The routes are the app's own HTTP surface (`/api/state-declarations/*`), the
 * same one every other settings page uses; the host turns each of them into the
 * matching Tauri command. Nothing here decides anything: the backend validates
 * and refuses, and its message is what the editor shows.
 */

import type { StateDeclaration } from './state-config-model';

const STATE_DECLARATION_ROUTES = Object.freeze({
    list: '/api/state-declarations/list',
    get: '/api/state-declarations/get',
    save: '/api/state-declarations/save',
    delete: '/api/state-declarations/delete',
});

async function postStateRoute<T>(url: string, body: unknown): Promise<T> {
    const response = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body ?? {}),
    });
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(details || response.statusText || `HTTP ${response.status}`);
    }
    return await response.json() as T;
}

export async function listStateDeclarations(): Promise<string[]> {
    const names = await postStateRoute<unknown>(STATE_DECLARATION_ROUTES.list, {});
    return Array.isArray(names) ? names.map((name) => String(name)) : [];
}

export async function getStateDeclaration(name: string): Promise<StateDeclaration> {
    const declaration = await postStateRoute<StateDeclaration>(STATE_DECLARATION_ROUTES.get, { name });
    if (!declaration || typeof declaration !== 'object' || !Array.isArray(declaration.fields)) {
        throw new Error(`state declaration \`${name}\` is not a declaration document`);
    }
    return declaration;
}

export async function saveStateDeclaration(name: string, declaration: StateDeclaration): Promise<void> {
    await postStateRoute<unknown>(STATE_DECLARATION_ROUTES.save, { name, declaration });
}

export async function deleteStateDeclaration(name: string): Promise<void> {
    await postStateRoute<unknown>(STATE_DECLARATION_ROUTES.delete, { name });
}
