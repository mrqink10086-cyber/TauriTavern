/**
 * The state machine editor's documents, as the editor holds them.
 *
 * The draft keeps what the user types — CSV text for position lists, text for
 * numbers — and only `normalizeMachineForSave` builds the finished spec. The
 * backend owns every semantic rule; the editor only refuses to guess. Script
 * hooks are a pass-through: this editor does not edit them, and a loaded hook
 * is stored back byte-identical rather than silently dropped.
 */

export type MachineCompose = { all: MachineCondition[] } | { any: MachineCondition[] } | { not: MachineCondition };

/** A comparison as the document stores it (`compose` combines parts instead). */
export type MachineCondition = {
    source: string;
    field?: string | null;
    op: string;
    value?: string | null;
    values?: string[];
    compose?: MachineCompose | null;
};

export type MachineAction = {
    kind: string;
    target?: string | null;
    values?: string[];
};

export type MachineTransition = {
    id?: string | null;
    from: string[];
    to: string[];
    conditions: MachineCondition[];
    actions: MachineAction[];
    priority?: number;
};

export type MachineStateSpec = {
    id: string;
    label?: string | null;
    terminal?: boolean;
};

/** A script hook, shared with picture-set condition scripts. Pass-through here. */
export type MachineHook = { script: string; entry?: string | null };

export type MachineSpec = {
    initial: string[];
    states: MachineStateSpec[];
    transitions: MachineTransition[];
    hooks?: MachineHook | null;
};

export type MachineErrorDto = { target?: string | null; code: string; message: string };

export type MachineRunDto = {
    evaluation: {
        active: string[];
        applied: Array<{ index: number; id?: string | null; from: string[]; to: string[] }>;
        skipped: Array<{ index: number; id?: string | null; reason: string }>;
        writes: Array<{ key: string; values: string[] }>;
        events: string[];
        hooks: Array<{ index: number; id?: string | null; from: string[]; to: string[] }>;
    };
    errors: MachineErrorDto[];
};

/** The condition as the editor holds it: one text line per side. */
export type MachineConditionDraft = {
    sourceText: string;
    fieldText: string;
    op: string;
    valueText: string;
    /** A loaded composition; kept as stored and never edited here. */
    compose?: MachineCompose | null;
};

export type MachineActionDraft = { kind: string; targetText: string; valuesText: string };

export type MachineTransitionDraft = {
    fromText: string;
    toText: string;
    priorityText: string;
    conditions: MachineConditionDraft[];
    actions: MachineActionDraft[];
};

export type MachineStateDraft = { id: string; labelText: string; terminal: boolean };

export type MachineDraft = {
    initialText: string;
    states: MachineStateDraft[];
    transitions: MachineTransitionDraft[];
    hooks: MachineHook | null;
};

/** Front-side early warnings; the backend is still the only authority. */
export type MachineConfigIssue = {
    code: 'duplicateState' | 'unknownState' | 'emptyTransition';
    target: string;
};

/** Split a comma list, keeping `/regex/flags`-style text whole is not needed for ids. */
export function splitIdCsv(text: string): string[] {
    return String(text ?? '')
        .split(',')
        .map((part) => part.trim())
        .filter((part) => part.length > 0);
}

export function emptyMachineDraft(): MachineDraft {
    return { initialText: '', states: [], transitions: [], hooks: null };
}

export function machineDraftFromSpec(spec: MachineSpec): MachineDraft {
    return {
        initialText: (spec.initial ?? []).join(', '),
        states: (spec.states ?? []).map((state) => ({
            id: state.id,
            labelText: state.label ?? '',
            terminal: state.terminal === true,
        })),
        transitions: (spec.transitions ?? []).map((transition) => ({
            fromText: (transition.from ?? []).join(', '),
            toText: (transition.to ?? []).join(', '),
            priorityText: transition.priority ? String(transition.priority) : '',
            conditions: (transition.conditions ?? []).map(conditionDraft),
            actions: (transition.actions ?? []).map((action) => ({
                kind: action.kind || 'setField',
                targetText: action.target ?? '',
                valuesText: (action.values ?? []).join(', '),
            })),
        })),
        hooks: spec.hooks ?? null,
    };
}

function conditionDraft(condition: MachineCondition): MachineConditionDraft {
    if (condition.compose) {
        return { sourceText: '', fieldText: '', op: '', valueText: '', compose: condition.compose };
    }
    const values = condition.values ?? [];
    const valueText = values.length > 0 ? values.join(', ') : String(condition.value ?? '');
    return {
        sourceText: condition.source || 'field',
        fieldText: condition.field ?? '',
        op: condition.op || 'eq',
        valueText,
    };
}

function conditionFromDraft(draft: MachineConditionDraft): MachineCondition | null {
    if (draft.compose) {
        // A composition is stored back exactly as loaded; this editor does not
        // rebuild it, so nothing about it can drift.
        return { source: '', op: '', compose: draft.compose };
    }
    const source = draft.sourceText.trim() || 'field';
    const field = draft.fieldText.trim();
    const op = draft.op.trim() || 'eq';
    if (!field && !['exists', 'missing'].includes(op)) {
        return null;
    }
    if (op === 'in' || op === 'not_in') {
        return { source, field: field || null, op, values: splitIdCsv(draft.valueText) };
    }
    if (op === 'exists' || op === 'missing') {
        return { source, field: field || null, op };
    }
    const value = draft.valueText.trim();
    return { source, field: field || null, op, value: value || null };
}

function actionFromDraft(draft: MachineActionDraft): MachineAction | null {
    const kind = draft.kind.trim() || 'setField';
    const target = draft.targetText.trim();
    if (!target) {
        return null;
    }
    if (kind === 'setField') {
        return { kind, target, values: splitIdCsv(draft.valuesText) };
    }
    return { kind, target };
}

/**
 * The finished spec, built from what the user typed.
 *
 * A half-filled row is dropped rather than stored broken — the same convention
 * the declaration editor follows. A transition row with nothing in it at all
 * reads as "not really written" and disappears.
 */
export function normalizeMachineForSave(draft: MachineDraft): MachineSpec {
    const states: MachineStateSpec[] = [];
    for (const state of draft.states ?? []) {
        const id = state.id.trim();
        if (!id) {
            continue;
        }
        const spec: MachineStateSpec = { id };
        const label = state.labelText.trim();
        if (label) {
            spec.label = label;
        }
        if (state.terminal) {
            spec.terminal = true;
        }
        states.push(spec);
    }

    const transitions = (draft.transitions ?? [])
        .map((transition) => {
            const from = splitIdCsv(transition.fromText);
            const to = splitIdCsv(transition.toText);
            const conditions = (transition.conditions ?? [])
                .map(conditionFromDraft)
                .filter((condition): condition is MachineCondition => condition !== null);
            const actions = (transition.actions ?? [])
                .map(actionFromDraft)
                .filter((action): action is MachineAction => action !== null);
            const touched = from.length > 0 || to.length > 0 || conditions.length > 0 || actions.length > 0;
            if (!touched) {
                return null;
            }
            const priority = Number.parseInt(transition.priorityText.trim(), 10);
            return {
                from,
                to,
                conditions,
                actions,
                ...(Number.isFinite(priority) && priority !== 0 ? { priority } : {}),
            } satisfies MachineTransition;
        })
        .filter((transition): transition is MachineTransition => transition !== null);

    return {
        initial: splitIdCsv(draft.initialText),
        states,
        transitions,
        hooks: draft.hooks ?? null,
    };
}

/** Every id the draft's transitions and initial list refer to. */
function referencedIds(draft: MachineDraft): Map<string, 'initial' | 'transition'> {
    const refs = new Map<string, 'initial' | 'transition'>();
    for (const id of splitIdCsv(draft.initialText)) {
        if (!refs.has(id)) {
            refs.set(id, 'initial');
        }
    }
    for (const transition of draft.transitions ?? []) {
        for (const id of [...splitIdCsv(transition.fromText), ...splitIdCsv(transition.toText)]) {
            if (!refs.has(id)) {
                refs.set(id, 'transition');
            }
        }
    }
    return refs;
}

export function machineDraftIssues(draft: MachineDraft): MachineConfigIssue[] {
    const issues: MachineConfigIssue[] = [];
    const seen = new Map<string, number>();
    for (const state of draft.states ?? []) {
        const id = state.id.trim();
        if (!id) {
            continue;
        }
        seen.set(id, (seen.get(id) ?? 0) + 1);
        if (seen.get(id) === 2) {
            issues.push({ code: 'duplicateState', target: id });
        }
    }
    const known = new Set((draft.states ?? []).map((state) => state.id.trim()).filter((id) => id.length > 0));
    for (const [id, where] of referencedIds(draft)) {
        if (!known.has(id)) {
            issues.push({ code: 'unknownState', target: `${id} (${where})` });
        }
    }
    for (const [index, transition] of (draft.transitions ?? []).entries()) {
        const touched = splitIdCsv(transition.fromText).length > 0
            || splitIdCsv(transition.toText).length > 0
            || transition.conditions.some((condition) => condition.compose || condition.fieldText.trim() || condition.valueText.trim());
        if (!touched) {
            issues.push({ code: 'emptyTransition', target: `#${index + 1}` });
        }
    }
    return issues;
}
