/**
 * The state declaration editor's controller.
 *
 * The draft is held here, not in the backend: the user edits a half-finished key
 * pattern all the time, and only the save call has to see a finished document.
 */

import { errorText, prettyJson } from './host-api';
import { createEditorSelection } from './editor-selection';
import { DEFAULT_DECLARATION_NAME } from './state-examples';
import { draftForExample } from './state-config-examples';
import { createBindingActions, ensureStateBinding } from './state-binding';
import {
    emptyDeclaration,
    isScriptModuleName,
    stateDeclarationIssues,
    stateDeclarationSaveBlockers,
    type StateDeclaration,
    type StateImageFit,
} from './state-config-model';
import { declarationDraftFromDocument, normalizeStateDeclarationForSave } from './state-declaration-normalize';
import { createDocumentJson, EMPTY_DOCUMENT_JSON } from './state-document-json';
import { createAssetFileActions } from './state-asset-file-actions';
import { createStateFileOps } from './state-config-file-ops';
import { SCENE_FILE_FORMAT } from './state-package';
import { stateConfigDraftIsDirty, sortedNames } from './state-config-status';
export { stateConfigBusy, stateConfigDraftIsDirty } from './state-config-status';
import {
    addDeclarationField,
    addPanel,
    addPanelField,
    addScriptModule,
    scriptModulesOf,
    setScriptModuleSource,
    setThemeCss,
    removeDeclarationField,
    removeScriptModule,
    removePanel,
    removePanelField,
    setPanelTemplate,
    updateDeclarationField,
    updateDeclarationFieldInitial,
    updateDeclarationFieldAccess,
    updateDeclarationLimits, updatePanel,
    updatePanelProse,
    updatePanelField,
    type StateConditionEdit,
} from './state-config-ops';
import {
    addImageCandidate,
    clearImageCandidateCondition,
    clearImageConditionScript,
    removeImageCandidate,
    setImageCandidateCondition,
    setImageCandidateSource,
    setImageConditionScript,
    setImageFit,
} from './state-config-image-ops';

import type {
    StateConfigController,
    StateConfigControllerDeps,
    StateConfigSnapshot,
    StateExampleKind,
} from './state-config-contract';

export type { StateConfigController, StateConfigControllerDeps, StateConfigSnapshot };
/** Mount-local owner of the state declaration editor. */
export function createStateConfigController(deps: StateConfigControllerDeps): StateConfigController {
    let snapshot: StateConfigSnapshot = {
        initialized: false,
        loading: false,
        saving: false,
        error: '',
        notice: '',
        names: [],
        selectedName: '',
        newName: DEFAULT_DECLARATION_NAME,
        newScriptName: '',
        draft: null,
        ...EMPTY_DOCUMENT_JSON,
        savedJson: '',
        issues: [],
        binding: null,
    };
    const listeners = new Set<() => void>();
    let disposed = false;

    function commit(patch: Partial<StateConfigSnapshot>): void {
        if (disposed) {
            return;
        }
        snapshot = { ...snapshot, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    /**
     * The whole-document box, so a three-hundred-field scene can be pasted in
     * instead of typed row by row.
     */
    const documentJson = createDocumentJson<StateDeclaration>({
        readDraft: () => snapshot.draft,
        readText: () => snapshot.draftJson,
        writeSnapshot: (patch) => commit(patch),
        print: (draft) => prettyJson(normalizeStateDeclarationForSave(draft)),
        fromDocument: declarationDraftFromDocument,
        applied: (draft, patch) => commit({
            draft,
            ...patch,
            issues: stateDeclarationIssues(draft),
            error: '',
            notice: deps.tr('jsonApplied'),
        }),
        tr: deps.tr,
    });

    /**
     * Apply an edit and re-derive what the editor shows about the draft.
     *
     * The JSON box follows the draft rather than lagging behind it: text typed
     * into the box is only ever applied by its own button, so a box that followed
     * an edit is not a box whose stale text can be re-applied over it.
     */
    function applyDraft(update: (draft: StateDeclaration) => StateDeclaration): void {
        const draft = snapshot.draft;
        if (!draft) {
            return;
        }
        const next = update(draft);
        commit({
            draft: next,
            ...documentJson.loaded(next),
            issues: stateDeclarationIssues(next),
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
        });
    }

    /** Discriminates overlapping loads: two quick clicks must not fight over the editor. */
    let loadToken = 0;

    async function loadDeclaration(name: string): Promise<void> {
        const token = ++loadToken;
        commit({ loading: true, error: '', notice: '' });
        try {
            const declaration = await deps.getDeclaration(name);
            if (disposed || token !== loadToken) {
                return;
            }
            const draft = declarationDraftFromDocument(declaration);
            commit({
                selectedName: name,
                draft,
                ...documentJson.loaded(draft),
                savedJson: documentJson.print(draft),
                issues: stateDeclarationIssues(draft),
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
        commit({ names: sortedNames(await deps.listDeclarations()) });
    }

    const bindingActions = createBindingActions({
        readBinding: deps.readBinding,
        toggleBinding: deps.toggleBinding,
        selectedName: () => snapshot.selectedName.trim(),
        isDisposed: () => disposed,
        commit,
    });

    async function refresh(): Promise<void> {
        commit({ loading: true, error: '' });
        try {
            await refreshNames();
            if (disposed) {
                return;
            }
            // A selection the backend no longer has would keep editing a ghost.
            // A name that was never stored is not a ghost: it is a declaration
            // the user has created but not saved, and it survives the refresh.
            if (
                snapshot.selectedName
                && snapshot.savedJson !== ''
                && !snapshot.names.includes(snapshot.selectedName)
            ) {
                clearSelection();
            }
            await bindingActions.refreshBinding();
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            commit({ loading: false });
        }
    }

    /**
     * Open the editor: load the list, then the first declaration.
     *
     * The tab calls this on every activation, so a second call only refreshes the
     * list — reloading the selected declaration would throw away unsaved edits.
     */
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
            await loadDeclaration(first);
        }
        commit({ initialized: true });
    }

    const selection = createEditorSelection({
        selectedName: () => snapshot.selectedName,
        isDirty: () => stateConfigDraftIsDirty(snapshot),
        isDisposed: () => disposed,
        confirmAction: deps.confirmAction,
        reportError: (error) => commit({ error: errorText(error) }),
        discardMessage: (name) => deps.tr('stateDeclarationDiscardConfirm', { name }),
        load: loadDeclaration,
    });

    async function createDeclaration(example: StateExampleKind = 'scene'): Promise<void> {
        const name = snapshot.newName.trim();
        if (!name) {
            return;
        }
        if (snapshot.names.includes(name)) {
            commit({ error: deps.tr('stateDeclarationExists', { name }) });
            return;
        }
        if (!(await selection.confirmDiscard(snapshot.selectedName))) {
            return;
        }
        const { draft, noticeKey } = draftForExample(example);
        commit({
            selectedName: name,
            newName: '',
            draft,
            ...documentJson.loaded(draft),
            // Nothing is stored under this name yet, so the draft starts dirty.
            savedJson: '',
            issues: stateDeclarationIssues(draft),
            error: '',
            notice: deps.tr(noticeKey),
        });
    }

    /**
     * What stops a document from being stored at all: a theme or a template is
     * compiled on the way out, and a broken one would store half a scene.
     */
    function saveBlockersOf(draft: StateDeclaration): string[] {
        return stateDeclarationSaveBlockers(draft).map((issue) => issue.message);
    }

    async function save(): Promise<void> {
        const draft = snapshot.draft;
        const name = snapshot.selectedName.trim();
        if (!draft || !name) {
            return;
        }
        const blockers = saveBlockersOf(draft);
        if (blockers.length > 0) {
            commit({
                error: deps.tr('stateDeclarationSaveBlocked', { detail: blockers.join('; ') }),
                notice: '',
            });
            return;
        }
        commit({ saving: true, error: '', notice: '' });
        try {
            const document = normalizeStateDeclarationForSave(draft);
            await deps.saveDeclaration(name, document);
            if (disposed) {
                return;
            }
            commit({
                draft: document,
                savedJson: prettyJson(document),
                issues: stateDeclarationIssues(document),
                notice: deps.tr('stateDeclarationSaved', { name }),
            });
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
            deps.notifyError(error);
            return;
        } finally {
            commit({ saving: false });
        }
        // The document is stored. A failure to refresh the list or the binding is
        // reported on its own rather than as a failed save, which would leave the
        // user believing their work was lost.
        try {
            await refreshNames();
            await ensureStateBinding('declaration', name);
        } catch (error) {
            if (disposed) {
                return;
            }
            commit({ error: errorText(error) });
        }
    }

    const sceneFile = createAssetFileActions<StateDeclaration>({
        deps,
        format: SCENE_FILE_FORMAT,
        messages: {
            exported: 'stateDeclarationExported',
            imported: 'stateDeclarationImported',
            overwrite: 'stateDeclarationImportOverwrite',
            blocked: 'stateDeclarationSaveBlocked',
            errorKeys: {
                invalid_json: 'stateDeclarationImport_invalid_json',
                not_a_package: 'stateDeclarationImport_not_a_package',
                unsupported_version: 'stateDeclarationImport_unsupported_version',
                no_document: 'stateDeclarationImport_no_declaration',
            },
        },
        currentDraft: () => ({ name: snapshot.selectedName.trim(), draft: snapshot.draft }),
        storedNames: () => snapshot.names,
        fallbackName: () => snapshot.newName.trim() || DEFAULT_DECLARATION_NAME,
        isDisposed: () => disposed,
        commit,
        saveBlockers: saveBlockersOf,
        acceptImport: async (name, declaration) => {
            // Compiled on the way in, exactly as a hand edit is on the way out.
            await deps.saveDeclaration(name, normalizeStateDeclarationForSave(declaration));
            await ensureStateBinding('declaration', name);
            await refreshNames();
            await loadDeclaration(name);
        },
        tr: deps.tr,
    });

    const fileOps = createStateFileOps({
        currentDraft: () => snapshot.draft,
        pickFilePath: deps.pickFilePath,
        applyDraft,
        commit,
        isDisposed: () => disposed,
        tr: deps.tr,
    });

    async function deleteDeclaration(): Promise<void> {
        const name = snapshot.selectedName.trim();
        if (!name) {
            return;
        }
        const isStored = snapshot.names.includes(name);
        const confirmed = await deps.confirmAction(deps.tr(
            isStored ? 'stateDeclarationDeleteConfirm' : 'stateDeclarationDiscardConfirm',
            { name },
        ));
        if (!confirmed || disposed) {
            return;
        }
        // A declaration that was never stored has nothing to delete, so the
        // delete is local: asking the backend would only report "not found".
        if (!isStored) {
            clearSelection();
            return;
        }

        commit({ saving: true, error: '', notice: '' });
        try {
            await deps.deleteDeclaration(name);
            if (disposed) {
                return;
            }
            clearSelection();
            await refreshNames();
            deps.notifySuccess(deps.tr('stateDeclarationDeleted', { name }));
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
        selectDeclaration: selection.select,
        setNewName(value: string): void {
            commit({ newName: value, error: '' });
        },
        setNewScriptName(value: string): void {
            commit({ newScriptName: value, error: '' });
        },
        createScriptModule(): void {
            const name = snapshot.newScriptName.trim();
            if (!name) {
                return;
            }
            if (!isScriptModuleName(name)) {
                commit({ error: deps.tr('stateDeclarationScriptNameInvalid', { name }) });
                return;
            }
            if (Object.prototype.hasOwnProperty.call(scriptModulesOf(snapshot.draft ?? emptyDeclaration()), name)) {
                commit({ error: deps.tr('stateDeclarationScriptExists', { name }) });
                return;
            }
            applyDraft((draft) => addScriptModule(draft, name));
            commit({ newScriptName: '' });
        },
        setScriptModuleSource(name: string, source: string): void {
            applyDraft((draft) => setScriptModuleSource(draft, name, source));
        },
        removeScriptModule(name: string): void {
            applyDraft((draft) => removeScriptModule(draft, name));
        },
        setThemeCss(source: string): void {
            applyDraft((draft) => setThemeCss(draft, source));
        },
        setDraftJson: documentJson.set,
        refreshDraftJson: documentJson.refresh,
        applyDraftJson: documentJson.apply,
        createDeclaration,
        deleteDeclaration,
        save,
        exportDeclaration: sceneFile.exportAsset,
        importDeclaration: sceneFile.importAsset,
        updateField(index, patch) {
            applyDraft((draft) => updateDeclarationField(draft, index, patch));
        },
        updateFieldAccess(index, patch) {
            applyDraft((draft) => updateDeclarationFieldAccess(draft, index, patch));
        },
        updateLimits: (patch) => applyDraft((draft) => updateDeclarationLimits(draft, patch)),
        importScriptModule: fileOps.importScriptModule,
        chooseVocabularyFile: fileOps.chooseVocabularyFile,
        updateFieldInitial(index, text) {
            applyDraft((draft) => updateDeclarationFieldInitial(draft, index, text));
        },
        addField() {
            applyDraft(addDeclarationField);
        },
        removeField(index) {
            applyDraft((draft) => removeDeclarationField(draft, index));
        },
        updatePanel(panelIndex, patch) {
            applyDraft((draft) => updatePanel(draft, panelIndex, patch));
        },
        updatePanelProse(panelIndex, patch) {
            applyDraft((draft) => updatePanelProse(draft, panelIndex, patch));
        },
        addPanel() {
            applyDraft(addPanel);
        },
        removePanel(panelIndex) {
            applyDraft((draft) => removePanel(draft, panelIndex));
        },
        updatePanelField(panelIndex, fieldIndex, patch) {
            applyDraft((draft) => updatePanelField(draft, panelIndex, fieldIndex, patch));
        },
        setPanelTemplate(panelIndex, source) {
            applyDraft((draft) => setPanelTemplate(draft, panelIndex, source));
        },
        addPanelField(panelIndex) {
            applyDraft((draft) => addPanelField(draft, panelIndex));
        },
        removePanelField(panelIndex, fieldIndex) {
            applyDraft((draft) => removePanelField(draft, panelIndex, fieldIndex));
        },
        addImageCandidate(target) {
            applyDraft((draft) => addImageCandidate(draft, target));
        },
        removeImageCandidate(target, candidateIndex) {
            applyDraft((draft) => removeImageCandidate(draft, target, candidateIndex));
        },
        setImageCandidateSource(target, candidateIndex, source) {
            applyDraft((draft) => setImageCandidateSource(draft, target, candidateIndex, source));
        },
        setImageFit(target, fit: StateImageFit) {
            applyDraft((draft) => setImageFit(draft, target, fit));
        },
        setImageCandidateCondition(target, candidateIndex, edit: StateConditionEdit) {
            applyDraft((draft) => setImageCandidateCondition(draft, target, candidateIndex, edit));
        },
        clearImageCandidateCondition(target, candidateIndex) {
            applyDraft((draft) => clearImageCandidateCondition(draft, target, candidateIndex));
        },
        setImageConditionScript(target, script) {
            applyDraft((draft) => setImageConditionScript(draft, target, script));
        },
        clearImageConditionScript(target) {
            applyDraft((draft) => clearImageConditionScript(draft, target));
        },
        refreshBinding: bindingActions.refreshBinding,
        bindSelectedTo: bindingActions.bindSelectedTo,
        dispose(): void {
            if (disposed) {
                return;
            }
            disposed = true;
            listeners.clear();
        },
    };
}
