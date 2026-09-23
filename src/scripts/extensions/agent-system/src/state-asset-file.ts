/**
 * A named asset as a file.
 *
 * A machine and a predicate set travel as documents, so sharing one is a file
 * rather than retyping it into the editor's forms. The wrapper says what the
 * file is: a bare document on disk would be indistinguishable from any other
 * JSON object with a `states` key, and an import would have to guess. A `kind`
 * plus a `version` lets it refuse a file it does not understand instead of
 * half-reading it.
 */

/** Why a file was not read; each one has its own sentence in the editor. */
export type AssetFileErrorReason =
    | 'invalid_json'
    | 'not_a_package'
    | 'unsupported_version'
    | 'no_document';

/**
 * What reading a file produced, in the shape the editor edits.
 *
 * One shape with a nullable draft rather than a union: the caller's question is
 * always "did I get the document, and if not why", and both halves are read at
 * the same place.
 */
export type AssetFileReadResult<TDraft> = {
    /** The draft, or `null` when the file could not be read. */
    draft: TDraft | null;
    /** The name the file carries, empty when it names nothing. */
    name: string;
    /** Why there is no draft, or `null` when there is one. */
    failure: AssetFileErrorReason | null;
};

export type AssetFileFormat<TDraft> = {
    print: (name: string, draft: TDraft) => string;
    read: (text: string) => AssetFileReadResult<TDraft>;
    /** The file name this asset would be written under. */
    fileName: (name: string) => string;
};

function asRecord(value: unknown): Record<string, unknown> | null {
    return typeof value === 'object' && value !== null && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : null;
}

/**
 * A file format for an asset the editor holds as a draft.
 *
 * What is stored and what is edited are not always the same shape — a machine
 * is a spec on disk and a row-per-line draft in the editor — so each direction
 * takes its own converter, and everything else is the wrapper around them.
 */
export function createDraftFileFormat<TDraft, TDocument>({
    kind,
    documentKey,
    fileSuffix,
    fallbackFileName,
    isDocument,
    toDraft,
    fromDraft,
}: {
    /** What the file says it is, so a foreign file can be refused by name. */
    kind: string;
    /** The key the document sits under inside the wrapper. */
    documentKey: string;
    /** Appended to the asset's name to make a file name. */
    fileSuffix: string;
    /** The file name used when the asset has no name yet. */
    fallbackFileName: string;
    /** Whether a parsed value is this kind of document at all. */
    isDocument: (value: Record<string, unknown>) => boolean;
    toDraft: (document: TDocument) => TDraft;
    fromDraft: (draft: TDraft) => TDocument;
}): AssetFileFormat<TDraft> {
    // A reader accepts anything at or below it and refuses the rest: a file from
    // the future may carry keys this build would drop on save, and dropping them
    // silently is how an import eats half a document.
    const version = 1;

    function failure(reason: AssetFileErrorReason): AssetFileReadResult<TDraft> {
        return { draft: null, name: '', failure: reason };
    }

    // Only the wrapper's shape is checked here; the document itself is checked
    // by the save the caller runs next, which is what a hand edit goes through.
    function readValue(parsed: unknown): AssetFileReadResult<TDraft> {
        const record = asRecord(parsed);
        if (!record || record.kind !== kind) {
            return failure('not_a_package');
        }

        const fileVersion = Number(record.version);
        if (!Number.isInteger(fileVersion) || fileVersion < 1 || fileVersion > version) {
            return failure('unsupported_version');
        }

        const document = asRecord(record[documentKey]);
        if (!document || !isDocument(document)) {
            return failure('no_document');
        }

        try {
            return {
                draft: toDraft(document as unknown as TDocument),
                name: typeof record.name === 'string' ? record.name.trim() : '',
                failure: null,
            };
        } catch {
            // A document that cannot become a draft is one this build cannot read,
            // and saying so beats letting the converter's own error escape.
            return failure('no_document');
        }
    }

    return {
        print(name, draft) {
            const payload = {
                kind,
                version,
                name: String(name ?? '').trim(),
                [documentKey]: fromDraft(draft),
            };
            return `${JSON.stringify(payload, null, 4)}\n`;
        },
        read(text) {
            let parsed: unknown;
            try {
                parsed = JSON.parse(text);
            } catch {
                return failure('invalid_json');
            }
            return readValue(parsed);
        },
        fileName(name) {
            const trimmed = String(name ?? '').trim();
            return trimmed ? `${trimmed}${fileSuffix}` : fallbackFileName;
        },
    };
}
