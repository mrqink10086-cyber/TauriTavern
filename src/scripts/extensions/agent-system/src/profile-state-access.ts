/**
 * The state access policy in its two shapes.
 *
 * Stored, an entry says only what it needs to: the switches, plus a slot and
 * depth when they differ from the backend's defaults. Edited, a row says
 * everything, because a form has to show a slot and a depth in every row — and
 * a depth input is empty while it is being retyped, which the stored shape
 * cannot hold.
 */

export type StateAccessSlot = 'before' | 'after' | 'atDepth';

/**
 * One row of the access grid.
 *
 * Everything but the key is optional, because a profile definition read from
 * disk *is* a valid draft and its entries say only what differs from the
 * defaults. Rows the editor built carry all of them; rows that came from a
 * document may not, and the readers below fill the gaps.
 */
export type StateAccessRow = {
    pattern: string;
    inject?: boolean;
    visible?: boolean;
    writable?: boolean;
    injectSlot?: StateAccessSlot;
    /** Empty while the input is being retyped; the save drops it. */
    injectDepth?: number | '';
};

/** The depth the backend applies when an entry does not name one. */
export const DEFAULT_STATE_ACCESS_DEPTH = 4;

type StateAccessPolicy = TauriTavernAgentProfileDefinition['stateAccess'];
type StateAccessEntry = NonNullable<NonNullable<StateAccessPolicy>['entries']>[number];

/** The rows the grid shows: what a profile stores, with the defaults filled in. */
export function stateAccessRowsFromPolicy(policy: StateAccessPolicy): StateAccessRow[] {
    return (policy?.entries ?? []).map((entry) => ({
        pattern: entry.pattern,
        inject: entry.inject === true,
        visible: entry.visible === true,
        writable: entry.writable === true,
        injectSlot: entry.injectSlot ?? 'atDepth',
        injectDepth: entry.injectDepth ?? DEFAULT_STATE_ACCESS_DEPTH,
    }));
}

/** One category of the grid: rows sharing a pattern's top-level segment. */
export type StateAccessRowGroup = {
    /** The segment text, `null` when the pattern has none (regex or blank). */
    category: string | null;
    /** A `/pattern/flags` row groups under the regex label, not its raw head. */
    regex: boolean;
    rows: Array<{ row: StateAccessRow; index: number }>;
};

/**
 * Group the grid by category, presentation only.
 *
 * The first `/`-separated segment is the category (`环境/日期` and `环境/时间`
 * sit together). Group order follows first appearance, and every row keeps its
 * original index, so collapse and edit never move data.
 */
export function groupStateAccessRows(rows: readonly StateAccessRow[]): StateAccessRowGroup[] {
    const groups = new Map<string, StateAccessRowGroup>();
    for (const [index, row] of rows.entries()) {
        const pattern = String(row.pattern ?? '').trim();
        const regex = pattern.startsWith('/');
        const category = !pattern || regex ? null : (pattern.split('/')[0] || null);
        const key = category ?? (regex ? '__regex__' : '__other__');
        let group = groups.get(key);
        if (!group) {
            group = { category, regex, rows: [] };
            groups.set(key, group);
        }
        group.rows.push({ row, index });
    }
    return [...groups.values()];
}

/**
 * The entries as they are stored.
 *
 * A row with no key covers nothing and would be refused by the backend, so a
 * half-typed row is dropped here — the same convention a blank field row
 * follows. Slot and depth are written only when they differ from the defaults,
 * because a document that repeats the default in every row is harder to read
 * than one that says nothing.
 */
export function stateAccessEntriesFromRows(rows: readonly StateAccessRow[]): StateAccessEntry[] {
    return rows
        .map((row) => ({ ...row, pattern: String(row.pattern ?? '').trim() }))
        .filter((row) => row.pattern.length > 0)
        .map((row) => {
            const entry: StateAccessEntry = {
                pattern: row.pattern,
                inject: row.inject === true,
                visible: row.visible === true,
                writable: row.writable === true,
            };
            const injectSlot = row.injectSlot ?? 'atDepth';
            if (injectSlot !== 'atDepth') {
                entry.injectSlot = injectSlot;
            }
            if (typeof row.injectDepth === 'number' && row.injectDepth !== DEFAULT_STATE_ACCESS_DEPTH) {
                entry.injectDepth = row.injectDepth;
            }
            return entry;
        });
}
