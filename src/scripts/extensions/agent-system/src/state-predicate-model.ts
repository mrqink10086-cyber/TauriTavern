/**
 * The predicate set editor's documents, as the editor holds them.
 *
 * A predicate set answers a question the world book cannot: "is the story in
 * this state?" — and the answer comes from the state document, through the same
 * condition vocabulary the machine and the panels use. The draft keeps what the
 * user types (text for numbers, comma lists for labels) and only
 * `normalizePredicateSetForSave` builds the finished document; the backend owns
 * every semantic rule and refuses with its own words.
 *
 * Two rules shape the editor:
 *
 * - A group selects exactly one entry, by priority with declaration order
 *   breaking ties. "Keeping distance" and "acting close" cannot both hold, so
 *   both are never injected — the model only ever sees the outcome.
 * - An entry acts on others by label: inhibit removes them, require holds this
 *   one back until one of them is selected. That relation is decided by the
 *   evaluator, not by the model.
 *
 * A condition the editor cannot render (a composition) is passed through as
 * stored, exactly like the machine editor does: nothing about it can drift.
 */

import { splitIdCsv } from './state-machine-model';

export type PredicateCompose =
    | { all: PredicateCondition[] }
    | { any: PredicateCondition[] }
    | { not: PredicateCondition };

/** A comparison as the document stores it (`compose` combines parts instead). */
export type PredicateCondition = {
    source: string;
    field?: string | null;
    op: string;
    value?: string | null;
    values?: string[];
    compose?: PredicateCompose | null;
};

/** When an entry is a candidate at all. */
export type PredicateSource =
    | { kind: 'constant' }
    | { kind: 'state'; condition: PredicateCondition };

export type PredicateEffectKind = 'inhibit' | 'require';

export type PredicateEffect = {
    kind: PredicateEffectKind;
    tags?: string[];
};

export type PredicateEntry = {
    id: string;
    label?: string | null;
    content: string;
    tags?: string[];
    source?: PredicateSource;
    availability?: PredicateCondition | null;
    effects?: PredicateEffect[];
    priority?: number;
};

/** Entries that cannot hold at the same time; the group picks one. */
export type PredicateGroup = {
    id: string;
    label?: string | null;
    entries?: PredicateEntry[];
};

export type StatePredicateSet = {
    groups?: PredicateGroup[];
    /** Entries that never compete — a standing instruction belongs here. */
    constants?: PredicateEntry[];
};

/** Where an entry lives: inside one group, or among the standing entries. */
export type PredicateEntryTarget =
    | { kind: 'group'; groupIndex: number }
    | { kind: 'constants' };

/** A condition as the editor holds it: one text field per side. */
export type PredicateConditionDraft = {
    fieldText: string;
    op: string;
    valueText: string;
    /** A loaded composition; kept as stored and never edited here. */
    compose?: PredicateCompose | null;
};

export type PredicateEffectDraft = { kind: PredicateEffectKind; tagsText: string };

export type PredicateEntryDraft = {
    id: string;
    labelText: string;
    content: string;
    tagsText: string;
    /** `state` reads a state field; `constant` is always a candidate. */
    sourceKind: 'constant' | 'state';
    condition: PredicateConditionDraft | null;
    /** An extra premise. `null` means the entry has none. */
    availability: PredicateConditionDraft | null;
    effects: PredicateEffectDraft[];
    priorityText: string;
};

export type PredicateGroupDraft = {
    id: string;
    labelText: string;
    entries: PredicateEntryDraft[];
};

export type PredicateSetDraft = {
    groups: PredicateGroupDraft[];
    constants: PredicateEntryDraft[];
};

/** What one evaluation reports: what holds, and why the rest did not. */
export type PredicateEvaluationDto = {
    selected: Array<{ groupId?: string | null; entryId: string; content: string }>;
    skipped: Array<{ entryId: string; reason: string }>;
};

/** Front-side early warnings; the backend is still the only authority. */
export type PredicateConfigIssue = {
    code: 'duplicateGroupId' | 'duplicateEntryId' | 'entryConditionIncomplete' | 'effectTagsRequired';
    target: string;
};

export function emptyPredicateSet(): StatePredicateSet {
    return { groups: [], constants: [] };
}

export function emptyPredicateDraft(): PredicateSetDraft {
    return { groups: [], constants: [] };
}

export function emptyConditionDraft(): PredicateConditionDraft {
    return { fieldText: '', op: 'eq', valueText: '' };
}

/** What a new entry starts as: nothing filled in, nothing that would be stored. */
export function emptyPredicateEntry(): PredicateEntryDraft {
    return {
        id: '',
        labelText: '',
        content: '',
        tagsText: '',
        sourceKind: 'constant',
        condition: null,
        availability: null,
        effects: [],
        priorityText: '',
    };
}

/** Rebuild the draft around a new entry list for one target. */
export function withEntries(
    draft: PredicateSetDraft,
    target: PredicateEntryTarget,
    update: (entries: PredicateEntryDraft[]) => PredicateEntryDraft[],
): PredicateSetDraft {
    if (target.kind === 'constants') {
        return { ...draft, constants: update(draft.constants ?? []) };
    }
    return {
        ...draft,
        groups: draft.groups.map((group, at) => (
            at !== target.groupIndex ? group : { ...group, entries: update(group.entries ?? []) }
        )),
    };
}

function conditionDraftFrom(condition: PredicateCondition | null | undefined): PredicateConditionDraft | null {
    if (!condition) {
        return null;
    }
    if (condition.compose) {
        return { fieldText: '', op: '', valueText: '', compose: condition.compose };
    }
    const values = condition.values ?? [];
    return {
        fieldText: String(condition.field ?? ''),
        op: condition.op || 'eq',
        valueText: values.length > 0 ? values.join(', ') : String(condition.value ?? ''),
    };
}

function effectsDraftFrom(effects: PredicateEffect[] | undefined): PredicateEffectDraft[] {
    return (effects ?? []).map((effect) => ({
        kind: effect.kind === 'require' ? 'require' : 'inhibit',
        tagsText: (effect.tags ?? []).join(', '),
    }));
}

function entryDraftFrom(entry: PredicateEntry): PredicateEntryDraft {
    const source = entry.source ?? { kind: 'constant' as const };
    return {
        id: entry.id ?? '',
        labelText: String(entry.label ?? ''),
        content: entry.content ?? '',
        tagsText: (entry.tags ?? []).join(', '),
        sourceKind: source.kind === 'state' ? 'state' : 'constant',
        condition: source.kind === 'state' ? conditionDraftFrom(source.condition) : null,
        availability: conditionDraftFrom(entry.availability),
        effects: effectsDraftFrom(entry.effects),
        priorityText: entry.priority ? String(entry.priority) : '',
    };
}

export function predicateDraftFromSet(set: StatePredicateSet): PredicateSetDraft {
    return {
        groups: (set.groups ?? []).map((group) => ({
            id: group.id ?? '',
            labelText: String(group.label ?? ''),
            entries: (group.entries ?? []).map(entryDraftFrom),
        })),
        constants: (set.constants ?? []).map(entryDraftFrom),
    };
}

/**
 * A condition the editor can build.
 *
 * `in` / `not_in` carry a list; `exists` / `missing` carry no value at all;
 * everything else carries one. A row with no field is not a condition yet, and
 * `null` says so instead of storing a comparison that reads nothing.
 */
function conditionFromDraft(draft: PredicateConditionDraft | null): PredicateCondition | null {
    if (!draft) {
        return null;
    }
    if (draft.compose) {
        // A composition is stored back exactly as loaded; this editor does not
        // rebuild it, so nothing about it can drift.
        return { source: 'field', op: '', compose: draft.compose };
    }
    const field = draft.fieldText.trim();
    const op = draft.op.trim() || 'eq';
    if (!field && !['exists', 'missing'].includes(op)) {
        return null;
    }
    if (op === 'in' || op === 'not_in') {
        return { source: 'field', field, op, values: splitIdCsv(draft.valueText) };
    }
    if (op === 'exists' || op === 'missing') {
        return { source: 'field', field, op };
    }
    return { source: 'field', field, op, value: draft.valueText.trim() || null };
}

function effectsFromDraft(draft: PredicateEntryDraft): PredicateEffect[] {
    return draft.effects
        .map((effect) => ({ kind: effect.kind, tags: splitIdCsv(effect.tagsText) }))
        // An effect that reaches no label can never do anything; a blank row is
        // a row the user has not filled in, not a rule they wrote.
        .filter((effect) => effect.tags.length > 0);
}

function entryFromDraft(draft: PredicateEntryDraft): PredicateEntry | null {
    const id = draft.id.trim();
    const content = draft.content ?? '';
    if (!id && !content.trim()) {
        return null;
    }
    const entry: PredicateEntry = { id, content };
    const label = draft.labelText.trim();
    if (label) {
        entry.label = label;
    }
    const tags = splitIdCsv(draft.tagsText);
    if (tags.length > 0) {
        entry.tags = tags;
    }
    if (draft.sourceKind === 'state') {
        // A condition that cannot be built leaves the entry constant rather than
        // dropping it: `predicateDraftIssues` says so before the save is sent.
        const condition = conditionFromDraft(draft.condition);
        entry.source = condition ? { kind: 'state', condition } : { kind: 'constant' };
    }
    const availability = conditionFromDraft(draft.availability);
    if (availability) {
        entry.availability = availability;
    }
    const effects = effectsFromDraft(draft);
    if (effects.length > 0) {
        entry.effects = effects;
    }
    const priority = Number.parseInt(draft.priorityText.trim(), 10);
    if (Number.isFinite(priority) && priority !== 0) {
        entry.priority = priority;
    }
    return entry;
}

function entriesFromDraft(entries: PredicateEntryDraft[]): PredicateEntry[] {
    return entries
        .map(entryFromDraft)
        .filter((entry): entry is PredicateEntry => entry !== null);
}

/**
 * The finished document, built from what the user typed.
 *
 * A half-filled row is dropped rather than stored broken — the same convention
 * the declaration and machine editors follow.
 */
export function normalizePredicateSetForSave(draft: PredicateSetDraft): StatePredicateSet {
    const groups: PredicateGroup[] = [];
    for (const group of draft.groups ?? []) {
        const id = group.id.trim();
        if (!id) {
            continue;
        }
        const spec: PredicateGroup = { id, entries: entriesFromDraft(group.entries ?? []) };
        const label = group.labelText.trim();
        if (label) {
            spec.label = label;
        }
        groups.push(spec);
    }
    return { groups, constants: entriesFromDraft(draft.constants ?? []) };
}

function entriesOf(draft: PredicateSetDraft): PredicateEntryDraft[] {
    return [
        ...(draft.groups ?? []).flatMap((group) => group.entries ?? []),
        ...(draft.constants ?? []),
    ];
}

function conditionIsIncomplete(draft: PredicateConditionDraft | null): boolean {
    if (!draft) {
        return true;
    }
    if (draft.compose) {
        return false;
    }
    return draft.fieldText.trim().length === 0;
}

export function predicateDraftIssues(draft: PredicateSetDraft): PredicateConfigIssue[] {
    const issues: PredicateConfigIssue[] = [];

    const groupIds = new Set<string>();
    for (const group of draft.groups ?? []) {
        const id = group.id.trim();
        if (!id) {
            continue;
        }
        if (groupIds.has(id)) {
            issues.push({ code: 'duplicateGroupId', target: id });
        }
        groupIds.add(id);
    }

    const entryIds = new Set<string>();
    for (const entry of entriesOf(draft)) {
        const id = entry.id.trim();
        if (!id) {
            continue;
        }
        if (entryIds.has(id)) {
            issues.push({ code: 'duplicateEntryId', target: id });
        }
        entryIds.add(id);
        if (entry.sourceKind === 'state' && conditionIsIncomplete(entry.condition)) {
            issues.push({ code: 'entryConditionIncomplete', target: id });
        }
        if (entry.availability && conditionIsIncomplete(entry.availability)) {
            issues.push({ code: 'entryConditionIncomplete', target: `${id} (premise)` });
        }
        for (const effect of entry.effects ?? []) {
            if (splitIdCsv(effect.tagsText).length === 0) {
                issues.push({ code: 'effectTagsRequired', target: id });
            }
        }
    }

    return issues;
}
