import { describe, expect, test } from '@rstest/core';

import { stateDeclarationIssues } from './state-config-model';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';
import { exampleDeclaration, exampleMachine, examplePredicateSet } from './state-examples';
import { seraphinaDeclaration } from './state-examples-seraphina';
import {
    machineDraftFromSpec,
    machineDraftIssues,
    normalizeMachineForSave,
    type MachineSpec,
} from './state-machine-model';
import {
    normalizePredicateSetForSave,
    predicateDraftFromSet,
    predicateDraftIssues,
    type StatePredicateSet,
} from './state-predicate-model';

describe('the document a new editor starts from', () => {
    // The starter is stored the moment it is saved, so a row that normalization
    // drops would make a brand new declaration shrink under the user's hands.
    test('the example declaration saves as it stands, losing no row', () => {
        const example = exampleDeclaration();
        expect(stateDeclarationIssues(example)).toEqual([]);

        const stored = normalizeStateDeclarationForSave(example);
        expect(stored.fields).toHaveLength(example.fields.length);
        expect(stored.panels?.panels).toHaveLength(example.panels?.panels.length ?? 0);
        expect(normalizeStateDeclarationForSave(stored)).toEqual(stored);
    });

    // An example that referred to a state it never declared would open every new
    // machine with a warning the user did not cause.
    // The worked example is a bigger document — a template per panel, a theme
    // sheet, initial values, and a machine inside it — so the same round trip
    // has to hold for it, or a user opens it already broken.
    test('the Seraphina example saves as it stands too', () => {
        const example = seraphinaDeclaration();
        expect(stateDeclarationIssues(example)).toEqual([]);

        const stored = normalizeStateDeclarationForSave(example);
        expect(stored.fields).toHaveLength(example.fields.length);
        expect(stored.panels?.panels).toHaveLength(example.panels?.panels.length ?? 0);
        expect(normalizeStateDeclarationForSave(stored)).toEqual(stored);

        // The values a chat starts from survive the trip, or the scene opens
        // empty and the model has to invent a world before it can narrate one.
        // Every literal key carries one: a pattern has no key to write to, which
        // is what keeps this an invariant rather than a count to keep in step.
        const literalFields = example.fields.filter((field) => (
            !field.pattern.includes('*') && !field.pattern.startsWith('/')
        ));
        expect(literalFields.length).toBeGreaterThan(0);
        expect(literalFields.every((field) => (field.initial ?? []).length > 0)).toBe(true);

        // The stages and the conditional text it carries are documents their own
        // editors would accept — the scene ships as something to open, not to fix.
        expect(machineDraftIssues(machineDraftFromSpec(example.machine as MachineSpec))).toEqual([]);
        expect(predicateDraftIssues(
            predicateDraftFromSet(example.predicates as StatePredicateSet),
        )).toEqual([]);
    });

    test('the example machine refers only to states it declares', () => {
        const example = exampleMachine();
        expect(machineDraftIssues(machineDraftFromSpec(example))).toEqual([]);
        expect(normalizeMachineForSave(machineDraftFromSpec(example))).toEqual(example);
    });

    // An example that normalization would change opens the editor already dirty,
    // so a brand new set must survive the round trip exactly as it stands.
    test('the example predicate set saves as it stands', () => {
        const example = examplePredicateSet();
        expect(predicateDraftIssues(predicateDraftFromSet(example))).toEqual([]);
        expect(normalizePredicateSetForSave(predicateDraftFromSet(example))).toEqual(example);
    });
});
