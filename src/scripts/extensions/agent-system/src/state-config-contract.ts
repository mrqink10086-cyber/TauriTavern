/**
 * What the state declaration editor promises its host.
 *
 * The snapshot a panel renders, the calls it makes, and what the controller
 * offers back — the same shape the other first-party panels use, kept in its own
 * file so the controller itself stays a controller rather than growing a
 * contract at the top of it.
 */

import type { Tr } from './AgentSystemPanelContract';
import type {
    DeclaredStateField,
    StateConfigIssue,
    StateDeclaration,
    StateImageFit,
    StateLimits,
    StatePanelFieldSpec,
    StatePanelSpec,
} from './state-config-model';
import type { StateFieldAccessSpec } from './state-field-access';
import type { StateConditionEdit, StateImageTarget } from './state-config-ops';
import type { StateBindingScope, StateBindingView } from './state-binding';

export type StateExampleKind = 'scene' | 'seraphina';

export type StateConfigSnapshot = {
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    /** The last thing that went right, worth keeping in front of the user. */
    notice: string;
    names: string[];
    selectedName: string;
    /** The name a new declaration would be stored under, while it is typed. */
    newName: string;
    /** The module name a new shared script would be stored under, while typed. */
    newScriptName: string;
    draft: StateDeclaration | null;
    /** The draft as the whole document, which is what the JSON box edits. */
    draftJson: string;
    /** Why the box's text was not read, or `''` when there is nothing to say. */
    jsonError: string;
    /** The stored document the draft is compared against, for the dirty flag. */
    savedJson: string;
    issues: StateConfigIssue[];
    /**
     * Where the selected declaration is bound, or `null` when the page runtime
     * could not be read at all — the panel shows a hint instead of buttons.
     */
    binding: StateBindingView | null;
};

export type StateConfigControllerDeps = {
    listDeclarations: () => Promise<string[]>;
    getDeclaration: (name: string) => Promise<StateDeclaration>;
    saveDeclaration: (name: string, declaration: StateDeclaration) => Promise<void>;
    deleteDeclaration: (name: string) => Promise<void>;
    confirmAction: (message: string) => Promise<boolean>;
    notifyError: (error: unknown) => void;
    notifySuccess: (message: string) => void;
    /** Save a scene file through the host, so it lands where downloads land. */
    downloadBlob: (blob: Blob, fileName: string) => Promise<{ mode?: string; completed?: boolean } | undefined>;
    /**
     * Ask the host for an existing file and return its absolute path, or `null`
     * when this host has no file dialog at all — a plain browser, where a local
     * path would mean nothing to the backend anyway.
     */
    pickFilePath: ((extensions: readonly string[]) => Promise<string | null>) | null;
    /** Where the selected declaration is bound right now, or null if unreadable. */
    readBinding: () => Promise<StateBindingView | null>;
    /** Bind `name`, or unbind it when that target already binds it. */
    toggleBinding: (scope: StateBindingScope, name: string) => Promise<boolean>;
    tr: Tr;
};

export type StateConfigController = {
    getSnapshot: () => StateConfigSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    refresh: () => Promise<void>;
    /** Whether closing the editor may go ahead, asked only when edits are pending. */
    confirmPendingEdits: () => Promise<boolean>;
    selectDeclaration: (name: string) => Promise<void>;
    setNewName: (value: string) => void;
    setNewScriptName: (value: string) => void;
    createScriptModule: () => void;
    /** Add a module from a chosen file, named after that file. */
    importScriptModule: (fileName: string, source: string) => void;
    setScriptModuleSource: (name: string, source: string) => void;
    removeScriptModule: (name: string) => void;
    setThemeCss: (source: string) => void;
    setDraftJson: (value: string) => void;
    /** Put the draft back into the box, discarding whatever was typed there. */
    refreshDraftJson: () => void;
    /** Read the box's text back into the draft; a refusal lands in `jsonError`. */
    applyDraftJson: () => void;
    /**
     * Start a new declaration from one of the shipped examples.
     *
     * `scene` is the small generic one; `seraphina` is the worked example built
     * for the default character card.
     */
    createDeclaration: (example?: StateExampleKind) => Promise<void>;
    deleteDeclaration: () => Promise<void>;
    save: () => Promise<void>;
    /** Write the current draft out as a scene file. */
    exportDeclaration: () => Promise<void>;
    /** Read a scene file back, saving it under the name the file carries. */
    importDeclaration: (text: string) => Promise<void>;
    updateField: (index: number, patch: Partial<DeclaredStateField>) => void;
    updateFieldAccess: (index: number, patch: Partial<StateFieldAccessSpec>) => void;
    /** Set one of the scene's ceilings, its unit, or the vocabulary it counts with. */
    updateLimits: (patch: Partial<StateLimits>) => void;
    /** Ask the host for a vocabulary file and point the limits at it. */
    chooseVocabularyFile: () => Promise<void>;
    /** Set the values a field starts from, from one comma-separated line. */
    updateFieldInitial: (index: number, text: string) => void;
    addField: () => void;
    removeField: (index: number) => void;
    updatePanel: (panelIndex: number, patch: Partial<StatePanelSpec>) => void;
    /** Set or clear a panel's prose block (a path inside `persist/`). */
    updatePanelProse: (panelIndex: number, patch: { path?: string; title?: string }) => void;
    addPanel: () => void;
    removePanel: (panelIndex: number) => void;
    updatePanelField: (panelIndex: number, fieldIndex: number, patch: Partial<StatePanelFieldSpec>) => void;
    setPanelTemplate: (panelIndex: number, source: string) => void;
    addPanelField: (panelIndex: number) => void;
    removePanelField: (panelIndex: number, fieldIndex: number) => void;
    addImageCandidate: (target: StateImageTarget) => void;
    removeImageCandidate: (target: StateImageTarget, candidateIndex: number) => void;
    setImageCandidateSource: (target: StateImageTarget, candidateIndex: number, source: string) => void;
    setImageCandidateCondition: (
        target: StateImageTarget,
        candidateIndex: number,
        edit: StateConditionEdit,
    ) => void;
    clearImageCandidateCondition: (target: StateImageTarget, candidateIndex: number) => void;
    setImageFit: (target: StateImageTarget, fit: StateImageFit) => void;
    setImageConditionScript: (target: StateImageTarget, script: string) => void;
    clearImageConditionScript: (target: StateImageTarget) => void;
    refreshBinding: () => Promise<void>;
    bindSelectedTo: (scope: StateBindingScope) => Promise<void>;
    dispose: () => void;
};
