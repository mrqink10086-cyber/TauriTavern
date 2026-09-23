import { describe, expect, test } from '@rstest/core';

import { printStatePackage, readStatePackage, statePackageFileName, STATE_PACKAGE_KIND } from './state-package';
import type { StateDeclaration } from './state-config-model';

const declaration: StateDeclaration = {
    fields: [{ pattern: '环境/日期', label: '日期' }],
    panels: { panels: [{ title: '环境', match: '环境', templateSource: '<div>{{value 环境/日期}}</div>' }] },
    machine: { initial: ['白天'], states: [{ id: '白天' }] },
};

describe('a scene file', () => {
    test('comes back exactly as it left, rules included', () => {
        const read = readStatePackage(printStatePackage('scene', declaration));

        expect(read).toEqual({ declaration, name: 'scene', failure: null });
    });

    test('names the file after the scene', () => {
        expect(statePackageFileName('酒馆场景')).toBe('酒馆场景.scene.json');
        expect(statePackageFileName('  ')).toBe('scene.json');
    });

    test('refuses a file it cannot read instead of half-reading it', () => {
        expect(readStatePackage('{"fields":[]}').failure).toBe('not_a_package');
        expect(readStatePackage('not json at all').failure).toBe('invalid_json');
        expect(readStatePackage(JSON.stringify({ kind: STATE_PACKAGE_KIND, version: 99, declaration })).failure)
            .toBe('unsupported_version');
        expect(readStatePackage(JSON.stringify({ kind: STATE_PACKAGE_KIND, version: 1, declaration: {} })).failure)
            .toBe('no_declaration');
    });
});
