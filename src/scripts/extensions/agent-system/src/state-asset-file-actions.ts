/**
 * Taking a named asset out of this machine, and bringing one in.
 *
 * The three editors share this because they share the shape: a draft, a name it
 * is stored under, and a store that refuses rather than half-saves. What differs
 * is the file format and what accepting an import means for that asset, and both
 * arrive as parameters.
 */

import type { AgentSystemMessageKey } from './i18n';
import type { Tr } from './AgentSystemPanelContract';
import type { AssetFileErrorReason, AssetFileFormat } from './state-asset-file';

export type AssetFileMessages = {
    exported: AgentSystemMessageKey;
    imported: AgentSystemMessageKey;
    overwrite: AgentSystemMessageKey;
    /** How a draft a save would refuse is reported; needed only with `saveBlockers`. */
    blocked?: AgentSystemMessageKey;
    /** One sentence per way the file itself was unreadable. */
    errorKeys: Readonly<Record<AssetFileErrorReason, AgentSystemMessageKey>>;
};

export type AssetFileActionsDeps<TDraft> = {
    deps: {
        downloadBlob: (blob: Blob, fileName: string) => Promise<{ mode?: string; completed?: boolean } | undefined>;
        confirmAction: (message: string) => Promise<boolean>;
        notifyError: (error: unknown) => void;
    };
    format: AssetFileFormat<TDraft>;
    messages: AssetFileMessages;
    /** The draft to export, and the name it would be stored under. */
    currentDraft: () => { name: string; draft: TDraft | null };
    /** Names already stored, so an import can ask before replacing one. */
    storedNames: () => readonly string[];
    /** The name a nameless file should take, if the user has typed one. */
    fallbackName: () => string;
    isDisposed: () => boolean;
    commit: (patch: { error?: string; notice?: string; saving?: boolean }) => void;
    /** Save the import under `name`, then list and open it. */
    acceptImport: (name: string, draft: TDraft) => Promise<void>;
    /** What would stop this draft from being stored at all, when anything can. */
    saveBlockers?: (draft: TDraft) => string[];
    tr: Tr;
};

export function createAssetFileActions<TDraft>(
    deps: AssetFileActionsDeps<TDraft>,
): { exportAsset: () => Promise<void>; importAsset: (text: string) => Promise<void> } {
    async function exportAsset(): Promise<void> {
        const { name, draft } = deps.currentDraft();
        if (!draft || !name) {
            return;
        }
        try {
            const blob = new Blob([deps.format.print(name, draft)], { type: 'application/json' });
            await deps.deps.downloadBlob(blob, deps.format.fileName(name));
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ error: '', notice: deps.tr(deps.messages.exported, { name }) });
        } catch (error) {
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ error: String(error) });
            deps.deps.notifyError(error);
        }
    }

    async function importAsset(text: string): Promise<void> {
        const read = deps.format.read(text);
        if (!read.draft) {
            deps.commit({
                error: deps.tr(deps.messages.errorKeys[read.failure ?? 'not_a_package']),
                notice: '',
            });
            return;
        }

        // Refused before the user is asked to overwrite anything: a document that
        // cannot be stored should not cost them a decision.
        const blockers = deps.saveBlockers?.(read.draft) ?? [];
        if (blockers.length > 0 && deps.messages.blocked) {
            deps.commit({
                error: deps.tr(deps.messages.blocked, { detail: blockers.join('; ') }),
                notice: '',
            });
            return;
        }

        const name = read.name || deps.fallbackName();
        if (deps.storedNames().includes(name)) {
            const confirmed = await deps.deps.confirmAction(
                deps.tr(deps.messages.overwrite, { name }),
            );
            if (!confirmed || deps.isDisposed()) {
                return;
            }
        }

        deps.commit({ saving: true, error: '', notice: '' });
        try {
            await deps.acceptImport(name, read.draft);
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ notice: deps.tr(deps.messages.imported, { name }) });
        } catch (error) {
            if (deps.isDisposed()) {
                return;
            }
            deps.commit({ error: String(error) });
            deps.deps.notifyError(error);
        } finally {
            deps.commit({ saving: false });
        }
    }

    return { exportAsset, importAsset };
}
