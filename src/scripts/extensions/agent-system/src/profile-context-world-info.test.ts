import { expect, test } from '@rstest/core';

import {
    worldInfoEntryCarried,
    worldInfoRulesWithEntry,
    worldInfoViewOf,
} from './profile-context-world-info';

const entry = { world: 'Char Lore', uid: 7 };

test('a delegated invocation reads nothing until its profile says so', () => {
    const view = worldInfoViewOf({ context: {} });

    expect(view.subagentInherits).toBe(false);
    expect(view.rules).toEqual([]);
    expect(worldInfoEntryCarried(view, entry)).toBe(false);
});

test('a row is an exception in both directions', () => {
    const closed = worldInfoViewOf({
        context: {
            worldInfo: {
                subagentInherits: false,
                entries: [{ book: 'Char Lore', uid: 7, inject: true }],
            },
        },
    });
    expect(worldInfoEntryCarried(closed, entry)).toBe(true);
    expect(worldInfoEntryCarried(closed, { world: 'Char Lore', uid: 8 })).toBe(false);

    const open = worldInfoViewOf({
        context: {
            worldInfo: {
                subagentInherits: true,
                entries: [{ book: 'Char Lore', uid: 7, inject: false }],
            },
        },
    });
    expect(worldInfoEntryCarried(open, entry)).toBe(false);
    expect(worldInfoEntryCarried(open, { world: 'Char Lore', uid: 8 })).toBe(true);
});

test('editing drops a row that agrees with the switch', () => {
    const closed = worldInfoViewOf({
        context: { worldInfo: { subagentInherits: false, entries: [] } },
    });
    expect(worldInfoRulesWithEntry(closed, entry, false)).toEqual([]);
    expect(worldInfoRulesWithEntry(closed, entry, true)).toEqual([
        { book: 'Char Lore', uid: 7, inject: true },
    ]);

    const open = worldInfoViewOf({
        context: {
            worldInfo: {
                subagentInherits: true,
                entries: [{ book: 'Char Lore', uid: 7, inject: false }],
            },
        },
    });
    expect(worldInfoRulesWithEntry(open, entry, true)).toEqual([]);
    expect(worldInfoRulesWithEntry(open, entry, false)).toEqual([
        { book: 'Char Lore', uid: 7, inject: false },
    ]);
});

test('a rule for another book does not decide this one', () => {
    const view = worldInfoViewOf({
        context: {
            worldInfo: {
                subagentInherits: true,
                entries: [{ book: 'Other Book', uid: 7, inject: false }],
            },
        },
    });

    expect(worldInfoEntryCarried(view, entry)).toBe(true);
});

test('an entry without a book or an id cannot be ruled on', () => {
    const view = worldInfoViewOf({
        context: { worldInfo: { subagentInherits: false, entries: [] } },
    });

    expect(worldInfoRulesWithEntry(view, { world: '  ', uid: 7 }, true)).toEqual([]);
    expect(worldInfoRulesWithEntry(view, { world: 'Char Lore', uid: Number.NaN }, true)).toEqual([]);
    expect(worldInfoEntryCarried(view, { world: 'Char Lore' })).toBe(false);
});
