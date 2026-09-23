/**
 * The predicate set editor's controller.
 *
 * Same shape as the declaration and machine editors': the draft lives here,
 * every edit replaces it with a normalized-on-save document, and the backend
 * owns every semantic rule. The preview runs the loaded draft through the
 * backend's evaluator against assumed field values — it never writes anything.
 */

import { errorText, prettyJson } from './host-api';
import { createDocumentJson, EMPTY_DOCUMENT_JSON } from './state-document-json';
import { createEditorSelection } from './editor-selection';
import { DEFAULT_PREDICATE_NAME, examplePredicateSet } from './state-examples';
import { splitIdCsv } from './state-machine-model';
import {
    emptyConditionDraft,
    emptyPredicateEntry,
    normalizePredicateSetForSave,
    predicateDraftFromSet,
    predicateDraftIssues,
    withEntries,
    type PredicateEntryDraft,
    type PredicateEntryTarget,
    type PredicateSetDraft,
    type StatePredicateSet,
} from './state-predicate-model';
import { createAssetFileActions } from './state-asset-file-actions';
import { PREDICATE_FILE_FORMAT } from './state-asset-file-formats';
import { ensureStateBinding, type StateBindingScope } from './state-binding';

export type {
    PredicateConfigController,
    PredicateConfigControllerDeps,
    PredicateConfigSnapshot,
    PredicatePreviewField,
} from './state-predicate-contract';

import type {
    PredicateConfigController,
    PredicateConfigControllerDeps,
    PredicateConfigSnapshot,
} from './state-predicate-contract';

export function predicateConfigBusy(snapshot: PredicateConfigSnapshot): boolean {
    return snapshot.loading || snapshot.saving || snapshot.previewing;
}

/** Whether the draft differs from what is stored (same normalization as the save). */
export function predicateDraftIsDirty(snapshot: PredicateConfigSnapshot): boolean {
    if (!snapshot.draft) {
        return false;
    }
    return prettyJson(normalizePredicateSetForSave(snapshot.draft)) !== snapshot.savedJson;
}

function sortedNames(names: readonly string[]): string[] {
    return [...names].map((name) => String(name)).sort((left, right) => left.localeCompare(right));
}

/** Mount-local owner of the predicate set editor. */
export function createPredicateConfigController(deps: PredicateConfigControllerDeps): PredicateConfigController {
    let snapshot: PredicateConfigSnapshot = {
        initialized: false,
        loading: false,
        saving: false,
        error: '',
        notice: '',
        names: [],
        selectedName: '',
        newName: DEFAULT_PREDICATE_NAME,
        draft: null,
        ...EMPTY_DOCUMENT_JSON,
        savedJson: '',
        issues: [],
        previewFields: [],
        previewing: false,
        preview: null,
        binding: null,
    };
    const listeners = new Set<() => void>();
    let disposed = false;

    function commit(patch: Partial<PredicateConfigSnapshot>): void {
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
    function applyDraft(update: (draft: PredicateSetDraft) => PredicateSetDraft): void {
        const draft = snapshot.draft;
        if (!draft) {
            return;
        }
        const next = update(draft);
        commit({
            draft: next,
            ...documentJson.loaded(next),
            issues: predicateDraftIssues(next),
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
            previewFields: [],
        });
    }

    const documentJson = createDocumentJson<PredicateSetDraft, StatePredicateSet>({
        readDraft: () => snapshot.draft,
        readText: () => snapshot.draftJson,
        writeSnapshot: (patch) => commit(patch),
        print: (draft) => prettyJson(normalizePredicateSetForSave(draft)),
        fromDocument: predicateDraftFromSet,
        applied: (draft, patch) => commit({
            draft,
            ...patch,
            issues: predicateDraftIssues(draft),
            error: '',
            notice: deps.tr('jsonApplied'),
        }),
        tr: deps.tr,
    });

    /** Discriminates overlapping loads: two quick clicks must not fight over the editor. */
    let loadToken = 0;

    async function loadSet(name: string): Promise<void> {
        const token = ++loadToken;
        commit({ loading: true, error: '', notice: '' });
        try {
            const set = await deps.getSet(name);
            if (disposed || token !== loadToken) {
                return;
            }
            const draft = predicateDraftFromSet(set);
            commit({
                selectedName: name,
                draft,
                ...documentJson.loaded(draft),
                savedJson: documentJson.print(draft),
                issues: predicateDraftIssues(draft),
                preview: null,
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
        commit({ names: sortedNames(await deps.listSets()) });
    }

    const predicateFile = createAssetFileActions<PredicateSetDraft>({
        deps,
        format: PREDICATE_FILE_FORMAT,
        messages: {
            exported: 'predicateExported',
            imported: 'predicateImported',
            overwrite: 'predicateImportOverwrite',
            errorKeys: {
                invalid_json: 'predicateImport_invalid_json',
                not_a_package: 'predicateImport_not_a_package',
                unsupported_version: 'predicateImport_unsupported_version',
                no_document: 'predicateImport_no_document',
            },
        },
        currentDraft: () => ({ name: snapshot.selectedName.trim(), draft: snapshot.draft }),
        storedNames: () => snapshot.names,
        fallbackName: () => snapshot.newName.trim() || DEFAULT_PREDICATE_NAME,
        isDisposed: () => disposed,
        commit,
        acceptImport: async (name, draft) => {
            await deps.saveSet(name, normalizePredicateSetForSave(draft));
            await ensureStateBinding('predicates', name);
            await refreshNames();
            await loadSet(name);
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
            // A name that was never stored is not a ghost: it is a set the user
            // has created but not saved, and it survives the refresh.
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

    /** Open the editor: load the list, then the first set. Tab-activation safe. */
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
            await loadSet(first);
        }
        commit({ initialized: true });
    }

    const selection = createEditorSelection({
        selectedName: () => snapshot.selectedName,
        isDirty: () => predicateDraftIsDirty(snapshot),
        isDisposed: () => disposed,
        confirmAction: deps.confirmAction,
        reportError: (error) => commit({ error: errorText(error) }),
        discardMessage: (name) => deps.tr('predicateDiscardConfirm', { name }),
        load: loadSet,
    });

    async function createSet(): Promise<void> {
        const name = snapshot.newName.trim();
        if (!name) {
            return;
        }
        if (snapshot.names.includes(name)) {
            commit({ error: deps.tr('predicateExists', { name }) });
            return;
        }
        if (!(await selection.confirmDiscard(snapshot.selectedName))) {
            return;
        }
        const draft = predicateDraftFromSet(examplePredicateSet());
        commit({
            selectedName: name,
            newName: '',
            draft,
            ...documentJson.loaded(draft),
            // Nothing is stored under this name yet, so the draft starts dirty.
            savedJson: '',
            issues: predicateDraftIssues(draft),
            error: '',
            notice: deps.tr('predicateExampleLoaded'),
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
            const set = normalizePredicateSetForSave(draft);
            await deps.saveSet(name, set);
            if (disposed) {
                return;
            }
            commit({
                draft: predicateDraftFromSet(set),
                savedJson: prettyJson(set),
                issues: predicateDraftIssues(predicateDraftFromSet(set)),
                notice: deps.tr('predicateSaved', { name }),
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

    async function deleteSet(): Promise<void> {
        const name = snapshot.selectedName.trim();
        if (!name) {
            return;
        }
        const isStored = snapshot.names.includes(name);
        const confirmed = await deps.confirmAction(deps.tr(
            isStored ? 'predicateDeleteConfirm' : 'predicateDiscardConfirm',
            { name },
        ));
        if (!confirmed || disposed) {
            return;
        }
        // A set that was never stored has nothing to delete, so the delete is
        // local: asking the backend would only report "not found".
        if (!isStored) {
            clearSelection();
            return;
        }
        commit({ saving: true, error: '', notice: '' });
        try {
            await deps.deleteSet(name);
            if (disposed) {
                return;
            }
            clearSelection();
            await refreshNames();
            deps.notifySuccess(deps.tr('predicateDeleted', { name }));
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
            const fields: Record<string, string[]> = {};
            for (const field of snapshot.previewFields) {
                const key = field.keyText.trim();
                if (key) {
                    fields[key] = splitIdCsv(field.valuesText);
                }
            }
            const evaluation = await deps.evaluate({
                set: normalizePredicateSetForSave(draft),
                fields,
            });
            if (disposed) {
                return;
            }
            commit({ preview: evaluation });
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
     * Read where the selected set is bound.
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
     * Bind the selected set to one target, or unbind it when that target already
     * binds it.
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

    function mapEntry(
        target: PredicateEntryTarget,
        entryIndex: number,
        update: (entry: PredicateEntryDraft) => PredicateEntryDraft,
    ): void {
        applyDraft((draft) => withEntries(draft, target, (entries) => entries.map(
            (entry, at) => (at === entryIndex ? update(entry) : entry),
        )));
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
        selectSet: selection.select,
        setNewName(value: string): void {
            commit({ newName: value, error: '' });
        },
        setDraftJson: documentJson.set,
        refreshDraftJson: documentJson.refresh,
        applyDraftJson: documentJson.apply,
        createSet,
        deleteSet,
        save,
        addGroup() {
            applyDraft((draft) => ({
                ...draft,
                groups: [...draft.groups, { id: '', labelText: '', entries: [] }],
            }));
        },
        updateGroup(index, patch) {
            applyDraft((draft) => ({
                ...draft,
                groups: draft.groups.map((group, at) => (at === index ? { ...group, ...patch } : group)),
            }));
        },
        removeGroup(index) {
            applyDraft((draft) => ({ ...draft, groups: draft.groups.filter((_, at) => at !== index) }));
        },
        addEntry(target) {
            applyDraft((draft) => withEntries(draft, target, (entries) => [...entries, emptyPredicateEntry()]));
        },
        updateEntry(target, index, patch) {
            mapEntry(target, index, (entry) => ({ ...entry, ...patch }));
        },
        removeEntry(target, index) {
            applyDraft((draft) => withEntries(draft, target, (entries) => entries.filter((_, at) => at !== index)));
        },
        updateEntryCondition(target, index, patch) {
            mapEntry(target, index, (entry) => ({
                ...entry,
                condition: { ...(entry.condition ?? emptyConditionDraft()), ...patch },
            }));
        },
        setEntryAvailability(target, index, enabled) {
            mapEntry(target, index, (entry) => ({
                ...entry,
                availability: enabled ? (entry.availability ?? emptyConditionDraft()) : null,
            }));
        },
        updateEntryAvailability(target, index, patch) {
            mapEntry(target, index, (entry) => {
                if (!entry.availability) {
                    return entry;
                }
                return { ...entry, availability: { ...entry.availability, ...patch } };
            });
        },
        addEffect(target, index) {
            mapEntry(target, index, (entry) => ({
                ...entry,
                effects: [...entry.effects, { kind: 'inhibit', tagsText: '' }],
            }));
        },
        updateEffect(target, entryIndex, effectIndex, patch) {
            mapEntry(target, entryIndex, (entry) => ({
                ...entry,
                effects: entry.effects.map((effect, at) => (at === effectIndex ? { ...effect, ...patch } : effect)),
            }));
        },
        removeEffect(target, entryIndex, effectIndex) {
            mapEntry(target, entryIndex, (entry) => ({
                ...entry,
                effects: entry.effects.filter((_, at) => at !== effectIndex),
            }));
        },
        updatePreviewField(index, patch) {
            commit({
                previewFields: snapshot.previewFields.map(
                    (field, at) => (at === index ? { ...field, ...patch } : field),
                ),
            });
        },
        addPreviewField() {
            commit({ previewFields: [...snapshot.previewFields, { keyText: '', valuesText: '' }] });
        },
        removePreviewField(index) {
            commit({ previewFields: snapshot.previewFields.filter((_, at) => at !== index) });
        },
        exportPredicateSet: predicateFile.exportAsset,
        importPredicateSet: predicateFile.importAsset,
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
