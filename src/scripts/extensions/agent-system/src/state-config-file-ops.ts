/**
 * Putting a file's contents into the draft.
 *
 * Both of these start with a file the user chose and end with the draft
 * changed, and both have to say why they refused rather than change it — which
 * is the part worth keeping in one place instead of inside a controller that
 * also owns saving, selection and the JSON box.
 */

import { errorText } from './host-api';
import {
    emptyDeclaration,
    isScriptModuleName,
    scriptModuleNameFromFile,
    TOKENIZER_FILE_PREFIX,
    type StateDeclaration,
} from './state-config-model';
import {
    addScriptModule,
    scriptModulesOf,
    setScriptModuleSource,
    updateDeclarationLimits,
} from './state-config-ops';
import type { Tr } from './AgentSystemPanelContract';

export type StateFileOpDeps = {
    /** The draft as it stands, or `null` when nothing is open. */
    currentDraft: () => StateDeclaration | null;
    /** The host's file dialog, or `null` when this window has none. */
    pickFilePath: ((extensions: readonly string[]) => Promise<string | null>) | null;
    applyDraft: (change: (draft: StateDeclaration) => StateDeclaration) => void;
    commit: (patch: { error?: string; notice?: string }) => void;
    isDisposed: () => boolean;
    tr: Tr;
};

export function createStateFileOps(deps: StateFileOpDeps): {
    importScriptModule: (fileName: string, source: string) => void;
    chooseVocabularyFile: () => Promise<void>;
} {
    return {
        /** A module named after the file it came from, filled in one edit. */
        importScriptModule(fileName, source) {
            const name = scriptModuleNameFromFile(fileName);
            if (!isScriptModuleName(name)) {
                deps.commit({ error: deps.tr('stateDeclarationScriptNameInvalid', { name }) });
                return;
            }
            const draft = deps.currentDraft() ?? emptyDeclaration();
            if (Object.prototype.hasOwnProperty.call(scriptModulesOf(draft), name)) {
                deps.commit({ error: deps.tr('stateDeclarationScriptExists', { name }) });
                return;
            }
            deps.applyDraft((current) => setScriptModuleSource(
                addScriptModule(current, name),
                name,
                source,
            ));
            deps.commit({ notice: deps.tr('stateDeclarationScriptImported', { name }) });
        },

        /**
         * Point the counting vocabulary at a file the user picks.
         *
         * The path is what the backend reads, so this asks the host for one;
         * a window without a dialog says so instead of failing silently.
         */
        async chooseVocabularyFile() {
            if (!deps.pickFilePath) {
                deps.commit({ error: deps.tr('stateLimitsTokenizerPickUnavailable') });
                return;
            }
            try {
                const path = await deps.pickFilePath(['json']);
                if (deps.isDisposed() || !path) {
                    return;
                }
                deps.applyDraft((draft) => updateDeclarationLimits(draft, {
                    tokenizer: `${TOKENIZER_FILE_PREFIX}${path}`,
                }));
            } catch (error) {
                if (deps.isDisposed()) {
                    return;
                }
                deps.commit({ error: errorText(error) });
            }
        },
    };
}
