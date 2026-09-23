import { describe, expect, test } from '@rstest/core';

import {
    emptyMachineDraft,
    machineDraftFromSpec,
    machineDraftIssues,
    normalizeMachineForSave,
    type MachineDraft,
    type MachineSpec,
} from './state-machine-model';

const SPEC: MachineSpec = {
    initial: ['day'],
    states: [
        { id: 'day', label: '白天' },
        { id: 'night', terminal: true },
    ],
    transitions: [{
        from: ['day'],
        to: ['night'],
        conditions: [
            { source: 'field', field: '环境/时间', op: 'eq', value: '23:00' },
            { source: 'field', field: '角色', op: 'in', values: ['甲', '乙'] },
            { source: '', op: '', compose: { all: [{ source: 'field', field: 'a', op: 'eq', value: '1' }] } },
        ],
        actions: [{ kind: 'emit', target: 'scene/ended' }],
        priority: 2,
    }],
    hooks: { script: 'export default () => true', entry: 'hook.js' },
};

describe('machine draft round trip', () => {
    test('a stored spec survives draft → normalize unchanged', () => {
        const draft = machineDraftFromSpec(SPEC);
        expect(normalizeMachineForSave(draft)).toEqual(SPEC);
    });

    test('empty rows are dropped, not stored broken', () => {
        const draft: MachineDraft = {
            initialText: '',
            states: [
                { id: '', labelText: '', terminal: false },
                { id: ' day ', labelText: ' ', terminal: false },
            ],
            transitions: [{
                fromText: '',
                toText: '',
                priorityText: '',
                conditions: [],
                actions: [],
            }],
            hooks: null,
        };
        const spec = normalizeMachineForSave(draft);
        expect(spec.states).toEqual([{ id: 'day' }]);
        expect(spec.transitions).toEqual([]);
        expect(spec.hooks).toBeNull();
    });

    test('a draft built from nothing normalizes to an empty but valid spec', () => {
        expect(normalizeMachineForSave(emptyMachineDraft())).toEqual({
            initial: [],
            states: [],
            transitions: [],
            hooks: null,
        });
    });

    test('conditions keep their value shape: eq stays a value, in becomes a list', () => {
        const draft = machineDraftFromSpec(SPEC);
        const [condition] = normalizeMachineForSave(draft).transitions[0]?.conditions ?? [];
        expect(condition).toEqual(SPEC.transitions[0]?.conditions[0]);
        const [inCondition] = normalizeMachineForSave(draft).transitions[0]?.conditions.slice(1, 2) ?? [];
        expect(inCondition?.values).toEqual(['甲', '乙']);
    });

    test('a priority of zero is not written into the document', () => {
        const draft = machineDraftFromSpec(SPEC);
        const [first] = draft.transitions;
        if (!first) {
            throw new Error('expected a transition');
        }
        draft.transitions = [{ ...first, priorityText: '0' }];
        expect(normalizeMachineForSave(draft).transitions[0]?.priority).toBeUndefined();
    });
});

describe('machine draft issues', () => {
    test('duplicate state ids, unknown references and empty transitions are reported', () => {
        const draft: MachineDraft = {
            initialText: 'day',
            states: [
                { id: 'day', labelText: '', terminal: false },
                { id: 'day', labelText: '', terminal: false },
            ],
            transitions: [
                { fromText: 'night', toText: '', priorityText: '', conditions: [], actions: [] },
                { fromText: '', toText: '', priorityText: '', conditions: [], actions: [] },
            ],
            hooks: null,
        };
        const codes = machineDraftIssues(draft).map((issue) => issue.code);
        expect(codes).toContain('duplicateState');
        expect(codes).toContain('unknownState');
        expect(codes).toContain('emptyTransition');
    });

    test('a well-formed draft raises nothing', () => {
        expect(machineDraftIssues(machineDraftFromSpec(SPEC))).toEqual([]);
    });
});
