import { expect, test } from '@rstest/core';

import { defaultProfile, normalizeProfileForSave, profileForEdit } from './profile-model';
import { applyRecallPatch } from './profile-recall';

test('the access grid stores the switches and only the values that differ from the defaults', () => {
    const profile = defaultProfile('state-access');
    const draft = profileForEdit(profile);
    draft.stateAccess = {
        entries: [
            // A half-typed row covers nothing, so it is dropped rather than
            // handed to a backend that would refuse the whole policy.
            { pattern: '   ', inject: true },
            {
                pattern: ' 角色/*/着装 ',
                inject: true,
                visible: false,
                writable: true,
                injectSlot: 'atDepth',
                injectDepth: 4,
            },
            {
                pattern: '环境/日期',
                inject: true,
                visible: true,
                writable: false,
                injectSlot: 'after',
                injectDepth: 6,
            },
        ],
    };

    const stored = normalizeProfileForSave(draft).stateAccess;

    expect(stored).toEqual({
        entries: [
            {
                pattern: '角色/*/着装',
                inject: true,
                visible: false,
                writable: true,
            },
            {
                pattern: '环境/日期',
                inject: true,
                visible: true,
                writable: false,
                injectSlot: 'after',
                injectDepth: 6,
            },
        ],
    });
});

test('a row the form shows has a slot and a depth even when the profile did not say', () => {
    const profile = defaultProfile('state-access-defaults');
    profile.stateAccess = {
        entries: [{ pattern: '环境/**', inject: true }],
    };

    const rows = profileForEdit(profile).stateAccess?.entries ?? [];

    expect(rows).toEqual([
        {
            pattern: '环境/**',
            inject: true,
            visible: false,
            writable: false,
            injectSlot: 'atDepth',
            injectDepth: 4,
        },
    ]);
});

test('profileForEdit migrates v2 native tool names to canonical ToolIds', () => {
    const profile = defaultProfile('legacy-profile');
    profile.schemaVersion = 2;
    profile.tools.allow = ['workspace.read_file'];
    profile.tools.deny = ['workspace.write_file'];
    profile.tools.toolDescriptions = { 'workspace.read_file': { description: 'Read' } };
    profile.tools.maxCallsPerTool = { 'workspace.read_file': 4 };
    // Simulate a v2 persisted profile that predates the field.
    Reflect.deleteProperty(profile.tools, 'mcpResultInlineCharLimit');

    const migrated = profileForEdit(profile);
    expect(migrated.schemaVersion).toBe(3);
    expect(migrated.tools.allow).toEqual(['builtin:workspace.read_file']);
    expect(migrated.tools.deny).toEqual(['builtin:workspace.write_file']);
    expect(Object.keys(migrated.tools.toolDescriptions ?? {})).toEqual(['builtin:workspace.read_file']);
    expect(Object.keys(migrated.tools.maxCallsPerTool ?? {})).toEqual(['builtin:workspace.read_file']);
    expect(migrated.tools.mcpResultInlineCharLimit).toBe(50_000);

    profile.schemaVersion = 4;
    expect(() => profileForEdit(profile)).toThrow(/profile\.schemaVersion is unsupported: 4/);
});

test('profileForEdit keeps CSV drafts separate and normalizeProfileForSave restores lists', () => {
    const profile = defaultProfile('writer');
    profile.run.stream = false;
    profile.skills.visible = ['lore', 'tools'];
    profile.delegation.allowedCallers = ['main', 'reviewer'];

    const draft = profileForEdit(profile);
    expect(draft.skills.visibleCsv).toBe('lore, tools');
    expect(draft.delegation.allowedCallersCsv).toBe('main, reviewer');
    draft.skills.visibleCsv = 'research, tools';
    draft.delegation.allowedCallersCsv = 'editor';

    const saved = normalizeProfileForSave(draft);
    expect(saved.run.stream).toBe(false);
    expect(saved.skills.visible).toEqual(['research', 'tools']);
    expect(saved.delegation.allowedCallers).toEqual(['editor']);
    expect('visibleCsv' in saved.skills).toBe(false);
    expect('allowedCallersCsv' in saved.delegation).toBe(false);
});

test('recall round-trips through the CSV mirror and saves what it shows', () => {
    const profile = defaultProfile('recall');
    // A Profile that says nothing about recall gets the host's own default: carry
    // the blocks, hand none of them to a sub-agent.
    expect(profileForEdit(profile).recall).toEqual({
        inject: true,
        sources: ['3_vectfox*'],
        subagent: 'skip',
        sourcesCsv: '3_vectfox*',
    });

    profile.recall = { inject: false, sources: ['3_vectfox', 'my_recall*'], subagent: 'inherit' };
    const draft = profileForEdit(profile);
    expect(draft.recall?.sourcesCsv).toBe('3_vectfox, my_recall*');

    draft.recall = applyRecallPatch(draft, { sourcesCsv: 'other_recall*' });
    const saved = normalizeProfileForSave(draft);

    expect(saved.recall).toEqual({
        inject: false,
        sources: ['other_recall*'],
        subagent: 'inherit',
    });
    expect('sourcesCsv' in (saved.recall ?? {})).toBe(false);
});

test('an emptied recall source list means this Agent claims no block', () => {
    const draft = profileForEdit(defaultProfile('recall-empty'));
    draft.recall = applyRecallPatch(draft, { sourcesCsv: '' });

    expect(normalizeProfileForSave(draft).recall?.sources).toEqual([]);
});

test('run streaming defaults missing fields and rejects non-booleans', () => {
    const profile = defaultProfile('streaming');
    Reflect.deleteProperty(profile.run, 'stream');
    expect(profileForEdit(profile).run.stream).toBe(false);

    Reflect.set(profile.run, 'stream', 'true');
    expect(() => normalizeProfileForSave(profile)).toThrow(/run\.stream must be a boolean/);
});

test('tool description overrides preserve user text and reject invalid values', () => {
    const profile = defaultProfile('descriptions');
    profile.tools.toolDescriptions = {
        'builtin:workspace.read_file': {
            description: '  Read exactly this way.  ',
            properties: { path: '  Use the supplied path.  ' },
        },
    };

    expect(normalizeProfileForSave(profile).tools.toolDescriptions).toEqual(profile.tools.toolDescriptions);

    Reflect.set(profile.tools, 'toolDescriptions', {
        'builtin:workspace.read_file': { description: 42 },
    });
    expect(() => normalizeProfileForSave(profile)).toThrow(/description must be a string/);
});
