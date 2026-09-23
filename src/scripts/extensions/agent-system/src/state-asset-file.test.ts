import { expect, test } from '@rstest/core';

import { createDraftFileFormat } from './state-asset-file';

/** A document on disk and the draft the editor holds, deliberately different. */
type Spec = { stages: string[] };
type Draft = { stagesText: string };

const FORMAT = createDraftFileFormat<Draft, Spec>({
    kind: 'test.thing',
    documentKey: 'thing',
    fileSuffix: '.thing.json',
    fallbackFileName: 'thing.json',
    isDocument: (value) => Array.isArray(value.stages),
    toDraft: (spec) => ({ stagesText: spec.stages.join(', ') }),
    fromDraft: (draft) => ({
        stages: draft.stagesText.split(',').map((stage) => stage.trim()).filter(Boolean),
    }),
});

function fileOf(thing: unknown, version = 1): string {
    return JSON.stringify({ kind: 'test.thing', version, thing });
}

test('a draft leaves as a file and comes back as the same draft', () => {
    const printed = FORMAT.print('scene', { stagesText: 'day, night' });

    expect(JSON.parse(printed)).toEqual({
        kind: 'test.thing',
        version: 1,
        name: 'scene',
        thing: { stages: ['day', 'night'] },
    });

    const read = FORMAT.read(printed);
    expect(read.failure).toBeNull();
    expect(read.name).toBe('scene');
    expect(read.draft).toEqual({ stagesText: 'day, night' });
});

test('each way a file can be unreadable has its own reason', () => {
    expect(FORMAT.read('{ not json').failure).toBe('invalid_json');
    expect(FORMAT.read('{"stages": []}').failure).toBe('not_a_package');
    expect(FORMAT.read(JSON.stringify({ kind: 'other.thing', version: 1, thing: { stages: [] } })).failure)
        .toBe('not_a_package');
    expect(FORMAT.read(fileOf({ stages: [] }, 99)).failure).toBe('unsupported_version');
    expect(FORMAT.read(fileOf({ nope: true })).failure).toBe('no_document');
});

test('a document the draft cannot be built from is refused rather than thrown', () => {
    const brittle = createDraftFileFormat<Draft, Spec>({
        kind: 'test.thing',
        documentKey: 'thing',
        fileSuffix: '.thing.json',
        fallbackFileName: 'thing.json',
        isDocument: (value) => Array.isArray(value.stages),
        toDraft: () => {
            throw new Error('this build cannot read that stage list');
        },
        fromDraft: (draft) => ({ stages: [draft.stagesText] }),
    });

    expect(brittle.read(fileOf({ stages: ['day'] })).failure).toBe('no_document');
});

test('the file name follows the name, and falls back when there is none', () => {
    expect(FORMAT.fileName('scene')).toBe('scene.thing.json');
    expect(FORMAT.fileName('  ')).toBe('thing.json');
});
