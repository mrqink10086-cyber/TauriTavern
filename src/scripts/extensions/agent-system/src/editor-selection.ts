/**
 * The selection loop the state editors repeat.
 *
 * Declarations, machines and predicate sets are picked the same way: ask before
 * dropping unsaved edits, then load what the user picked. Only the wording of
 * the confirmation differs.
 */

export type EditorSelectionDeps = {
    selectedName: () => string;
    isDirty: () => boolean;
    isDisposed: () => boolean;
    confirmAction: (message: string) => Promise<boolean>;
    /** Report a failure that happened while asking. */
    reportError: (error: unknown) => void;
    discardMessage: (name: string) => string;
    load: (name: string) => Promise<void>;
};

export type EditorSelection = {
    confirmDiscard: (name: string) => Promise<boolean>;
    /** Whether the editor may close, asked only when edits are pending. */
    confirmPendingEdits: () => Promise<boolean>;
    select: (name: string) => Promise<void>;
};

export function createEditorSelection(deps: EditorSelectionDeps): EditorSelection {
    async function confirmDiscard(name: string): Promise<boolean> {
        if (!deps.isDirty()) {
            return true;
        }
        const confirmed = await deps.confirmAction(deps.discardMessage(name));
        return confirmed && !deps.isDisposed();
    }

    async function confirmPendingEdits(): Promise<boolean> {
        try {
            return await confirmDiscard(deps.selectedName());
        } catch (error) {
            deps.reportError(error);
            return false;
        }
    }

    async function select(name: string): Promise<void> {
        if (name === deps.selectedName()) {
            return;
        }
        let confirmed = false;
        try {
            confirmed = await confirmDiscard(deps.selectedName());
        } catch (error) {
            deps.reportError(error);
            return;
        }
        if (confirmed) {
            await deps.load(name);
        }
    }

    return { confirmDiscard, confirmPendingEdits, select };
}
