import { expect, test } from '@rstest/core';

import {
    embeddedMachineSummary,
    embeddedPredicateSummary,
    portableEmbeddedMachine,
    portableEmbeddedPredicate,
    readEmbeddedMachinePackage,
    readEmbeddedPredicatePackage,
} from './embedded-asset-packages';
import { DEFAULT_MACHINE_NAME, DEFAULT_PREDICATE_NAME, exampleMachine, examplePredicateSet } from './state-examples';

test('a target with no carrier holds nothing of either kind', () => {
    expect(readEmbeddedMachinePackage(undefined)).toEqual({ version: 1, items: [] });
    expect(readEmbeddedPredicatePackage(null)).toEqual({ version: 1, items: [] });
});

test('a carried machine is read back with its name', () => {
    const machine = exampleMachine();
    const packageValue = { version: 1, items: [portableEmbeddedMachine(DEFAULT_MACHINE_NAME, machine)] };

    expect(readEmbeddedMachinePackage(packageValue).items).toEqual([
        { name: DEFAULT_MACHINE_NAME, machine },
    ]);
});

test('a carried conditional set is read back with its name', () => {
    const set = examplePredicateSet();
    const packageValue = { version: 1, items: [portableEmbeddedPredicate(DEFAULT_PREDICATE_NAME, set)] };

    expect(readEmbeddedPredicatePackage(packageValue).items).toEqual([
        { name: DEFAULT_PREDICATE_NAME, set },
    ]);
});

test('a carrier from a future build is refused rather than half-read', () => {
    expect(() => readEmbeddedMachinePackage({ version: 2, items: [] })).toThrow();
    expect(() => readEmbeddedPredicatePackage({ version: 2, items: [] })).toThrow();
    expect(() => readEmbeddedMachinePackage({ version: 1 })).toThrow();
    expect(() => readEmbeddedPredicatePackage({ version: 1 })).toThrow();
});

test('an item that is not a document is refused', () => {
    expect(() => readEmbeddedMachinePackage({ version: 1, items: [{ name: 'flow' }] })).toThrow();
    expect(() => readEmbeddedMachinePackage({
        version: 1,
        items: [{ name: 'flow', machine: { states: [] } }],
    })).toThrow();
    expect(() => readEmbeddedPredicatePackage({ version: 1, items: [{ name: 'tones', set: {} }] })).toThrow();
    // A document nobody named has nowhere to be stored.
    expect(() => readEmbeddedMachinePackage({
        version: 1,
        items: [{ name: '  ', machine: exampleMachine() }],
    })).toThrow();
});

test('a summary counts what the document brings, not its name', () => {
    expect(embeddedMachineSummary({ name: 'flow', machine: exampleMachine() })).toEqual({
        name: 'flow',
        stateCount: 2,
        transitionCount: 1,
        hasHooks: false,
    });
    expect(embeddedPredicateSummary({ name: 'tones', set: examplePredicateSet() })).toEqual({
        name: 'tones',
        groupCount: 1,
        entryCount: 3,
    });
});

test('a summary of something unreadable reports zeroes instead of throwing', () => {
    expect(embeddedMachineSummary({ name: 'flow', machine: {} as never }).stateCount).toBe(0);
    expect(embeddedPredicateSummary({ name: 'tones', set: {} }).entryCount).toBe(0);
});
