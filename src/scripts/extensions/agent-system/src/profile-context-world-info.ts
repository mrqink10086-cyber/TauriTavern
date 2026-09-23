/**
 * The World Info exceptions a Profile's context policy carries.
 *
 * An entry is named by its book and its id, the pair the scan keys entries by: a
 * comment is a title a human wrote and may repeat, and the id alone says nothing
 * across books.
 *
 * A row is an exception, in both directions — `inject: false` keeps one entry out
 * while the switch says entries come through, `inject: true` lets one through
 * while the switch says they do not. The switch it is read against differs by who
 * is asking: the Profile's own prompt follows `includeActivatedWorldInfo`, and a
 * delegated invocation follows `subagentInherits`.
 *
 * Editing drops a row that agrees with the switch: what a reader set is a decision
 * about an entry, and a decision the switch already makes is not worth storing —
 * it would silently change meaning the next time the switch is flipped.
 */

import type { AgentProfileDraft, WorldInfoEntryRule } from './profile-model';

type WorldInfoPolicy = {
    entries?: ReadonlyArray<WorldInfoEntryRule>;
    subagentInherits?: boolean;
};

/** Anything that may carry the policy: a draft, a saved Profile, a pasted JSON. */
export type WorldInfoPolicyHolder = { context?: { worldInfo?: WorldInfoPolicy | undefined } };

export type WorldInfoView = {
    subagentInherits: boolean;
    rules: ReadonlyArray<WorldInfoEntryRule>;
};

export function worldInfoViewOf(holder: WorldInfoPolicyHolder): WorldInfoView {
    return {
        subagentInherits: holder.context?.worldInfo?.subagentInherits === true,
        rules: holder.context?.worldInfo?.entries ?? [],
    };
}

/** Whether a delegated invocation carries that entry, rows and switch together. */
export function worldInfoEntryCarried(
    view: WorldInfoView,
    entry: { world?: unknown; uid?: unknown },
): boolean {
    const rule = findRule(view.rules, entry);
    return rule ? rule.inject : view.subagentInherits;
}

export function worldInfoEntryRuleOf(
    view: WorldInfoView,
    entry: { world?: unknown; uid?: unknown },
): WorldInfoEntryRule | undefined {
    return findRule(view.rules, entry);
}

/** The rules after one entry's decision, with a row that says nothing dropped. */
export function worldInfoRulesWithEntry(
    view: WorldInfoView,
    entry: { world?: unknown; uid?: unknown },
    carried: boolean,
): WorldInfoEntryRule[] {
    const key = entryKey(entry);
    if (!key) {
        return [...view.rules];
    }

    const rest = view.rules.filter((rule) => `${rule.book}.${rule.uid}` !== `${key.book}.${key.uid}`);
    return carried === view.subagentInherits
        ? rest
        : [...rest, { book: key.book, uid: key.uid, inject: carried }];
}

export function entryKey(entry: { world?: unknown; uid?: unknown }): { book: string; uid: number } | null {
    const book = typeof entry?.world === 'string' ? entry.world.trim() : '';
    const uid = Number(entry?.uid);
    return book && Number.isFinite(uid) ? { book, uid } : null;
}

function findRule(
    rules: ReadonlyArray<WorldInfoEntryRule>,
    entry: { world?: unknown; uid?: unknown },
): WorldInfoEntryRule | undefined {
    const key = entryKey(entry);
    return key
        ? rules.find((rule) => rule.book === key.book && rule.uid === key.uid)
        : undefined;
}

/**
 * The World Info controls, bound to a draft editor.
 *
 * Kept next to the policy they write, the way the recall controls are: the panel's
 * controller only has to spread them in.
 */
export function worldInfoControls<Draft extends WorldInfoPolicyHolder & AgentProfileDraft>(
    editDraft: (mutate: (draft: Draft) => void) => void,
): {
    setWorldInfoSubagentInherits: (inherits: boolean) => void;
    setWorldInfoEntry: (entry: { world?: unknown; uid?: unknown }, carried: boolean) => void;
    clearWorldInfoRules: () => void;
} {
    // Written whole rather than patched: an empty policy is what a Profile with no
    // exception stores, and the save drops it, so there is nothing to clean up
    // here. A row that agrees with the switch is dropped by the caller below.
    const write = (draft: Draft, policy: { entries: WorldInfoEntryRule[]; subagentInherits: boolean }): void => {
        draft.context = { ...draft.context, worldInfo: policy };
    };

    const policyOf = (draft: Draft) => {
        const view = worldInfoViewOf(draft);
        return { entries: [...view.rules], subagentInherits: view.subagentInherits };
    };

    return {
        setWorldInfoSubagentInherits(inherits) {
            editDraft((draft) => {
                const policy = policyOf(draft);
                write(draft, { ...policy, subagentInherits: inherits });
            });
        },
        setWorldInfoEntry(entry, carried) {
            editDraft((draft) => {
                const policy = policyOf(draft);
                const view = worldInfoViewOf(draft);
                write(draft, {
                    ...policy,
                    entries: worldInfoRulesWithEntry(view, entry, carried),
                });
            });
        },
        clearWorldInfoRules() {
            editDraft((draft) => {
                write(draft, { ...policyOf(draft), entries: [] });
            });
        },
    };
}
