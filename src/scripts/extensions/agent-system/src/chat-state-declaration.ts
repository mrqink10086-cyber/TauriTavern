/**
 * The state declaration bound to the current chat, as the profile editor reads it.
 *
 * The same binding policy the run path follows (`state-declaration-binding-policy.js`):
 * chat header metadata first, then the character/group binding owned by
 * `power-user.js`; a candidate only counts when its name still exists in the
 * saved declaration list. The host invoke is read straight off the bridge —
 * this editor needs the binding chain, not the prompt snapshot layer that the
 * run path wraps it in. Every failure resolves to `null`: an editor that cannot
 * see a binding must not invent one.
 */

import { resolveStateDeclarationBinding } from '../../../state-declaration-binding-policy.js';
import type { DeclaredStateField } from './state-config-model';

export type ChatStateDeclaration = { name: string; fields: DeclaredStateField[] };

type SafeInvoke = (command: string, args?: unknown) => Promise<unknown>;

type DeclarationBindingCandidates = {
    getStateDeclarationBindingCandidates?: () => unknown;
};

type BindingCandidate = { scope: 'chat' | 'character' | 'group'; name: string | undefined };

function getSafeInvoke(): SafeInvoke | null {
    const fn = window.__TAURITAVERN__?.invoke?.safeInvoke;
    return typeof fn === 'function' ? fn : null;
}

export async function resolveChatStateDeclaration(): Promise<ChatStateDeclaration | null> {
    const safeInvoke = getSafeInvoke();
    if (!safeInvoke) {
        return null;
    }

    let candidates: BindingCandidate[] = [];
    try {
        // Load the live page module, not a bundled copy: binding candidates read
        // the runtime's chat state, which a private copy would never see.
        const url = '/scripts/power-user.js';
        const powerUser = await import(/* webpackIgnore: true */ url) as DeclarationBindingCandidates;
        const found = powerUser.getStateDeclarationBindingCandidates?.();
        candidates = Array.isArray(found) ? (found as BindingCandidate[]) : [];
    } catch {
        return null;
    }
    if (candidates.length === 0) {
        return null;
    }

    let names: unknown;
    try {
        names = await safeInvoke('list_state_declarations');
    } catch {
        return null;
    }
    if (!Array.isArray(names)) {
        return null;
    }

    const selectedName = resolveStateDeclarationBinding(candidates, names.map((name) => String(name)));
    if (!selectedName) {
        return null;
    }

    try {
        const declaration = await safeInvoke('get_state_declaration', { name: selectedName }) as
            | { fields?: DeclaredStateField[] }
            | null;
        if (!declaration || !Array.isArray(declaration.fields)) {
            return null;
        }
        return { name: selectedName, fields: declaration.fields };
    } catch {
        return null;
    }
}
