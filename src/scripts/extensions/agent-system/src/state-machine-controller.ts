/**
 * The state machine editor's controller.
 *
 * Same shape as the declaration editor's: the draft lives here, every edit
 * replaces it with a normalized-on-save document, and the backend owns every
 * semantic rule. The preview runs the loaded draft through the backend's
 * evaluator against assumed inputs — it never writes anything.
 */

import { errorText, prettyJson } from './host-api';
import { createDocumentJson, EMPTY_DOCUMENT_JSON } from './state-document-json';
import { createEditorSelection } from './editor-selection';
import { DEFAULT_MACHINE_NAME, exampleMachine } from './state-examples';
import {
    machineDraftFromSpec,
    machineDraftIssues,
    normalizeMachineForSave,
    splitIdCsv,
    type MachineActionDraft,
    type MachineConditionDraft,
    type MachineDraft,
    type MachineSpec,
    type MachineTransitionDraft,
} from './state-machine-model';
import { createAssetFileActions } from './state-asset-file-actions';
import { MACHINE_FILE_FORMAT } from './state-asset-file-formats';
import { ensureStateBinding, type StateBindingScope } from './state-binding';

export type {
    MachineConfigController,
    MachineConfigControllerDeps,
    MachineConfigSnapshot,
    MachinePreviewField,
} from './state-machine-contract';

import type {
    MachineConfigController,
    MachineConfigControllerDeps,
    MachineConfigSnapshot,
} from './state-machine-contract';

export function machineConfigBusy(snapshot: MachineConfigSnapshot): boolean {
    return snapshot.loading || snapshot.saving || snapshot.previewing;
}

/** Whether the draft differs from what is stored (same normalization as the save). */
export function machineDraftIsDirty(snapshot: MachineConfigSnapshot): boolean {
    if (!snapshot.draft) {
        return false;
    }
    return prettyJson(normalizeMachineForSave(snapshot.draft)) !== snapshot.savedJson;
}

function sortedNames(names: readonly string[]): string[] {
    return [...names].map((name) => String(name)).sort((left, right) => left.localeCompare(right));
}

/** Mount-local owner of the state machine editor. */
export function createMachineConfigController(deps: MachineConfigControllerDeps): MachineConfigController {
    let snapshot: MachineConfigSnapshot = {
        initialized: false,
        loading: false,
        saving: false,
        error: '',
        notice: '',
        names: [],
        selectedName: '',
        newName: DEFAULT_MACHINE_NAME,
        draft: null,
        ...EMPTY_DOCUMENT_JSON,
        savedJson: '',
        issues: [],
        previewActiveText: '',
        previewFields: [],
        previewing: false,
        preview: null,
        binding: null,
    };
    const listeners = new Set<() => void>();
    let disposed = false;

    function commit(patch: Partial<MachineConfigSnapshot>): void {
        if (disposed) {
            return;
        }
        snapshot = { ...snapshot, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    /**
     * Apply an edit and re-derive what the editor shows about the draft.
     *
     * The JSON box follows the draft rather than lagging behind it: text typed
     * into the box is only ever applied by its own button, so a box that followed
     * an edit is not a box whose stale text can be re-applied over it.
     */
    function applyDraft(update: (draft: MachineDraft) => MachineDraft): void {
        const draft = snapshot.draft;
        if (!draft) {
            return;
        }
        const next = update(draft);
        commit({
            draft: next,
            ...documentJson.loaded(next),
            issues: machineDraftIssues(next),
            error: '',
            notice: '',
        });
    }

    function clearSelection(): void {
        commit({
            selectedName: '',
            draft: null,
            ...EMPTY_DOCUMENT_JSON,
            savedJson: '',
            issues: [],
            error: '',
            notice: '',
            preview: null,
            previewActiveText: '',
            previewFields: [],
        });
    }

    /** The whole-document box, so a long stage list can be pasted instead of typed. */
    const documentJson = createDocumentJson<MachineDraft, MachineSpec>({
        readDraft: () => snapshot.draft,
        readText: () => snapshot.draftJson,
        writeSnapshot: (patch) => commit(patch),
        print: (draft) => prettyJson(normalizeMachineForSave(draft)),
        fromDocument: machineDraftFromSpec,
        applied: (draft, patch) => commit({
            draft,
            ...patch,
            issues: machineDraftIssues(draft),
            error: '',
            notice: deps.tr('jsonApplied'),
        }),
        tr: deps.tr,
    });

    /** Discriminates overlapping loads: two quick clicks must not fight over the editor. */
    let loadToken = 0;

    async function loadMachine(name: string): Promise<void> {
        const token = ++loadToken;
        commit({ loading: true, error: '', notice: '' });
        try {
            const spec = await deps.getMachine(name);
            if (disposed || token !== loadToken) {
                return;
            }
            const draft = machineDraftFromSpec(spec);
            commit({
                selectedName: name,
                draft,
                ...documentJson.loaded(draft),
                savedJson: documentJson.print(draft),
                issues: machineDraftIssues(draft),
                previewActiveText: (spec.initial ?? []).join(', '),
            });
        } catch (error) {
            if (disposed || token !== loadToken) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            if (token === loadToken) {
                commit({ loading: false });
            }
        }
    }

    async function refreshNames(): Promise<void> {
        commit({ names: sortedNames(await deps.listMachines()) });
    }

    const machineFile = createAssetFileActions<MachineDraft>({
        deps,
        format: MACHINE_FILE_FORMAT,
        messages: {
            exported: 'machineExported',
            imported: 'machineImported',
            overwrite: 'machineImportOverwrite',
            errorKeys: {
                invalid_json: 'machineImport_invalid_json',
                not_a_package: 'machineImport_not_a_package',
                unsupported_version: 'machineImport_unsupported_version',
                no_document: 'machineImport_no_document',
            },
        },
        currentDraft: () => ({ name: snapshot.selectedName.trim(), draft: snapshot.draft }),
        storedNames: () => snapshot.names,
        fallbackName: () => snapshot.newName.trim() || DEFAULT_MACHINE_NAME,
        isDisposed: () => disposed,
        commit,
        acceptImport: async (name, machine) => {
            await deps.saveMachine(name, normalizeMachineForSave(machine));
            await ensureStateBinding('machine', name);
            await refreshNames();
            await loadMachine(name);
        },
        tr: deps.tr,
    });

    async function refresh(): Promise<void> {
        commit({ loading: true, error: '' });
        try {
            await refreshNames();
            if (disposed) {
                return;
            }
            // A selection the backend no longer has would keep editing a ghost.
            // A name that was never stored is not a ghost: it is a machine the
            // user has created but not saved, and it survives the refresh.
            if (
                snapshot.selectedName
                && snapshot.savedJson !== ''
                && !snapshot.names.includes(snapshot.selectedName)
            ) {
                clearSelection();
            }
            await refreshBinding();
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            commit({ loading: false });
        }
    }

    /** Open the editor: load the list, then the first machine. Tab-activation safe. */
    async function init(): Promise<void> {
        if (snapshot.initialized) {
            await refresh();
            return;
        }
        await refresh();
        if (disposed) {
            return;
        }
        const [first] = snapshot.names;
        if (first) {
            await loadMachine(first);
        }
        commit({ initialized: true });
    }

    const selection = createEditorSelection({
        selectedName: () => snapshot.selectedName,
        isDirty: () => machineDraftIsDirty(snapshot),
        isDisposed: () => disposed,
        confirmAction: deps.confirmAction,
        reportError: (error) => commit({ error: errorText(error) }),
        discardMessage: (name) => deps.tr('machineDiscardConfirm', { name }),
        load: loadMachine,
    });

    function mapTransitions(update: (list: MachineTransitionDraft[]) => MachineTransitionDraft[]): void {
        applyDraft((draft) => ({ ...draft, transitions: update(draft.transitions ?? []) }));
    }

    function mapConditions(transitionIndex: number, update: (list: MachineConditionDraft[]) => MachineConditionDraft[]): void {
        mapTransitions((list) => list.map((transition, at) => (
            at !== transitionIndex ? transition : { ...transition, conditions: update(transition.conditions ?? []) }
        )));
    }

    function mapActions(transitionIndex: number, update: (list: MachineActionDraft[]) => MachineActionDraft[]): void {
        mapTransitions((list) => list.map((transition, at) => (
            at !== transitionIndex ? transition : { ...transition, actions: update(transition.actions ?? []) }
        )));
    }


    async function createMachine(): Promise<void> {
        const name = snapshot.newName.trim();
        if (!name) {
            return;
        }
        if (snapshot.names.includes(name)) {
            commit({ error: deps.tr('machineExists', { name }) });
            return;
        }
        if (!(await selection.confirmDiscard(snapshot.selectedName))) {
            return;
        }
        const draft = machineDraftFromSpec(exampleMachine());
        commit({
            selectedName: name,
            newName: '',
            draft,
            ...documentJson.loaded(draft),
            // Nothing is stored under this name yet, so the draft starts dirty.
            savedJson: '',
            issues: machineDraftIssues(draft),
            error: '',
            notice: deps.tr('machineExampleLoaded'),
            previewActiveText: (draft.initialText || '').trim(),
            preview: null,
        });
    }

    async function save(): Promise<void> {
        const draft = snapshot.draft;
        const name = snapshot.selectedName.trim();
        if (!draft || !name) {
            return;
        }
        commit({ saving: true, error: '', notice: '' });
        try {
            const machine = normalizeMachineForSave(draft);
            await deps.saveMachine(name, machine);
            if (disposed) {
                return;
            }
            commit({
                draft: machineDraftFromSpec(machine),
                savedJson: prettyJson(machine),
                notice: deps.tr('machineSaved', { name }),
            });
            await refreshNames();
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
            deps.notifyError(error);
        } finally {
            commit({ saving: false });
        }
    }

    async function deleteMachine(): Promise<void> {
        const name = snapshot.selectedName.trim();
        if (!name) {
            return;
        }
        const isStored = snapshot.names.includes(name);
        const confirmed = await deps.confirmAction(deps.tr(
            isStored ? 'machineDeleteConfirm' : 'machineDiscardConfirm',
            { name },
        ));
        if (!confirmed || disposed) {
            return;
        }
        if (!isStored) {
            clearSelection();
            return;
        }
        commit({ saving: true, error: '', notice: '' });
        try {
            await deps.deleteMachine(name);
            if (disposed) {
                return;
            }
            clearSelection();
            await refreshNames();
            deps.notifySuccess(deps.tr('machineDeleted', { name }));
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
            deps.notifyError(error);
        } finally {
            commit({ saving: false });
        }
    }

    async function runPreview(): Promise<void> {
        const draft = snapshot.draft;
        if (!draft) {
            return;
        }
        commit({ previewing: true, error: '', notice: '' });
        try {
            const machine = normalizeMachineForSave(draft);
            const fields: Record<string, string[]> = {};
            for (const field of snapshot.previewFields) {
                const key = field.keyText.trim();
                if (key) {
                    fields[key] = splitIdCsv(field.valuesText);
                }
            }
            const run = await deps.evaluate({
                machine,
                active: splitIdCsv(snapshot.previewActiveText),
                fields,
            });
            if (disposed) {
                return;
            }
            commit({ preview: run });
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
            deps.notifyError(error);
        } finally {
            commit({ previewing: false });
        }
    }

    /**
     * Read where the selected machine is bound.
     *
     * Called with every refresh, because the answer changes with the chat the
     * user has open and the character they are talking to — not with this panel.
     * A runtime that cannot be read is `null`, and the panel says so instead of
     * offering buttons that could not work.
     */
    async function refreshBinding(): Promise<void> {
        try {
            const binding = await deps.readBinding();
            if (disposed) {
                return;
            }
            commit({ binding });
        } catch {
            if (disposed) {
                return;
            }
            commit({ binding: null });
        }
    }

    /**
     * Bind the selected machine to one target, or unbind it when that target
     * already binds it.
     *
     * The outcome is not reported here: `power-user.js` owns the user-facing
     * notification, and the refresh below is what tells the panel what actually
     * happened — a failed toggle must not leave it claiming a binding.
     */
    async function bindSelectedTo(scope: StateBindingScope): Promise<void> {
        const name = snapshot.selectedName.trim();
        if (!name) {
            return;
        }
        await deps.toggleBinding(scope, name);
        await refreshBinding();
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
            };
        },
        init,
        refresh,
        confirmPendingEdits: selection.confirmPendingEdits,
        selectMachine: selection.select,
        setNewName(value: string): void {
            commit({ newName: value, error: '' });
        },
        setDraftJson: documentJson.set,
        refreshDraftJson: documentJson.refresh,
        applyDraftJson: documentJson.apply,
        createMachine,
        deleteMachine,
        save,
        updateInitial(value: string): void {
            applyDraft((draft) => ({ ...draft, initialText: value }));
        },
        updateState(index, patch) {
            applyDraft((draft) => ({
                ...draft,
                states: draft.states.map((state, position) => (position === index ? { ...state, ...patch } : state)),
            }));
        },
        addState() {
            applyDraft((draft) => ({
                ...draft,
                states: [...draft.states, { id: '', labelText: '', terminal: false }],
            }));
        },
        removeState(index) {
            applyDraft((draft) => ({
                ...draft,
                states: draft.states.filter((_, position) => position !== index),
            }));
        },
        updateTransition(index, patch) {
            mapTransitions((list) => list.map((t, at) => (at === index ? { ...t, ...patch } : t)));
        },
        addTransition() {
            mapTransitions((list) => [...list, {
                fromText: '', toText: '', priorityText: '', conditions: [], actions: [],
            }]);
        },
        removeTransition(index) {
            mapTransitions((list) => list.filter((_, at) => at !== index));
        },
        updateCondition(transitionIndex, conditionIndex, patch) {
            mapConditions(transitionIndex, (list) => list.map((c, at) => (at === conditionIndex ? { ...c, ...patch } : c)));
        },
        addCondition(transitionIndex) {
            mapConditions(transitionIndex, (list) => [...list, {
                sourceText: 'field', fieldText: '', op: 'eq', valueText: '',
            }]);
        },
        removeCondition(transitionIndex, conditionIndex) {
            mapConditions(transitionIndex, (list) => list.filter((_, at) => at !== conditionIndex));
        },
        updateAction(transitionIndex, actionIndex, patch) {
            mapActions(transitionIndex, (list) => list.map((a, at) => (at === actionIndex ? { ...a, ...patch } : a)));
        },
        addAction(transitionIndex) {
            mapActions(transitionIndex, (list) => [...list, { kind: 'setField', targetText: '', valuesText: '' }]);
        },
        removeAction(transitionIndex, actionIndex) {
            mapActions(transitionIndex, (list) => list.filter((_, at) => at !== actionIndex));
        },
        setPreviewActive(value: string): void {
            commit({ previewActiveText: value });
        },
        updatePreviewField(index, patch) {
            commit({
                previewFields: snapshot.previewFields.map(
                    (field, position) => (position === index ? { ...field, ...patch } : field),
                ),
            });
        },
        addPreviewField() {
            commit({ previewFields: [...snapshot.previewFields, { keyText: '', valuesText: '' }] });
        },
        removePreviewField(index) {
            commit({ previewFields: snapshot.previewFields.filter((_, position) => position !== index) });
        },
        exportMachine: machineFile.exportAsset,
        importMachine: machineFile.importAsset,
        runPreview,
        refreshBinding,
        bindSelectedTo,
        dispose(): void {
            if (disposed) {
                return;
            }
            disposed = true;
            listeners.clear();
        },
    };
}
