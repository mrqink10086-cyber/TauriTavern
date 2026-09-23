/**
 * The recall policy in its two shapes.
 *
 * Stored, it says only what a Profile wants to differ on: whether this Agent
 * carries the blocks its extensions wrote, which prompt keys are recall, and
 * whether a delegated invocation inherits them. Edited, it also carries a CSV
 * mirror of the key list, because the panel's source field is typed into — the
 * arrangement the delegation policy uses for its caller list.
 *
 * Nothing here recalls anything. The blocks are written before a run starts and
 * frozen with the rest of its input, so a second retrieval would be the same
 * question of the same index with the same context.
 */

type RecallPolicy = NonNullable<TauriTavernAgentProfileDefinition['recall']>;

/** The same policy with every switch answered. */
export type ResolvedRecallPolicy = {
    inject: boolean;
    sources: string[];
    subagent: 'skip' | 'inherit';
};

export type AgentRecallDraft = RecallPolicy & {
    /** CSV mirror of `sources`; save-time normalization reads it when present. */
    sourcesCsv?: string;
};

/** The three controls the recall section shows, whatever the draft holds. */
export type AgentRecallView = {
    inject: boolean;
    sourcesCsv: string;
    subagentInherits: boolean;
};

/** The field a recall control edits. */
export type RecallField = 'inject' | 'subagent' | 'sources';

/**
 * What a Profile that says nothing about recall gets.
 *
 * The same policy the host applies to a Profile with no `recall` field: carrying
 * the blocks is the standing behavior, and a sub-agent does not inherit them.
 */
export const DEFAULT_RECALL_POLICY: ResolvedRecallPolicy = Object.freeze({
    inject: true,
    sources: ['3_vectfox*'],
    subagent: 'skip',
});

/** Anything that may carry a recall draft: a draft, a saved Profile, a fixture. */
export type RecallDraftHolder = { recall?: AgentRecallDraft | undefined };

/**
 * The recall policy a draft stands for.
 *
 * A draft can come from a saved Profile, from pasted JSON, or from a fixture, so
 * every reader goes through here rather than assuming the field is filled in.
 */
export function recallPolicyOf(draft: RecallDraftHolder): ResolvedRecallPolicy {
    const policy = draft.recall ?? DEFAULT_RECALL_POLICY;
    return {
        inject: policy.inject ?? DEFAULT_RECALL_POLICY.inject,
        sources: policy.sources ?? DEFAULT_RECALL_POLICY.sources,
        subagent: policy.subagent === 'inherit' ? 'inherit' : 'skip',
    };
}

export function recallViewOf(draft: RecallDraftHolder): AgentRecallView {
    const policy = recallPolicyOf(draft);
    return {
        inject: policy.inject,
        // The mirror wins: it is what the user last typed.
        sourcesCsv: draft.recall?.sourcesCsv ?? joinCsv(policy.sources),
        subagentInherits: policy.subagent === 'inherit',
    };
}

/** The draft a Profile starts as, with its source list mirrored as CSV. */
export function recallDraftFromPolicy(policy: TauriTavernAgentProfileDefinition['recall']): AgentRecallDraft {
    const resolved = recallPolicyOf({ recall: policy });
    return { ...resolved, sourcesCsv: joinCsv(resolved.sources) };
}

/** The draft one edit leaves behind: the switches, the mirror, and the change. */
export function applyRecallPatch(
    draft: RecallDraftHolder,
    patch: Partial<AgentRecallDraft>,
): AgentRecallDraft {
    const base: AgentRecallDraft = recallPolicyOf(draft);
    const mirror = draft.recall?.sourcesCsv;
    return mirror === undefined
        ? { ...base, ...patch }
        : { ...base, sourcesCsv: mirror, ...patch };
}

/**
 * The recall policy to save, from whatever the draft holds.
 *
 * A draft can hold the editor's shape (a CSV mirror) or the saved shape (a list),
 * depending on whether the field was typed into or JSON was pasted, so both are
 * read.
 */
export function normalizeRecallPolicy(value: unknown): RecallPolicy {
    const policy = isPlainObject(value) ? value : {};
    const sources = Object.prototype.hasOwnProperty.call(policy, 'sourcesCsv')
        ? parseCsv(policy.sourcesCsv)
        : (Array.isArray(policy.sources)
            ? policy.sources.map((source: unknown) => looseString(source).trim()).filter(Boolean)
            : [...DEFAULT_RECALL_POLICY.sources]);

    return {
        inject: Boolean(policy.inject ?? DEFAULT_RECALL_POLICY.inject),
        sources,
        subagent: policy.subagent === 'inherit' ? 'inherit' : 'skip',
    };
}

/** The draft one control leaves behind. */
export function applyRecallField(
    draft: RecallDraftHolder,
    field: RecallField,
    value: boolean | string,
): AgentRecallDraft {
    switch (field) {
        case 'inject':
            return applyRecallPatch(draft, { inject: Boolean(value) });
        case 'subagent':
            return applyRecallPatch(draft, { subagent: value ? 'inherit' : 'skip' });
        default:
            return applyRecallPatch(draft, { sourcesCsv: String(value) });
    }
}

/**
 * The recall controls, bound to a draft editor.
 *
 * Kept next to the policy they write, the way the access grid keeps its own
 * conversions: the panel's controller only has to spread them in.
 */
export function recallControls<Draft extends RecallDraftHolder>(
    editDraft: (mutate: (draft: Draft) => void) => void,
): { setRecallField: (field: RecallField, value: boolean | string) => void } {
    return {
        setRecallField(field, value) {
            editDraft((draft) => {
                draft.recall = applyRecallField(draft, field, value);
            });
        },
    };
}

function parseCsv(value: unknown): string[] {
    return looseString(value)
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean);
}

function joinCsv(values: unknown): string {
    return Array.isArray(values) ? values.join(', ') : '';
}

function looseString(value: unknown): string {
    return typeof value === 'string' ? value : '';
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}
