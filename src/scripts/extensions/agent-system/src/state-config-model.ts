/**
 * The state declaration editor's model.
 *
 * These types mirror `tt-domain/models/state.rs`, `state_access.rs` and
 * `state_panel.rs`. They are deliberately a copy rather than a generated
 * binding: the editor needs a draft shape it can hold half-finished (an empty
 * pattern while the user is typing), while the Rust side only ever sees a
 * finished document.
 *
 * What the backend owns and this file does not repeat: the condition `op`
 * registry, the anchored matching rules, and whether a candidate can actually
 * be selected. The editor's own checks are the cheap shape ones — an image path
 * that is not a host path, an empty panel title — so the user does not have to
 * round-trip to the backend to learn that a field is blank. Everything else is
 * answered by the save call, which refuses with a coded message.
 */

import { compileTemplate, type CompiledTemplate } from './state-template';
import { compileThemeCss } from './state-theme-css';
import type { StateFieldAccessSpec } from './state-field-access';

export type StateConditionSpec = {
    /** Left out on a combination: the parts carry the comparisons. */
    source?: string | null;
    field?: string | null;
    op?: string | null;
    value?: string | null;
    values?: string[];
    compose?: StateConditionCompose | null;
};

/**
 * Several conditions combined into one.
 *
 * The editor does not rewrite a combination: it is authored as JSON and holds
 * parts this form does not render, so it is passed through unchanged and left to
 * the backend's shape check (which refuses a node that also compares a field).
 */
export type StateConditionCompose =
    | { all: StateConditionSpec[] }
    | { any: StateConditionSpec[] }
    | { not: StateConditionSpec };

/** One inline script module: the picture set's own condition script. */
export type StateScriptSpec = {
    script: string;
    entry?: string | null;
};

export type StateImageCandidate = {
    source: string;
    when?: StateConditionSpec | null;
};

/**
 * How a picture fills the box it is given.
 *
 * The two jobs want opposite answers and the set says which one it is doing:
 * `cover` fills edge to edge and crops what falls outside, which is what a
 * scene picture wants, and `contain` shows the picture whole, which is what a
 * portrait wants — a person cropped to the panel's aspect shows a shoulder
 * where a face belongs.
 */
export type StateImageFit = 'cover' | 'contain';

export const STATE_IMAGE_FITS: readonly StateImageFit[] = ['cover', 'contain'];

export type StateImageSet = {
    candidates: StateImageCandidate[];
    /** Absent means `cover`, which is what every set written before this said. */
    fit?: StateImageFit;
    /**
     * A script that decides which candidate applies.
     *
     * Absent is the default and the common case: the candidates' conditions
     * decide. When one is written it **replaces** those conditions, so the
     * editor says so next to the field.
     */
    conditionScript?: StateScriptSpec | null;
};

export type StateFieldRender = 'text' | 'image';

export type StatePanelFieldSpec = {
    pattern: string;
    label?: string | null;
    render?: StateFieldRender;
    images?: StateImageSet | null;
};

export type StatePanelRail = 'left' | 'right';

/**
 * A block of prose a panel shows, kept as a file rather than a field.
 *
 * The path is inside the run's persistent content (`persist/...`), so the block
 * travels with the floor through the same version chain the state does. The
 * file is written by the model with the workspace tools — the editor only says
 * where to look.
 */
export type StateProseSpec = {
    path: string;
    title?: string | null;
};

export type StatePanelSpec = {
    title: string;
    rail?: StatePanelRail;
    match: string;
    prose?: StateProseSpec | null;
    background?: StateImageSet | null;
    fields?: StatePanelFieldSpec[];
    /**
     * The panel's template **as text**, which is what the editor edits.
     *
     * A stored document carries only the compiled tree, so this key exists in
     * drafts: it is printed from the tree when a document is opened, and it is
     * compiled back when one is stored. `''` means "no template", which is how
     * clearing the box is expressed.
     */
    templateSource?: string | null;
    /**
     * The compiled tree, as it came from a document. Never edited by hand.
     *
     * Named for what it is: elements and bindings, not something anybody
     * evaluates later. The editor's text lives in `templateSource`.
     */
    markup?: CompiledTemplate | null;
};

export type DeclaredStateField = {
    pattern: string;
    label: string;
    /**
     * What this field grants unless a Profile's access rows override it. See
     * `state-field-access.ts` for the switches and their defaults.
     */
    access?: StateFieldAccessSpec;
    /**
     * What the field holds before anything writes it.
     *
     * A chat has no state until something writes one, so without this the model
     * has to write every field before the story can start — and a panel that
     * reads a field nobody wrote shows nothing. Only a literal key can carry
     * them; the backend refuses a pattern, because there is no key to write to.
     */
    initial?: string[];
};

/** The modules a picture set's script may import, keyed by flat module name. */
export type StateScriptModules = Record<string, string>;

/**
 * What a scene counts one value in.
 *
 * A character is deterministic, free, and the same on every machine; a token is
 * what the ceiling is actually about. The same 512 characters is roughly 105
 * tokens of English and 320 to 640 of Chinese, so the unit decides what the
 * number beside it means.
 */
export type StateCostUnit = 'chars' | 'tokens';

/**
 * What a scene lets a value cost.
 *
 * `0` on any ceiling means the scene does not limit it. The tokenizer is only
 * read when the unit is `tokens`: empty follows whatever model the chat runs, a
 * shipped family's name pins one, and `file:<path>` counts with a tokenizer the
 * user supplied.
 */
export type StateLimits = {
    value?: number;
    valuesPerField?: number;
    fieldsPerUpdate?: number;
    unit?: StateCostUnit;
    tokenizer?: string;
};

/**
 * What a scene that says nothing allows.
 *
 * The numbers are the ones the engine used to hard-code, so a document written
 * before the field existed means exactly what it meant then.
 */
export const DEFAULT_STATE_LIMITS: Readonly<Required<StateLimits>> = Object.freeze({
    value: 512,
    valuesPerField: 32,
    fieldsPerUpdate: 128,
    unit: 'chars',
    tokenizer: '',
});

/** One scene's limits, with the defaults filled in for the editor's benefit. */
export function stateLimitsOf(declaration: StateDeclaration): Required<StateLimits> {
    return { ...DEFAULT_STATE_LIMITS, ...(declaration.limits ?? {}) };
}

export type StateDeclaration = {
    fields: DeclaredStateField[];
    /**
     * What this scene lets a value cost, and in what unit.
     *
     * Absent means the scene says nothing and reads as the defaults.
     */
    limits?: StateLimits;
    panels?: {
        panels: StatePanelSpec[];
        /** One logic, several picture sets: see the domain's panel config. */
        scripts?: StateScriptModules | null;
        /**
         * The theme stylesheet, **as the user wrote it**.
         *
         * This is the source, not what the panel applies: the sheet is compiled
         * (validated and scoped to the panel root) when the document is stored,
         * and the compiled text is what a panel reads back.
         */
        css?: string | null;
    };
    /**
     * This scene's stages, exactly as the machine editor writes them.
     *
     * Opaque here on purpose: a scene carries its rules, but they are written in
     * their own tab (or arrive in an imported file), so this editor passes them
     * through instead of re-modelling a spec it does not render.
     */
    machine?: unknown;
    /** This scene's conditional text, passed through the same way. */
    predicates?: unknown;
};

/**
 * The module-name rule the backend enforces, so the editor can refuse earlier.
 *
 * A flat `name.js`: that is exactly what `./name.js` resolves to from an entry
 * script, and a path would make an import mean something else in every set.
 */
export function isScriptModuleName(name: string): boolean {
    const match = /^([A-Za-z0-9_-]+)\.js$/u.exec(String(name ?? '').trim());
    return Boolean(match && (match[1]?.length ?? 0) > 0 && (match[1]?.length ?? 0) <= 48);
}

/**
 * The module name a chosen file would be stored under: its base name.
 *
 * A file carries whatever name the folder gave it, but a module needs the one
 * entry scripts import it by, which is `./pick.js` in every set regardless of
 * the directory the file was picked from.
 */
export function scriptModuleNameFromFile(fileName: string): string {
    return String(fileName ?? '').split(/[\\/]/u).pop()?.trim() ?? '';
}

/**
 * How a vocabulary of the user's own is named in `limits.tokenizer`.
 *
 * The backend strips this prefix and treats the rest as a path, so the two
 * sides have to agree on it; it lives here rather than in the editor that
 * happens to render the picker.
 */
export const TOKENIZER_FILE_PREFIX = 'file:';

/** The condition ops the built-in registry answers, for the editor's pickers. */
export const STATE_CONDITION_OPS: readonly string[] = Object.freeze([
    'eq',
    'ne',
    'in',
    'not_in',
    'contains',
    'not_contains',
    'gt',
    'gte',
    'lt',
    'lte',
    'exists',
    'missing',
    'matches',
]);

/** The ops that compare against a list of values instead of a single one. */
export const STATE_SET_CONDITION_OPS: readonly string[] = Object.freeze(['in', 'not_in']);

export function isSetConditionOp(op: string): boolean {
    return STATE_SET_CONDITION_OPS.includes(op);
}

/**
 * What the editor shows for a condition's comparison side.
 *
 * A list comparison is edited as one comma-separated line, so the draft holds
 * text either way and only the save normalization has to know which shape the
 * op needs. A condition loaded from disk may carry `values` instead of `value`,
 * which is the same line, joined.
 */
export function conditionValueText(condition: StateConditionSpec | null | undefined): string {
    const values = condition?.values ?? [];
    if (values.length > 0) {
        return values.join(', ');
    }
    return String(condition?.value ?? '');
}

export function emptyDeclaration(): StateDeclaration {
    return { fields: [], panels: { panels: [] } };
}

export function emptyImageSet(): StateImageSet {
    return { candidates: [{ source: '' }] };
}

/**
 * A picture source has to be a path the host serves.
 *
 * Same rule as the domain: never a scheme, never a traversal, and no character
 * that could end the path early once it is inside `url(...)` or an attribute.
 */
export function isHostImageSource(source: string): boolean {
    const value = String(source ?? '').trim();
    if (value.length <= 1 || !value.startsWith('/') || value.startsWith('//')) {
        return false;
    }
    return !value.includes('..')
        && !value.includes(':')
        && !value.includes('\\')
        && !/[\s"'()]/u.test(value);
}

export type StateConfigIssue = {
    /** Where the problem is, for the editor to point at. */
    path: string;
    message: string;
    /**
     * Whether the problem stops the document from being built at all.
     *
     * A theme or a template is compiled on the way to storage, so a problem
     * there is not "worth fixing later": there is no document to store. Those
     * are the issues a save refuses over (see `stateDeclarationSaveBlockers`).
     */
    blocking?: boolean;
};

/**
 * Split a comma-separated row of key patterns.
 *
 * Commas inside a `/pattern/flags` escape hatch belong to the pattern — a regex
 * such as `/(\d{1,2})/` must not be cut in half — while the `/` of an ordinary
 * path (`环境/日期`) is just a separator. Only a `/` that opens a token starts a
 * regex, which is exactly the grammar the declaration uses.
 */
export function splitKeyPatterns(text: string): string[] {
    const patterns: string[] = [];
    let current = '';
    let inRegex = false;
    let escaped = false;

    for (const character of String(text ?? '')) {
        if (escaped) {
            current += character;
            escaped = false;
            continue;
        }
        if (character === '\\') {
            current += character;
            escaped = true;
            continue;
        }
        if (character === '/' && (inRegex || current.trim().length === 0)) {
            inRegex = !inRegex;
            current += character;
            continue;
        }
        if (character === ',' && !inRegex) {
            patterns.push(current);
            current = '';
            continue;
        }
        current += character;
    }

    patterns.push(current);
    return patterns.map((pattern) => pattern.trim()).filter((pattern) => pattern.length > 0);
}

/**
 * Shape problems worth showing before the save round trip.
 *
 * The list is intentionally short: it catches what a half-finished form looks
 * like, and leaves every semantic rule — overlapping panels, unreachable
 * candidates, unknown ops — to the backend, which owns them.
 */
export function stateDeclarationIssues(declaration: StateDeclaration): StateConfigIssue[] {
    const issues: StateConfigIssue[] = [];

    declaration.fields.forEach((field, index) => {
        if (!field.pattern.trim()) {
            issues.push({ path: `fields[${index}].pattern`, message: 'a field needs a key pattern' });
        }
    });

    for (const panel of declaration.panels?.panels ?? []) {
        if (!panel.title.trim()) {
            issues.push({ path: 'panels', message: 'a panel needs a title' });
        }
        if (!panel.match.trim()) {
            issues.push({ path: 'panels', message: `panel \`${panel.title}\` needs a key pattern to match` });
        }
        collectImageIssues(panel.background, `panel \`${panel.title}\` background`, issues);
        for (const field of panel.fields ?? []) {
            collectImageIssues(field.images, `panel \`${panel.title}\` field \`${field.pattern}\``, issues);
        }
        // A template is compiled on the way to storage, so what is wrong with it
        // now is what would stop the save: reported here, marked as blocking.
        const where = `panel \`${panel.title}\` template`;
        issues.push(...compileTemplate(panel.templateSource ?? '', where).issues
            .map((issue) => ({ ...issue, blocking: true })));
    }

    issues.push(...compileThemeCss(declaration.panels?.css ?? '')
        .issues.map((issue) => ({ ...issue, blocking: true })));

    return issues;
}

/**
 * The problems that make a document impossible to build.
 *
 * The theme and each template are compiled while the document is normalized, so
 * an issue there is not something to fix later — there is nothing to store. The
 * save refuses over exactly this list, and the editor shows it in full.
 */
export function stateDeclarationSaveBlockers(declaration: StateDeclaration): StateConfigIssue[] {
    return stateDeclarationIssues(declaration).filter((issue) => issue.blocking === true);
}

function collectImageIssues(
    images: StateImageSet | null | undefined,
    where: string,
    issues: StateConfigIssue[],
): void {
    for (const candidate of images?.candidates ?? []) {
        if (!isHostImageSource(candidate.source)) {
            issues.push({
                path: where,
                message: `\`${candidate.source}\` must be a host path such as /backgrounds/name.png`,
            });
        }
    }
}

