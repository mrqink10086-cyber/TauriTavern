/**
 * The state binding a settings panel reads and writes.
 *
 * A binding is not a backend fact: it lives in the page's own runtime — chat
 * header metadata for chat bindings, `power_user` for character/group bindings —
 * and only `power-user.js` knows both containers and which save call each one
 * needs. This module is the panel's side of that line: it loads the live page
 * module and hands back a plain view, or performs the toggle.
 *
 * The page module is loaded from the page origin at runtime rather than bundled:
 * a private copy would read its own state, not the chat the user has open. When
 * that load fails, the panel is told so (`null`) instead of being shown a binding
 * it cannot actually see — an editor that cannot see a binding must not offer to
 * change one.
 */

export type StateBindingKind = 'declaration' | 'machine' | 'predicates';

/** The two things a binding can attach to. */
export type StateBindingScope = 'chat' | 'entity';

/** What a panel needs to label its binding buttons and its current state. */
export type StateBindingView = {
    chatAvailable: boolean;
    entityScope: 'character' | 'group' | null;
    chatName: string;
    entityName: string;
};

type PowerUserBindingApi = {
    getStateBindingState?: (kind: string) => unknown;
    toggleStateDeclarationBinding?: (scope: string, name: string) => Promise<unknown>;
    toggleStateMachineBinding?: (scope: string, name: string) => Promise<unknown>;
    toggleStatePredicateBinding?: (scope: string, name: string) => Promise<unknown>;
    bindStateDeclarationToCharacter?: (avatar: string, name: string) => Promise<unknown>;
    getStateDeclarationBindingForAvatar?: (avatar: string) => unknown;
};

async function loadBindingApi(): Promise<PowerUserBindingApi | null> {
    try {
        const url = '/scripts/power-user.js';
        return await import(/* webpackIgnore: true */ url) as PowerUserBindingApi;
    } catch {
        return null;
    }
}

function normalizeScope(value: unknown): 'character' | 'group' | null {
    return value === 'character' || value === 'group' ? value : null;
}

/**
 * Read where this asset is bound, or `null` when the page runtime is unreadable.
 *
 * A panel renders its hint instead of its buttons in that case: there is nothing
 * it could honestly offer to bind.
 */
export async function readStateBinding(kind: StateBindingKind): Promise<StateBindingView | null> {
    const api = await loadBindingApi();
    const state = api?.getStateBindingState?.(kind);
    if (!state || typeof state !== 'object') {
        return null;
    }
    const raw = state as Partial<StateBindingView>;
    return {
        chatAvailable: raw.chatAvailable === true,
        entityScope: normalizeScope(raw.entityScope),
        chatName: String(raw.chatName || ''),
        entityName: String(raw.entityName || ''),
    };
}

/** Which exported toggle each kind reaches, so a fourth kind is one line. */
const BINDING_TOGGLES: Readonly<Record<StateBindingKind, keyof PowerUserBindingApi>> = Object.freeze({
    declaration: 'toggleStateDeclarationBinding',
    machine: 'toggleStateMachineBinding',
    predicates: 'toggleStatePredicateBinding',
});

/**
 * Bind the named asset to one target, or remove the binding when it is already
 * the bound name.
 *
 * Returns whether the target now binds `name`. Every failure — no runtime, no
 * usable target, a rejected save — is `false`, which is also what the caller's
 * own refresh will show, so a failed toggle cannot leave the panel claiming a
 * binding that was never written.
 */
export async function toggleStateBinding(
    kind: StateBindingKind,
    scope: StateBindingScope,
    name: string,
): Promise<boolean> {
    const api = await loadBindingApi();
    const toggle = api?.[BINDING_TOGGLES[kind]];
    if (typeof toggle !== 'function') {
        return false;
    }
    try {
        const bound = await toggle(scope, name) === true;
        if (bound) {
            announceBindingChanged();
        }
        return bound;
    } catch {
        return false;
    }
}

/**
 * Bind the named asset where nothing is bound yet, and leave a bound target alone.
 *
 * This is what an import and a shipped example call: bringing a scene in should
 * end with it on screen, and a panel whose declaration is bound to nothing renders
 * nothing wherever it is opened. The character or group is preferred — a scene
 * that belongs to a card should follow its card — and an open chat takes the
 * binding only when no entity is in play. A target that already binds something
 * keeps it: rebinding someone's chat out from under them is not an import's
 * decision.
 */
export async function ensureStateBinding(kind: StateBindingKind, name: string): Promise<boolean> {
    const trimmed = String(name || '').trim();
    if (!trimmed) {
        return false;
    }

    const view = await readStateBinding(kind);
    if (!view) {
        return false;
    }
    if (view.chatName === trimmed || view.entityName === trimmed) {
        return true;
    }
    if (view.entityScope && !view.entityName) {
        return toggleStateBinding(kind, 'entity', trimmed);
    }
    if (view.chatAvailable && !view.chatName) {
        return toggleStateBinding(kind, 'chat', trimmed);
    }
    return false;
}

/**
 * Bind the declaration to one character by avatar file name.
 *
 * The import path needs this and cannot use `ensureStateBinding`: a card is
 * imported with its own avatar in hand, while the character it belongs to may
 * not be the active one yet. `false` means nothing was written — the caller
 * reports the outcome rather than assuming a panel will show the scene.
 */
export async function bindStateDeclarationToCharacter(avatar: string, name: string): Promise<boolean> {
    const api = await loadBindingApi();
    const bind = api?.bindStateDeclarationToCharacter;
    if (typeof bind !== 'function') {
        return false;
    }
    try {
        const bound = await bind(avatar, name) === true;
        if (bound) {
            announceBindingChanged();
        }
        return bound;
    } catch {
        return false;
    }
}

/**
 * What one character binds, for a caller that holds an avatar rather than a panel.
 *
 * The empty string is the answer for every failure — no runtime, no such
 * character, nothing bound — because that is the answer the one caller acts on:
 * bind only where nothing is bound yet.
 */
export async function readStateDeclarationBindingForAvatar(avatar: string): Promise<string> {
    const api = await loadBindingApi();
    const read = api?.getStateDeclarationBindingForAvatar;
    if (typeof read !== 'function') {
        return '';
    }
    try {
        const name = read(avatar);
        return typeof name === 'string' ? name : '';
    } catch {
        return '';
    }
}

/**
 * Tell the page a state binding moved, so what it drives can redraw now.
 *
 * The state panel listens for chat and generation events, and a binding written
 * between them would otherwise stay invisible until the next one — a panel that
 * exists but shows nothing, which reads as broken.
 */
function announceBindingChanged(): void {
    window.dispatchEvent(new CustomEvent(BINDING_CHANGED_EVENT));
}

/** The event the page's state panel redraws on. */
export const BINDING_CHANGED_EVENT = 'tauritavern:state-binding-changed';

/** What the panel hands the binding actions: the calls, and where to put the answer. */
export type BindingActionDeps = {
    readBinding: () => Promise<StateBindingView | null>;
    toggleBinding: (scope: StateBindingScope, name: string) => Promise<boolean>;
    selectedName: () => string;
    isDisposed: () => boolean;
    commit: (patch: { binding?: StateBindingView | null }) => void;
};

export type BindingActions = {
    refreshBinding: () => Promise<void>;
    bindSelectedTo: (scope: StateBindingScope) => Promise<void>;
};

/**
 * The panel's two binding moves, as one pair.
 *
 * They belong together because the second always ends in the first: a toggle
 * whose outcome the panel does not re-read would leave it claiming a binding
 * that is not there. The outcome is not reported here either — `power-user.js`
 * owns the user-facing notification, and the refresh is what tells the panel
 * what actually happened.
 *
 * `readBinding` answering `null` is the one case that is not an error: the page
 * runtime could not be read, so the panel says so instead of offering buttons
 * that could not work.
 */
export function createBindingActions(deps: BindingActionDeps): BindingActions {
    async function refreshBinding(): Promise<void> {
        try {
            const binding = await deps.readBinding();
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ binding });
        } catch {
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ binding: null });
        }
    }

    async function bindSelectedTo(scope: StateBindingScope): Promise<void> {
        const name = deps.selectedName();
        if (!name) {
            return;
        }
        await deps.toggleBinding(scope, name);
        await refreshBinding();
    }

    return { refreshBinding, bindSelectedTo };
}
