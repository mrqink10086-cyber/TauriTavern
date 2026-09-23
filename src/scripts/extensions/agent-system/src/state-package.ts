/**
 * A scene declaration as a file.
 *
 * A scene is not just its field list: the panels, the HTML template, the theme
 * sheet, the shared scripts and the rules that move the stages all live in the
 * same document. Exporting that document is exporting the scene, so a scene can
 * be handed to someone else, kept next to a character card, or written by hand
 * in an editor — rather than retyped into these forms field by field.
 *
 * The wrapper exists so a file says what it is. A bare declaration on disk would
 * be indistinguishable from any other JSON object with a `fields` key, and an
 * import would have to guess; a `kind` plus a `version` lets it refuse a file it
 * does not understand instead of half-reading it.
 */

import type { AssetFileFormat } from './state-asset-file';
import type { StateDeclaration } from './state-config-model';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';

export const STATE_PACKAGE_KIND = 'tauritavern.state-declaration';

/**
 * The format this build writes.
 *
 * A reader accepts anything at or below it and refuses the rest: a file from the
 * future may carry keys this build would drop on save, and dropping them
 * silently is how an import eats half a scene.
 */
export const STATE_PACKAGE_VERSION = 1;

export type StatePackage = {
    kind: typeof STATE_PACKAGE_KIND;
    version: number;
    name: string;
    declaration: StateDeclaration;
};

/** Why a file was not read; each one has its own sentence in the editor. */
export type StatePackageErrorReason =
    | 'invalid_json'
    | 'not_a_package'
    | 'unsupported_version'
    | 'no_declaration';

/**
 * What reading a file produced.
 *
 * One shape with a nullable declaration rather than a union: the caller's
 * question is always "did I get a scene, and if not why", and both halves are
 * read at the same place.
 */
export type StatePackageReadResult = {
    /** The scene, or `null` when the file could not be read. */
    declaration: StateDeclaration | null;
    /** The name the file carries, empty when it names nothing. */
    name: string;
    /** Why there is no declaration, or `null` when there is one. */
    failure: StatePackageErrorReason | null;
};

export function toStatePackage(name: string, declaration: StateDeclaration): StatePackage {
    return {
        kind: STATE_PACKAGE_KIND,
        version: STATE_PACKAGE_VERSION,
        name: String(name ?? '').trim(),
        declaration,
    };
}

export function printStatePackage(name: string, declaration: StateDeclaration): string {
    return `${JSON.stringify(toStatePackage(name, declaration), null, 4)}\n`;
}

/** `scene.scene.json` for `scene`; the host's fallback when the name is empty. */
export function statePackageFileName(name: string): string {
    const trimmed = String(name ?? '').trim();
    return trimmed ? `${trimmed}.scene.json` : 'scene.json';
}

function failure(failure: StatePackageErrorReason): StatePackageReadResult {
    return { declaration: null, name: '', failure };
}

function asRecord(value: unknown): Record<string, unknown> | null {
    return typeof value === 'object' && value !== null && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : null;
}

/**
 * Read a file back into a name and a declaration.
 *
 * Only the shape of the wrapper is checked here; the declaration itself is
 * checked by the save the caller runs next, which is the same validation a hand
 * edit goes through. Refusing early is for the file, not for the scene.
 */
export function readStatePackage(text: string): StatePackageReadResult {
    let parsed: unknown;
    try {
        parsed = JSON.parse(text);
    } catch {
        return failure('invalid_json');
    }

    return readStatePackageValue(parsed);
}

/**
 * The same read for a package that is already parsed.
 *
 * A scene carried inside a character card arrives as a value, not as a file, and
 * the wrapper is what says it is a scene. Both callers go through one check, so a
 * card cannot smuggle in a document the file path would have refused.
 */
export function readStatePackageValue(parsed: unknown): StatePackageReadResult {
    const record = asRecord(parsed);
    if (!record || record.kind !== STATE_PACKAGE_KIND) {
        return failure('not_a_package');
    }

    const version = Number(record.version);
    if (!Number.isInteger(version) || version < 1 || version > STATE_PACKAGE_VERSION) {
        return failure('unsupported_version');
    }

    const declaration = asRecord(record.declaration);
    if (!declaration || !Array.isArray(declaration.fields)) {
        return failure('no_declaration');
    }

    return {
        declaration: declaration as unknown as StateDeclaration,
        name: typeof record.name === 'string' ? record.name.trim() : '',
        failure: null,
    };
}

/**
 * The scene as the editor's shared file format.
 *
 * A scene was the first asset to travel as a file, so its wrapper predates the
 * shared one; this adapts it rather than restating it, which keeps a scene file
 * already on someone's disk readable.
 */
export const SCENE_FILE_FORMAT: AssetFileFormat<StateDeclaration> = {
    print: (name, declaration) => printStatePackage(name, normalizeStateDeclarationForSave(declaration)),
    read(text) {
        const read = readStatePackage(text);
        return {
            draft: read.declaration,
            name: read.name,
            // Two words for the same failure: this file's wrapper says
            // "declaration" where the shared one says "document".
            failure: read.failure === 'no_declaration' ? 'no_document' : read.failure,
        };
    },
    fileName: statePackageFileName,
};
