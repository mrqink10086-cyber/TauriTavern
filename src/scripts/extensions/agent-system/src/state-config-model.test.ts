import { describe, expect, test } from '@rstest/core';

import {
    emptyDeclaration,
    isHostImageSource,
    splitKeyPatterns,
    stateDeclarationIssues,
    stateDeclarationSaveBlockers,
    type StateDeclaration,
} from './state-config-model';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';

function declarationWithPanels(panels: NonNullable<StateDeclaration['panels']>): StateDeclaration {
    return { fields: [{ pattern: '环境/日期', label: 'DATE' }], panels };
}

describe('a pasted list of key patterns', () => {
    test('splits on commas, keeping the ones a regex needs', () => {
        expect(splitKeyPatterns('环境/日期, 角色/*/着装')).toEqual(['环境/日期', '角色/*/着装']);
        expect(splitKeyPatterns('/(\\d{1,2})-(\\d{2})/, 环境/时间')).toEqual([
            '/(\\d{1,2})-(\\d{2})/',
            '环境/时间',
        ]);
        expect(splitKeyPatterns('角色/**')).toEqual(['角色/**']);
        expect(splitKeyPatterns('  ,  ')).toEqual([]);
    });
});

describe('a picture source must be one the host serves', () => {
    test('accepts the paths the host already answers', () => {
        expect(isHostImageSource('/backgrounds/cafe.png')).toBe(true);
        expect(isHostImageSource('/user/images/aira.jpg')).toBe(true);
    });

    test('refuses a scheme, a traversal, and anything that could end the path early', () => {
        for (const source of [
            'https://example.test/scene.png',
            '//example.test/scene.png',
            '/backgrounds/../../secrets.png',
            '/backgrounds/sc ene.png',
            '/backgrounds/sc"ene.png',
            '/backgrounds/scene).png',
            'backgrounds/scene.png',
        ]) {
            expect(isHostImageSource(source)).toBe(false);
        }
    });
});

describe('shape problems the editor can answer without the backend', () => {
    test('a half-typed panel is reported where the user can see it', () => {
        const issues = stateDeclarationIssues({
            fields: [{ pattern: '   ', label: 'DATE' }],
            panels: {
                panels: [
                    { title: '', match: '' },
                    { title: '环境', match: '环境/**', background: { candidates: [{ source: 'https://x/y.png' }] } },
                ],
            },
        });

        expect(issues.map((issue) => issue.path)).toEqual(['fields[0].pattern', 'panels', 'panels', 'panel `环境` background']);
    });

    test('a finished declaration has no problems', () => {
        const declaration = declarationWithPanels({
            panels: [{ title: '环境', match: '环境/**', background: { candidates: [{ source: '/backgrounds/cafe.png' }] } }],
        });

        expect(stateDeclarationIssues(declaration)).toEqual([]);
    });
});

describe('what gets stored', () => {
    test('blank rows are dropped instead of being refused by the backend', () => {
        const stored = normalizeStateDeclarationForSave({
            fields: [{ pattern: ' 环境/日期 ', label: ' DATE ' }, { pattern: '  ', label: 'ignored' }],
            panels: {
                panels: [
                    { title: ' 环境 ', match: ' 环境/** ', rail: 'right' },
                    { title: '', match: '环境/*' },
                ],
            },
        });

        expect(stored).toEqual({
            fields: [{ pattern: '环境/日期', label: 'DATE' }],
            panels: { panels: [{ title: '环境', match: '环境/**', rail: 'right' }] },
        });
    });

    test('an image set with nothing usable is left out rather than stored empty', () => {
        const stored = normalizeStateDeclarationForSave({
            fields: [],
            panels: {
                panels: [{
                    title: '环境',
                    match: '环境/**',
                    background: { candidates: [{ source: '   ' }] },
                    fields: [{ pattern: '环境/日期', images: { candidates: [{ source: '' }] } }],
                }],
            },
        });

        expect(stored.panels?.panels[0]?.background).toBeUndefined();
        expect(stored.panels?.panels[0]?.fields?.[0]?.images).toBeUndefined();
    });

    test('a candidate keeps its condition and loses an empty one', () => {
        const stored = normalizeStateDeclarationForSave({
            fields: [],
            panels: {
                panels: [{
                    title: '环境',
                    match: '环境/**',
                    background: {
                        candidates: [
                            { source: '/backgrounds/night.png', when: { source: 'field', field: ' 环境/时间 ', op: 'eq', value: '夜晚' } },
                            { source: '/backgrounds/default.png' },
                        ],
                    },
                }],
            },
        });

        expect(stored.panels?.panels[0]?.background).toEqual({
            candidates: [
                { source: '/backgrounds/night.png', when: { source: 'field', field: '环境/时间', op: 'eq', value: '夜晚' } },
                { source: '/backgrounds/default.png' },
            ],
        });
    });

    test('an empty declaration round-trips as an empty document, not a missing one', () => {
        expect(normalizeStateDeclarationForSave(emptyDeclaration())).toEqual({
            fields: [],
            panels: { panels: [] },
        });
    });

    test('a theme is stored compiled, and saving what was stored changes nothing', () => {
        const stored = normalizeStateDeclarationForSave({
            fields: [],
            panels: { panels: [{ title: '环境', match: '环境/**' }], css: '.tt-state-field { color: red }' },
        });

        expect(stored.panels?.css).toContain('.tt-state-root .tt-state-field');
        // A second prefix would make the editor's dirty flag never settle.
        expect(normalizeStateDeclarationForSave(stored)).toEqual(stored);
    });

    test('a template is stored compiled, and the text it came from is not', () => {
        const stored = normalizeStateDeclarationForSave({
            fields: [],
            panels: {
                panels: [{
                    title: '环境',
                    match: '环境/**',
                    templateSource: '<div>{{value 环境/日期}}</div>',
                }],
            },
        });

        const panel = stored.panels?.panels[0];
        expect(panel?.markup).toEqual({
            nodes: [{
                kind: 'element',
                tag: 'div',
                attrs: {},
                boundAttrs: {},
                children: [{ kind: 'value', key: '环境/日期' }],
            }],
        });
        // The source is a draft key: what a document carries is the tree.
        expect(panel).not.toHaveProperty('templateSource');
    });

    test('a template that cannot be compiled blocks the save and is not stored', () => {
        const declaration: StateDeclaration = {
            fields: [],
            panels: {
                panels: [{ title: '环境', match: '环境/**', templateSource: '<script>alert(1)</script>' }],
            },
        };

        expect(stateDeclarationSaveBlockers(declaration).map((issue) => issue.path))
            .toEqual(['panel `环境` template']);
        expect(normalizeStateDeclarationForSave(declaration).panels?.panels[0]?.markup).toBeUndefined();
    });

    test('a theme that cannot be compiled is reported and left out of the document', () => {
        const declaration: StateDeclaration = {
            fields: [],
            panels: { panels: [], css: '@import url(/backgrounds/theme.css);' },
        };

        expect(stateDeclarationIssues(declaration).map((issue) => issue.path)).toEqual(['theme.css']);
        expect(normalizeStateDeclarationForSave(declaration).panels?.css).toBeUndefined();
    });
});
