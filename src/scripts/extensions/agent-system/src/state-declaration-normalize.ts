/**
 * The stored shape of a declaration.
 *
 * The editor holds a draft — half-typed rows, template text, an unparsed theme —
 * and the document is what survives that: trimmed, blank rows dropped, the theme
 * and every template compiled, and the parts this editor does not render (the
 * stages, the conditional text) carried through untouched.
 *
 * Everything here is a pure function of the draft, because the save, the dirty
 * flag and the export all have to agree on what "stored" means.
 */

import { compileTemplate, printTemplate } from './state-template';
import { compileThemeCss } from './state-theme-css';
import {
    conditionValueText,
    isSetConditionOp,
    type DeclaredStateField,
    type StateConditionSpec,
    type StateDeclaration,
    type StateImageSet,
    type StatePanelFieldSpec,
    type StatePanelSpec,
    type StateScriptModules,
    type StateScriptSpec,
} from './state-config-model';

/**
 * The document to store: trimmed, with blank rows dropped.
 *
 * A blank row is what a half-typed form leaves behind, and storing it would
 * make the backend refuse the whole declaration for a row the user cannot see.
 */
export function normalizeStateDeclarationForSave(declaration: StateDeclaration): StateDeclaration {
    const fields = declaration.fields
        .map((field) => {
            const normalized: DeclaredStateField = {
                pattern: field.pattern.trim(),
                label: field.label.trim(),
            };
            // A field that still carries the defaults says nothing, and dropping
            // it keeps the document readable and diffable.
            if (field.access) {
                normalized.access = field.access;
            }
            const initial = (field.initial ?? [])
                .map((value) => String(value).trim())
                .filter((value) => value.length > 0);
            if (initial.length > 0) {
                normalized.initial = initial;
            }
            return normalized;
        })
        .filter((field) => field.pattern.length > 0);

    const panels = (declaration.panels?.panels ?? [])
        .map((panel) => {
            const normalized: StatePanelSpec = {
                title: panel.title.trim(),
                rail: panel.rail === 'right' ? 'right' : 'left',
                match: panel.match.trim(),
            };
            const background = normalizeImageSet(panel.background);
            if (background) {
                normalized.background = background;
            }
            const panelFields = (panel.fields ?? [])
                .map((field) => normalizePanelField(field))
                .filter((field) => field.pattern.length > 0);
            if (panelFields.length > 0) {
                normalized.fields = panelFields;
            }
            // A row with a heading but no path is half-typed, and storing it
            // would make the backend refuse the whole declaration over it.
            const prosePath = String(panel.prose?.path ?? '').trim();
            if (prosePath) {
                normalized.prose = {
                    path: prosePath,
                    title: String(panel.prose?.title ?? '').trim(),
                };
            }
            const markup = panel.templateSource === undefined || panel.templateSource === null
                // A draft that never carried a source — a loaded document handed
                // straight back — keeps the tree it already has.
                ? (panel.markup ?? null)
                : compileTemplate(String(panel.templateSource), 'template').markup;
            if (markup && markup.nodes.length > 0) {
                normalized.markup = markup;
            }
            return normalized;
        })
        .filter((panel) => panel.title.length > 0 && panel.match.length > 0);

    const scripts = normalizeScriptModules(declaration.panels?.scripts);
    const css = compileThemeCss(declaration.panels?.css ?? '').css;

    const config: NonNullable<StateDeclaration['panels']> = { panels };
    if (scripts) {
        config.scripts = scripts;
    }
    if (css) {
        config.css = css;
    }

    const document: StateDeclaration = { fields, panels: config };
    // The ceilings belong to the scene, and a document that dropped them on
    // every save would reset what the author decided without saying so.
    if (declaration.limits) {
        document.limits = declaration.limits;
    }
    // The stages and the conditional text belong to the scene but are edited in
    // their own tabs; this editor carries them through untouched rather than
    // dropping what it does not render.
    if (declaration.machine) {
        document.machine = declaration.machine;
    }
    if (declaration.predicates) {
        document.predicates = declaration.predicates;
    }
    return document;
}

/**
 * A stored declaration as the editor holds it.
 *
 * A stored panel carries the compiled tree and no source; the editor edits the
 * text it was compiled from, so the tree is printed back — unless the document
 * already carries source, which is what a pasted draft looks like.
 */
export function declarationDraftFromDocument(declaration: StateDeclaration): StateDeclaration {
    return {
        ...declaration,
        panels: {
            ...declaration.panels,
            panels: (declaration.panels?.panels ?? []).map((panel) => ({
                ...panel,
                templateSource: panel.templateSource ?? printTemplate(panel.markup),
            })),
        },
    };
}

/**
 * The shared modules as the document stores them.
 *
 * A module with no source is a row the user has not written yet, which is the
 * same convention a blank field row follows; a blank name cannot happen, since
 * the editor refuses one before it is added.
 */
function normalizeScriptModules(
    modules: StateScriptModules | null | undefined,
): StateScriptModules | null {
    const entries = Object.entries(modules ?? {})
        .map(([name, source]) => [name.trim(), String(source ?? '')] as const)
        .filter(([name, source]) => name.length > 0 && source.trim().length > 0);

    return entries.length > 0 ? Object.fromEntries(entries) : null;
}

function normalizePanelField(field: StatePanelFieldSpec): StatePanelFieldSpec {
    const normalized: StatePanelFieldSpec = {
        pattern: field.pattern.trim(),
        render: field.render === 'image' ? 'image' : 'text',
    };
    const label = String(field.label ?? '').trim();
    if (label) {
        normalized.label = label;
    }
    const images = normalizeImageSet(field.images);
    if (images) {
        normalized.images = images;
    }
    return normalized;
}

function normalizeImageSet(images: StateImageSet | null | undefined): StateImageSet | null {
    const candidates = (images?.candidates ?? [])
        .map((candidate) => {
            const source = candidate.source.trim();
            const condition = normalizeCondition(candidate.when);
            return condition ? { source, when: condition } : { source };
        })
        .filter((candidate) => candidate.source.length > 0);
    const conditionScript = normalizeScript(images?.conditionScript);

    // A set with no usable candidate is not a set — unless it declares a script,
    // which is a decision in its own right and survives on its own.
    if (candidates.length === 0 && !conditionScript) {
        return null;
    }
    // `cover` is what an absent fit means, so only the departure is stored.
    const fit = images?.fit === 'contain' ? { fit: 'contain' as const } : {};
    return conditionScript
        ? { candidates, conditionScript, ...fit }
        : { candidates, ...fit };
}

/**
 * The script as the document stores it.
 *
 * A blank script is the default state — no script, the conditions decide — so a
 * half-filled textarea is dropped rather than stored as an empty script the
 * backend would refuse.
 */
function normalizeScript(script: StateScriptSpec | null | undefined): StateScriptSpec | null {
    const source = String(script?.script ?? '').trim();
    if (!source) {
        return null;
    }
    const entry = String(script?.entry ?? '').trim();
    return entry ? { script: source, entry } : { script: source };
}

/**
 * The condition as the document stores it.
 *
 * The editor holds the comparison as text either way; a list comparison is
 * split here, where the finished document is built, so the draft stays what the
 * user typed. A condition without a field is not a condition: it would compare
 * nothing and read as configured.
 */
function normalizeCondition(condition: StateConditionSpec | null | undefined): StateConditionSpec | null {
    // A combination is passed through whole: the editor does not rewrite parts
    // it cannot show, and the backend still refuses one that also compares.
    if (condition?.compose) {
        return condition;
    }

    const field = String(condition?.field ?? '').trim();
    if (!field) {
        return null;
    }
    const op = String(condition?.op ?? '').trim() || 'eq';
    const source = condition?.source || 'field';
    if (isSetConditionOp(op)) {
        const values = conditionValueText(condition)
            .split(',')
            .map((value) => value.trim())
            .filter((value) => value.length > 0);
        return { source, field, op, values };
    }
    const value = String(condition?.value ?? '').trim();
    return { source, field, op, value: value || null };
}
