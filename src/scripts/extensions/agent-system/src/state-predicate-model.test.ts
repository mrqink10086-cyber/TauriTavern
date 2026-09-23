import { describe, expect, test } from '@rstest/core';

import {
    normalizePredicateSetForSave,
    predicateDraftFromSet,
    predicateDraftIssues,
    type PredicateSetDraft,
    type StatePredicateSet,
} from './state-predicate-model';

const SET: StatePredicateSet = {
    groups: [{
        id: 'tone',
        label: '相处',
        entries: [
            {
                id: 'distant',
                content: '保持距离。',
                tags: ['distance'],
            },
            {
                id: 'close',
                content: '可以靠近。',
                tags: ['closeness'],
                source: {
                    kind: 'state',
                    condition: { source: 'field', field: '环境/时间', op: 'in', values: ['晚上', '深夜'] },
                },
                availability: { source: 'field', field: '环境/地点', op: 'exists' },
                effects: [{ kind: 'inhibit', tags: ['distance'] }],
                priority: 10,
            },
        ],
    }],
    constants: [{
        id: 'standing',
        content: '先照顾对方的感受。',
    }],
};

describe('predicate draft round trip', () => {
    test('a stored set survives draft → normalize unchanged', () => {
        const stored = normalizePredicateSetForSave(predicateDraftFromSet(SET));
        expect(stored.groups).toEqual(SET.groups);
        expect(stored.constants).toEqual(SET.constants);
    });

    test('a composition condition is stored back exactly as loaded', () => {
        const compose = { all: [{ source: 'field', field: '环境/天气', op: 'eq', value: '雨' }] };
        const set: StatePredicateSet = {
            groups: [],
            constants: [{
                id: 'rainy',
                content: '下雨。',
                source: { kind: 'state', condition: { source: 'field', op: '', compose } },
            }],
        };

        expect(normalizePredicateSetForSave(predicateDraftFromSet(set))).toEqual(set);
    });

    test('a half-filled row is dropped rather than stored broken', () => {
        const draft: PredicateSetDraft = {
            groups: [{ id: '  ', labelText: '', entries: [] }],
            constants: [
                { id: '', labelText: '', content: '', tagsText: '', sourceKind: 'constant', condition: null, availability: null, effects: [], priorityText: '' },
                { id: ' kept ', labelText: '', content: '内容', tagsText: ' , ', sourceKind: 'constant', condition: null, availability: null, effects: [{ kind: 'inhibit', tagsText: '   ' }], priorityText: '' },
            ],
        };

        expect(normalizePredicateSetForSave(draft)).toEqual({
            groups: [],
            // A constant source is the default, so the document says nothing.
            constants: [{ id: 'kept', content: '内容' }],
        });
    });

    test('an entry that reads a state field keeps its condition', () => {
        const draft = predicateDraftFromSet(SET);
        const entry = draft.groups?.[0]?.entries?.[1];
        if (!entry) {
            throw new Error('the example group holds entries');
        }
        entry.priorityText = '10';

        const normalized = normalizePredicateSetForSave(draft);
        const close = normalized.groups?.[0]?.entries?.[1];
        expect(close).toBeDefined();
        expect(close?.source).toEqual({
            kind: 'state',
            condition: { source: 'field', field: '环境/时间', op: 'in', values: ['晚上', '深夜'] },
        });
        expect(close?.availability).toEqual({ source: 'field', field: '环境/地点', op: 'exists' });
        expect(close?.priority).toBe(10);
    });
});

describe('predicate draft issues', () => {
    function draftWith(overrides: (draft: PredicateSetDraft) => void): PredicateSetDraft {
        const draft = predicateDraftFromSet(SET);
        overrides(draft);
        return draft;
    }

    test('a clean example carries no issues', () => {
        expect(predicateDraftIssues(predicateDraftFromSet(SET))).toEqual([]);
    });

    test('two groups sharing an id are reported', () => {
        const draft = draftWith((current) => {
            current.groups.push({ id: 'tone', labelText: '', entries: [] });
        });

        expect(predicateDraftIssues(draft).map((issue) => issue.code)).toContain('duplicateGroupId');
    });

    test('two entries sharing an id are reported across groups and constants', () => {
        const draft = draftWith((current) => {
            current.constants.push({
                id: 'distant',
                labelText: '',
                content: '重复。',
                tagsText: '',
                sourceKind: 'constant',
                condition: null,
                availability: null,
                effects: [],
                priorityText: '',
            });
        });

        expect(predicateDraftIssues(draft).map((issue) => issue.code)).toContain('duplicateEntryId');
    });

    test('an entry that applies on a state field but names no field is reported', () => {
        const draft = draftWith((current) => {
            const entry = current.groups?.[0]?.entries?.[0];
            if (!entry) {
                throw new Error('the example group holds entries');
            }
            entry.sourceKind = 'state';
            entry.condition = { fieldText: '', op: 'eq', valueText: '' };
        });

        const issues = predicateDraftIssues(draft);
        expect(issues.map((issue) => issue.code)).toContain('entryConditionIncomplete');
        expect(issues.some((issue) => issue.target === 'distant')).toBe(true);
    });

    test('an effect that reaches no label is reported', () => {
        const draft = draftWith((current) => {
            const entry = current.groups?.[0]?.entries?.[0];
            if (!entry) {
                throw new Error('the example group holds entries');
            }
            entry.effects.push({ kind: 'require', tagsText: ' ' });
        });

        expect(predicateDraftIssues(draft).map((issue) => issue.code)).toContain('effectTagsRequired');
    });
});
