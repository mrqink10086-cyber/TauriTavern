/**
 * The picture sets of a declaration.
 *
 * A set hangs off one of two places — a panel's own background, or one field's
 * pictures — and every edit here has to rebuild the set whole. That is what
 * makes it a module: an edit that spread the old set around would drop a part
 * it did not know about, and `fit` is exactly the part that arrives later than
 * the code that rebuilds it.
 *
 * The panels themselves belong to `state-config-ops`; this only addresses them.
 */

import { updatePanelAt } from './state-config-ops';
import type { StateConditionEdit, StateImageTarget } from './state-config-ops';
import type {
    StateConditionSpec,
    StateDeclaration,
    StateImageCandidate,
    StateImageFit,
    StateImageSet,
    StatePanelFieldSpec,
    StatePanelSpec,
    StateScriptSpec,
} from './state-config-model';

/**
 * Rebuild a panel with the picture set it is left with.
 *
 * A set that became empty loses the key instead of being stored empty: "no
 * pictures" and "pictures configured, none of them usable" must not look alike.
 */
function panelWithBackground(panel: StatePanelSpec, background: StateImageSet | null): StatePanelSpec {
    const next: StatePanelSpec = { title: panel.title, match: panel.match };
    if (panel.rail) {
        next.rail = panel.rail;
    }
    if (background) {
        next.background = background;
    }
    if (panel.fields) {
        next.fields = panel.fields;
    }
    return next;
}

function fieldWithImages(field: StatePanelFieldSpec, images: StateImageSet | null): StatePanelFieldSpec {
    const next: StatePanelFieldSpec = { pattern: field.pattern };
    if (field.label) {
        next.label = field.label;
    }
    if (field.render) {
        next.render = field.render;
    }
    if (images) {
        next.images = images;
    }
    return next;
}

function withImages(
    declaration: StateDeclaration,
    target: StateImageTarget,
    update: (images: StateImageSet | null) => StateImageSet | null,
): StateDeclaration {
    if (target.kind === 'background') {
        return updatePanelAt(declaration, target.panelIndex, (panel) => (
            panelWithBackground(panel, update(panel.background ?? null))
        ));
    }
    return updatePanelAt(declaration, target.panelIndex, (panel) => ({
        ...panel,
        fields: (panel.fields ?? []).map((field, index) => (
            index === target.fieldIndex ? fieldWithImages(field, update(field.images ?? null)) : field
        )),
    }));
}

/** A candidate without its condition: what makes it the unconditional fallback. */
function withoutCondition(candidate: StateImageCandidate): StateImageCandidate {
    return { source: candidate.source };
}

/**
 * A set as it is stored, or nothing when it now says nothing.
 *
 * Every part is named here rather than spread, because this is what an edit
 * rebuilds: a part an edit forgot to carry is dropped on the next keystroke.
 */
function storedImageSet(
    candidates: StateImageCandidate[],
    conditionScript: StateScriptSpec | null,
    fit: StateImageFit | undefined,
): StateImageSet | null {
    if (candidates.length === 0 && !conditionScript) {
        return null;
    }
    const set: StateImageSet = { candidates };
    if (conditionScript) {
        set.conditionScript = conditionScript;
    }
    // `cover` is what an absent fit means, so only the departure is stored.
    if (fit === 'contain') {
        set.fit = fit;
    }
    return set;
}

/**
 * Edit just the candidates of a set, keeping everything else about it.
 *
 * The script and the fit live next to the candidates, so an edit that only
 * meant to change a picture must not quietly drop either — and a set left with
 * neither pictures nor a script is removed rather than stored empty.
 */
function withCandidates(
    declaration: StateDeclaration,
    target: StateImageTarget,
    update: (candidates: StateImageCandidate[]) => StateImageCandidate[],
): StateDeclaration {
    return withImages(declaration, target, (images) => storedImageSet(
        update(images?.candidates ?? []),
        images?.conditionScript ?? null,
        images?.fit,
    ));
}

/**
 * How the set's picture fills its box.
 *
 * Choosing `cover` stores nothing, because the set already says that by not
 * saying anything.
 */
export function setImageFit(
    declaration: StateDeclaration,
    target: StateImageTarget,
    fit: StateImageFit,
): StateDeclaration {
    return withImages(declaration, target, (images) => storedImageSet(
        images?.candidates ?? [],
        images?.conditionScript ?? null,
        fit === 'contain' ? 'contain' : undefined,
    ));
}

export function addImageCandidate(
    declaration: StateDeclaration,
    target: StateImageTarget,
): StateDeclaration {
    return withCandidates(declaration, target, (candidates) => [
        ...candidates,
        { source: '' },
    ]);
}

export function removeImageCandidate(
    declaration: StateDeclaration,
    target: StateImageTarget,
    candidateIndex: number,
): StateDeclaration {
    return withCandidates(declaration, target, (candidates) => (
        candidates.filter((_, index) => index !== candidateIndex)
    ));
}

export function setImageCandidateSource(
    declaration: StateDeclaration,
    target: StateImageTarget,
    candidateIndex: number,
    source: string,
): StateDeclaration {
    return withCandidates(declaration, target, (candidates) => candidates.map((candidate, index) => (
        index === candidateIndex ? { ...candidate, source } : candidate
    )));
}

/**
 * Set the condition of one candidate.
 *
 * The comparison stays as text; the save normalization splits it for the ops
 * that compare against a list. An empty field means "no condition", which is
 * what makes a candidate the fallback.
 */
export function setImageCandidateCondition(
    declaration: StateDeclaration,
    target: StateImageTarget,
    candidateIndex: number,
    edit: StateConditionEdit,
): StateDeclaration {
    const field = edit.field.trim();
    return withCandidates(declaration, target, (candidates) => candidates.map((candidate, index) => {
        if (index !== candidateIndex) {
            return candidate;
        }
        if (!field) {
            return withoutCondition(candidate);
        }
        const when: StateConditionSpec = {
            source: 'field',
            field,
            op: edit.op.trim() || 'eq',
            value: edit.valueText,
        };
        return { ...candidate, when };
    }));
}

export function clearImageCandidateCondition(
    declaration: StateDeclaration,
    target: StateImageTarget,
    candidateIndex: number,
): StateDeclaration {
    return withCandidates(declaration, target, (candidates) => candidates.map((candidate, index) => (
        index === candidateIndex ? withoutCondition(candidate) : candidate
    )));
}

/**
 * Set the set's condition script.
 *
 * The source is kept exactly as typed, including nothing: an empty script is the
 * default state ("conditions decide"), and the save normalization is what drops
 * it from the stored document.
 */
export function setImageConditionScript(
    declaration: StateDeclaration,
    target: StateImageTarget,
    script: string,
): StateDeclaration {
    return withImages(declaration, target, (images) => storedImageSet(
        images?.candidates ?? [],
        { script },
        images?.fit,
    ));
}

export function clearImageConditionScript(
    declaration: StateDeclaration,
    target: StateImageTarget,
): StateDeclaration {
    return withCandidates(declaration, target, (candidates) => candidates);
}
