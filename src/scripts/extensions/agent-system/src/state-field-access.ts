/**
 * What one declared field grants unless a Profile overrides it.
 *
 * Mirrors the domain's `StateFieldAccess`. The switches are named for what they
 * do rather than for how they are stored: a field the model is told about every
 * turn, one it may look up, and one it may change.
 *
 * This is the default a Profile's access rows override. Before field defaults
 * existed the answer lived only in those rows, which made "no row" mean "nothing
 * is decided" and left a reader with no way to tell a field's intent from a
 * Profile's silence.
 */

export type StateFieldAccessSpec = {
    inject?: boolean;
    visible?: boolean;
    writable?: boolean;
    injectSlot?: 'before' | 'after' | 'atDepth';
    injectDepth?: number;
};

/** One field's switches, with every part present. */
export type ResolvedFieldAccess = Required<StateFieldAccessSpec>;

/**
 * What a declared field grants when it says nothing.
 *
 * The domain's answer, repeated so a new row shows the same switches the backend
 * will apply: naming a field in the declaration says the model should know it,
 * so its value is injected every turn. Visibility and writability are extra
 * authority — a field can be tracked without letting every Agent pull on it or
 * rewrite it — so both stay off until a Profile row grants them.
 */
export const DEFAULT_FIELD_ACCESS: ResolvedFieldAccess = Object.freeze({
    inject: true,
    visible: false,
    writable: false,
    injectSlot: 'atDepth',
    injectDepth: 4,
});

/** One field's switches, with the defaults filled in for the editor's benefit. */
export function fieldAccessOf(field: { access?: StateFieldAccessSpec } | null | undefined): ResolvedFieldAccess {
    return { ...DEFAULT_FIELD_ACCESS, ...(field?.access ?? {}) };
}
