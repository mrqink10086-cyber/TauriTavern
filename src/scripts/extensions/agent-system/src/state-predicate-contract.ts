/**
 * What the predicate set editor promises its host.
 *
 * The snapshot a panel renders, the calls it makes, and what the controller
 * offers back — the same shape the other first-party panels use, kept in its own
 * file so the controller itself stays a controller rather than growing a
 * contract at the top of it.
 */

import type { Tr } from './AgentSystemPanelContract';
import type {
    PredicateConditionDraft,
    PredicateConfigIssue,
    PredicateEffectDraft,
    PredicateEntryDraft,
    PredicateEntryTarget,
    PredicateEvaluationDto,
    PredicateGroupDraft,
    PredicateSetDraft,
    StatePredicateSet,
} from './state-predicate-model';
import type { StateBindingScope, StateBindingView } from './state-binding';

/** One assumed field value, as the preview holds it. */
export type PredicatePreviewField = { keyText: string; valuesText: string };

export type PredicateConfigSnapshot = {
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    /** The last thing that went right, worth keeping in front of the user. */
    notice: string;
    names: string[];
    selectedName: string;
    /** The name a new set would be stored under, while it is typed. */
    newName: string;
    draft: PredicateSetDraft | null;
    /** The draft as the whole document, which is what the JSON box edits. */
    draftJson: string;
    /** Why the box's text was not read, or `''` when there is nothing to say. */
    jsonError: string;
    /** The stored document the draft is compared against, for the dirty flag. */
    savedJson: string;
    issues: PredicateConfigIssue[];
    previewFields: PredicatePreviewField[];
    previewing: boolean;
    preview: PredicateEvaluationDto | null;
    /**
     * Where the selected set is bound, or `null` when the page runtime could not
     * be read at all — the panel shows a hint instead of buttons.
     */
    binding: StateBindingView | null;
};

export type PredicateConfigControllerDeps = {
    listSets: () => Promise<string[]>;
    getSet: (name: string) => Promise<StatePredicateSet>;
    saveSet: (name: string, set: StatePredicateSet) => Promise<void>;
    deleteSet: (name: string) => Promise<void>;
    /** Run one set against assumed field values. Nothing is stored. */
    evaluate: (input: {
        set: StatePredicateSet;
        fields?: Record<string, string[]>;
    }) => Promise<PredicateEvaluationDto>;
    confirmAction: (message: string) => Promise<boolean>;
    notifyError: (error: unknown) => void;
    notifySuccess: (message: string) => void;
    /** Save a predicate set file through the host, so it lands where downloads land. */
    downloadBlob: (blob: Blob, fileName: string) => Promise<{ mode?: string; completed?: boolean } | undefined>;
    /** Where the selected set is bound right now, or null if unreadable. */
    readBinding: () => Promise<StateBindingView | null>;
    /** Bind `name`, or unbind it when that target already binds it. */
    toggleBinding: (scope: StateBindingScope, name: string) => Promise<boolean>;
    tr: Tr;
};

export type PredicateConfigController = {
    getSnapshot: () => PredicateConfigSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    refresh: () => Promise<void>;
    /** Whether closing the editor may go ahead, asked only when edits are pending. */
    confirmPendingEdits: () => Promise<boolean>;
    selectSet: (name: string) => Promise<void>;
    setNewName: (value: string) => void;
    setDraftJson: (value: string) => void;
    /** Put the draft back into the box, discarding whatever was typed there. */
    refreshDraftJson: () => void;
    /** Read the box's text back into the draft; a refusal lands in `jsonError`. */
    applyDraftJson: () => void;
    createSet: () => Promise<void>;
    deleteSet: () => Promise<void>;
    save: () => Promise<void>;
    /** Write the current draft out as a predicate set file. */
    exportPredicateSet: () => Promise<void>;
    /** Read a predicate set file back, saving it under the name the file carries. */
    importPredicateSet: (text: string) => Promise<void>;
    addGroup: () => void;
    updateGroup: (index: number, patch: Partial<PredicateGroupDraft>) => void;
    removeGroup: (index: number) => void;
    addEntry: (target: PredicateEntryTarget) => void;
    updateEntry: (target: PredicateEntryTarget, index: number, patch: Partial<PredicateEntryDraft>) => void;
    removeEntry: (target: PredicateEntryTarget, index: number) => void;
    updateEntryCondition: (
        target: PredicateEntryTarget,
        index: number,
        patch: Partial<PredicateConditionDraft>,
    ) => void;
    setEntryAvailability: (target: PredicateEntryTarget, index: number, enabled: boolean) => void;
    updateEntryAvailability: (
        target: PredicateEntryTarget,
        index: number,
        patch: Partial<PredicateConditionDraft>,
    ) => void;
    addEffect: (target: PredicateEntryTarget, index: number) => void;
    updateEffect: (
        target: PredicateEntryTarget,
        entryIndex: number,
        effectIndex: number,
        patch: Partial<PredicateEffectDraft>,
    ) => void;
    removeEffect: (target: PredicateEntryTarget, entryIndex: number, effectIndex: number) => void;
    updatePreviewField: (index: number, patch: Partial<PredicatePreviewField>) => void;
    addPreviewField: () => void;
    removePreviewField: (index: number) => void;
    runPreview: () => Promise<void>;
    refreshBinding: () => Promise<void>;
    bindSelectedTo: (scope: StateBindingScope) => Promise<void>;
    dispose: () => void;
};
