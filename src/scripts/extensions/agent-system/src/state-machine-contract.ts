/**
 * What the state machine editor promises its host.
 *
 * The same split the declaration editor uses: the snapshot a panel renders, the
 * calls it makes, and what the controller offers back live here, kept out of the
 * controller so it stays a controller rather than growing a contract at the top
 * of it.
 */

import type { Tr } from './AgentSystemPanelContract';
import type { StateBindingScope, StateBindingView } from './state-binding';
import type {
    MachineActionDraft,
    MachineConditionDraft,
    MachineConfigIssue,
    MachineDraft,
    MachineRunDto,
    MachineSpec,
    MachineStateDraft,
    MachineTransitionDraft,
} from './state-machine-model';

export type MachinePreviewField = { keyText: string; valuesText: string };

export type MachineConfigSnapshot = {
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    notice: string;
    names: string[];
    selectedName: string;
    newName: string;
    draft: MachineDraft | null;
    /** The draft as the whole document, which is what the JSON box edits. */
    draftJson: string;
    /** Why the box's text was not read, or `''` when there is nothing to say. */
    jsonError: string;
    savedJson: string;
    issues: MachineConfigIssue[];
    previewActiveText: string;
    previewFields: MachinePreviewField[];
    previewing: boolean;
    preview: MachineRunDto | null;
    /**
     * Where the selected machine is bound, or `null` when the page runtime could
     * not be read at all — the panel shows a hint instead of buttons.
     */
    binding: StateBindingView | null;
};

export type MachineConfigControllerDeps = {
    listMachines: () => Promise<string[]>;
    getMachine: (name: string) => Promise<MachineSpec>;
    saveMachine: (name: string, machine: MachineSpec) => Promise<void>;
    deleteMachine: (name: string) => Promise<void>;
    evaluate: (input: { machine: MachineSpec; active?: string[]; fields?: Record<string, string[]> }) => Promise<MachineRunDto>;
    confirmAction: (message: string) => Promise<boolean>;
    notifyError: (error: unknown) => void;
    notifySuccess: (message: string) => void;
    /** Save a machine file through the host, so it lands where downloads land. */
    downloadBlob: (blob: Blob, fileName: string) => Promise<{ mode?: string; completed?: boolean } | undefined>;
    /** Where the selected machine is bound right now, or null if unreadable. */
    readBinding: () => Promise<StateBindingView | null>;
    /** Bind `name`, or unbind it when that target already binds it. */
    toggleBinding: (scope: StateBindingScope, name: string) => Promise<boolean>;
    tr: Tr;
};

export type MachineConfigController = {
    getSnapshot: () => MachineConfigSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    refresh: () => Promise<void>;
    /** Whether closing the editor may go ahead, asked only when edits are pending. */
    confirmPendingEdits: () => Promise<boolean>;
    selectMachine: (name: string) => Promise<void>;
    setNewName: (value: string) => void;
    setDraftJson: (value: string) => void;
    /** Put the draft back into the box, discarding whatever was typed there. */
    refreshDraftJson: () => void;
    /** Read the box's text back into the draft; a refusal lands in `jsonError`. */
    applyDraftJson: () => void;
    createMachine: () => Promise<void>;
    deleteMachine: () => Promise<void>;
    save: () => Promise<void>;
    /** Write the current draft out as a machine file. */
    exportMachine: () => Promise<void>;
    /** Read a machine file back, saving it under the name the file carries. */
    importMachine: (text: string) => Promise<void>;
    updateInitial: (value: string) => void;
    updateState: (index: number, patch: Partial<MachineStateDraft>) => void;
    addState: () => void;
    removeState: (index: number) => void;
    updateTransition: (index: number, patch: Partial<MachineTransitionDraft>) => void;
    addTransition: () => void;
    removeTransition: (index: number) => void;
    updateCondition: (transitionIndex: number, conditionIndex: number, patch: Partial<MachineConditionDraft>) => void;
    addCondition: (transitionIndex: number) => void;
    removeCondition: (transitionIndex: number, conditionIndex: number) => void;
    updateAction: (transitionIndex: number, actionIndex: number, patch: Partial<MachineActionDraft>) => void;
    addAction: (transitionIndex: number) => void;
    removeAction: (transitionIndex: number, actionIndex: number) => void;
    setPreviewActive: (value: string) => void;
    updatePreviewField: (index: number, patch: Partial<MachinePreviewField>) => void;
    addPreviewField: () => void;
    removePreviewField: (index: number) => void;
    runPreview: () => Promise<void>;
    refreshBinding: () => Promise<void>;
    bindSelectedTo: (scope: StateBindingScope) => Promise<void>;
    dispose: () => void;
};
