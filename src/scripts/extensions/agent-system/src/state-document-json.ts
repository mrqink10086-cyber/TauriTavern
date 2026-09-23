/**
 * The "whole document as JSON" box, over one editor's draft.
 *
 * All three editors whose tables grow a row at a time offer this box, so the
 * printing, the parsing and the refusal live here once. Applying is an edit like
 * any table edit: the editor hands in what else a new draft implies for it, and
 * gets the parsed draft back.
 */

import { errorText } from './host-api';
import type { Tr } from './AgentSystemPanelContract';

export type DocumentJsonSnapshot = {
    /** The draft as the whole document, which is what the box edits. */
    draftJson: string;
    /** Why the box's text was not read, or `''` when there is nothing to say. */
    jsonError: string;
};

/** Nothing loaded yet, and nothing to report about it. */
export const EMPTY_DOCUMENT_JSON: DocumentJsonSnapshot = Object.freeze({
    draftJson: '',
    jsonError: '',
});

export type DocumentJson<TDraft> = {
    /** The draft as text — also what the save stores, so the two cannot disagree. */
    print: (draft: TDraft) => string;
    /** The snapshot fields for a draft that was just loaded or created. */
    loaded: (draft: TDraft) => DocumentJsonSnapshot;
    set: (value: string) => void;
    refresh: () => void;
    apply: () => void;
};

type DocumentRead<TDraft> = { draft: TDraft; error: '' } | { draft: null; error: string };

/**
 * Read the box's text back into a draft.
 *
 * A refusal is a value rather than a throw: the box is edited by hand, and a
 * broken paste has to leave the draft exactly as it was.
 */
function readJsonDocument<TDraft, TDocument>(
    text: string,
    fromDocument: (value: TDocument) => TDraft,
    tr: Tr,
): DocumentRead<TDraft> {
    let parsed: unknown;
    try {
        parsed = JSON.parse(text);
    } catch (error) {
        return { draft: null, error: errorText(error) };
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        return { draft: null, error: tr('jsonDocumentMustBeObject') };
    }

    // The object check above is the whole of what a document has to be here; the
    // editor's own reader decides what its fields mean.
    return { draft: fromDocument(parsed as TDocument), error: '' };
}

export function createDocumentJson<TDraft, TDocument = TDraft>(options: {
    readDraft: () => TDraft | null;
    readText: () => string;
    writeSnapshot: (patch: Partial<DocumentJsonSnapshot>) => void;
    print: (draft: TDraft) => string;
    /** The stored document as the editor holds it (compiled parts printed back). */
    fromDocument: (value: TDocument) => TDraft;
    /** Everything else applying an edit means here — the issues, the notice. */
    applied: (draft: TDraft, patch: DocumentJsonSnapshot) => void;
    tr: Tr;
}): DocumentJson<TDraft> {
    const printed = (draft: TDraft): DocumentJsonSnapshot => ({
        draftJson: options.print(draft),
        jsonError: '',
    });

    return {
        print: options.print,
        loaded: printed,
        set(value: string): void {
            options.writeSnapshot({ draftJson: value, jsonError: '' });
        },
        refresh(): void {
            const draft = options.readDraft();
            if (draft) {
                options.writeSnapshot(printed(draft));
            }
        },
        apply(): void {
            if (!options.readDraft()) {
                return;
            }
            const read = readJsonDocument(options.readText(), options.fromDocument, options.tr);
            if (!read.draft) {
                options.writeSnapshot({ jsonError: read.error });
                return;
            }
            options.applied(read.draft, printed(read.draft));
        },
    };
}
