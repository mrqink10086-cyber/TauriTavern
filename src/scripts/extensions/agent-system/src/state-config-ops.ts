/**
 * Editing operations on a state declaration draft.
 *
 * Every function returns a new document; the controller commits that value so
 * React re-renders. Keeping the edits here rather than in the controller means
 * the rules — what a blank row is, what clearing a condition means — stay readable.
 */

import { stateLimitsOf } from './state-config-model';
import type {
    DeclaredStateField,
    StateDeclaration,
    StateLimits,
    StatePanelFieldSpec,
    StatePanelSpec,
    StateScriptModules,
} from './state-config-model';
import { DEFAULT_FIELD_ACCESS, fieldAccessOf, type StateFieldAccessSpec } from './state-field-access';

/** Which picture set an edit targets: a panel's background or one of its fields. */
export type StateImageTarget =
    | { kind: 'background'; panelIndex: number }
    | { kind: 'field'; panelIndex: number; fieldIndex: number };

/** A condition as the editor holds it: the comparison is still text. */
export type StateConditionEdit = {
    field: string;
    op: string;
    valueText: string;
};

function panelsOf(declaration: StateDeclaration): StatePanelSpec[] {
    return declaration.panels?.panels ?? [];
}

/**
 * The panel config as an editable copy.
 *
 * Every edit starts from what the document already carries, so a theme or a
 * shared module survives an edit that was about something else — which is the
 * whole reason the config is copied in one place rather than rebuilt from one
 * field at a time.
 */
function panelConfigOf(declaration: StateDeclaration): NonNullable<StateDeclaration['panels']> {
    const config = declaration.panels ?? { panels: [] };
    return { ...config, panels: [...(config.panels ?? [])] };
}

/** Rebuild the panel config around a new list, keeping what it also carries. */
function withPanels(declaration: StateDeclaration, panels: StatePanelSpec[]): StateDeclaration {
    return { ...declaration, panels: { ...panelConfigOf(declaration), panels } };
}

export function scriptModulesOf(declaration: StateDeclaration): StateScriptModules {
    return declaration.panels?.scripts ?? {};
}

function withScriptModules(
    declaration: StateDeclaration,
    scripts: StateScriptModules,
): StateDeclaration {
    return { ...declaration, panels: { ...panelConfigOf(declaration), scripts } };
}

/**
 * Set the theme stylesheet, as the user wrote it.
 *
 * The draft holds the source: compiling it belongs to the save, so the box keeps
 * showing the text that was typed rather than a normalized copy of it.
 */
export function setThemeCss(declaration: StateDeclaration, source: string): StateDeclaration {
    return { ...declaration, panels: { ...panelConfigOf(declaration), css: source } };
}

/**
 * Add a shared module with no source yet.
 *
 * The name is checked by the caller (the editor refuses one that could never be
 * imported, and one that already exists): adding an empty row is what "I am
 * about to write this module" looks like, and the save normalization drops it
 * if the user never does.
 */
export function addScriptModule(declaration: StateDeclaration, name: string): StateDeclaration {
    return withScriptModules(declaration, { ...scriptModulesOf(declaration), [name]: '' });
}

export function setScriptModuleSource(
    declaration: StateDeclaration,
    name: string,
    source: string,
): StateDeclaration {
    return withScriptModules(declaration, { ...scriptModulesOf(declaration), [name]: source });
}

export function removeScriptModule(declaration: StateDeclaration, name: string): StateDeclaration {
    const rest: StateScriptModules = { ...scriptModulesOf(declaration) };
    delete rest[name];
    return withScriptModules(declaration, rest);
}

function replacePanel(
    declaration: StateDeclaration,
    panelIndex: number,
    panel: StatePanelSpec,
): StateDeclaration {
    return withPanels(declaration, panelsOf(declaration).map((item, index) => (
        index === panelIndex ? panel : item
    )));
}

export function updatePanelAt(
    declaration: StateDeclaration,
    panelIndex: number,
    update: (panel: StatePanelSpec) => StatePanelSpec,
): StateDeclaration {
    const panel = panelsOf(declaration).find((_, index) => index === panelIndex);
    if (!panel) {
        return declaration;
    }
    return replacePanel(declaration, panelIndex, update(panel));
}

export function updateDeclarationField(
    declaration: StateDeclaration,
    index: number,
    patch: Partial<DeclaredStateField>,
): StateDeclaration {
    return {
        ...declaration,
        fields: declaration.fields.map((field, position) => (
            position === index ? { ...field, ...patch } : field
        )),
    };
}

/**
 * Set one of a field's switches.
 *
 * The row holds the whole spec once any switch is touched: what the editor shows
 * is what gets saved, so a later change to the defaults cannot silently move a
 * field the user already decided about.
 */
export function updateDeclarationFieldAccess(
    declaration: StateDeclaration,
    index: number,
    patch: Partial<StateFieldAccessSpec>,
): StateDeclaration {
    return {
        ...declaration,
        fields: declaration.fields.map((field, position) => (
            position === index
                ? { ...field, access: { ...fieldAccessOf(field), ...patch } }
                : field
        )),
    };
}

/**
 * Set one of a scene's ceilings, or its unit.
 *
 * The whole limits object is written once anything is touched, for the same
 * reason a field's switches are: what the editor shows is what gets saved, so a
 * later change to the defaults cannot re-measure a scene that already decided.
 */
export function updateDeclarationLimits(
    declaration: StateDeclaration,
    patch: Partial<StateLimits>,
): StateDeclaration {
    return { ...declaration, limits: { ...stateLimitsOf(declaration), ...patch } };
}

/**
 * Set a field's initial values from one comma-separated line.
 *
 * The line is the editor's shape, not the stored one: an empty line means the
 * field starts out holding nothing, which is written as no `initial` key at all
 * rather than an empty list.
 */
export function updateDeclarationFieldInitial(
    declaration: StateDeclaration,
    index: number,
    text: string,
): StateDeclaration {
    const values = String(text ?? '')
        .split(',')
        .map((value) => value.trim())
        .filter((value) => value.length > 0);

    return {
        ...declaration,
        fields: declaration.fields.map((field, position) => {
            if (position !== index) {
                return field;
            }
            if (values.length === 0) {
                const cleared = { ...field };
                delete cleared.initial;
                return cleared;
            }
            return { ...field, initial: values };
        }),
    };
}

/**
 * Set or clear a panel's prose block.
 *
 * A blank path clears the whole block: the heading on its own says nothing, and
 * the backend refuses a pathless one.
 */
export function updatePanelProse(
    declaration: StateDeclaration,
    panelIndex: number,
    patch: { path?: string; title?: string },
): StateDeclaration {
    const panel = declaration.panels?.panels?.[panelIndex];
    if (!panel) {
        return declaration;
    }
    const path = String(patch.path ?? panel.prose?.path ?? '');
    const title = String(patch.title ?? panel.prose?.title ?? '');
    return updatePanel(
        declaration,
        panelIndex,
        path.trim() ? { prose: { path, title } } : { prose: null },
    );
}

/** A new row is blank on purpose: it is a row the user has not filled in yet. */
export function addDeclarationField(declaration: StateDeclaration): StateDeclaration {
    return {
        ...declaration,
        fields: [...declaration.fields, { pattern: '', label: '', access: { ...DEFAULT_FIELD_ACCESS } }],
    };
}

export function removeDeclarationField(declaration: StateDeclaration, index: number): StateDeclaration {
    return {
        ...declaration,
        fields: declaration.fields.filter((_, position) => position !== index),
    };
}

export function updatePanel(
    declaration: StateDeclaration,
    panelIndex: number,
    patch: Partial<StatePanelSpec>,
): StateDeclaration {
    return updatePanelAt(declaration, panelIndex, (panel) => ({ ...panel, ...patch }));
}

export function addPanel(declaration: StateDeclaration): StateDeclaration {
    return withPanels(declaration, [
        ...panelsOf(declaration),
        { title: '', rail: 'left', match: '' },
    ]);
}

export function removePanel(declaration: StateDeclaration, panelIndex: number): StateDeclaration {
    return withPanels(declaration, panelsOf(declaration).filter((_, index) => index !== panelIndex));
}

/**
 * Set a panel's template, as the user wrote it.
 *
 * The draft holds the text; the tree is built when the document is stored, so
 * the box keeps showing what was typed.
 */
export function setPanelTemplate(
    declaration: StateDeclaration,
    panelIndex: number,
    source: string,
): StateDeclaration {
    return updatePanel(declaration, panelIndex, { templateSource: String(source ?? '') });
}

export function updatePanelField(
    declaration: StateDeclaration,
    panelIndex: number,
    fieldIndex: number,
    patch: Partial<StatePanelFieldSpec>,
): StateDeclaration {
    return updatePanelAt(declaration, panelIndex, (panel) => ({
        ...panel,
        fields: (panel.fields ?? []).map((field, index) => (
            index === fieldIndex ? { ...field, ...patch } : field
        )),
    }));
}

export function addPanelField(declaration: StateDeclaration, panelIndex: number): StateDeclaration {
    return updatePanelAt(declaration, panelIndex, (panel) => ({
        ...panel,
        fields: [...(panel.fields ?? []), { pattern: '', render: 'text' }],
    }));
}

export function removePanelField(
    declaration: StateDeclaration,
    panelIndex: number,
    fieldIndex: number,
): StateDeclaration {
    return updatePanelAt(declaration, panelIndex, (panel) => ({
        ...panel,
        fields: (panel.fields ?? []).filter((_, index) => index !== fieldIndex),
    }));
}


