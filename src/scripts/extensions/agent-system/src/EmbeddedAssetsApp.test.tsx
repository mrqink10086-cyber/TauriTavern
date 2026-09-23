import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';

import { EmbeddedAssetsApp } from './EmbeddedAssetsApp';
import {
    type EmbeddedAssetsActions,
    type EmbeddedAssetsInitial,
    type EmbeddedAssetsRead,
    type EmbeddedSkillItem,
    type EmbeddedStateItem,
    buildSkillOptions,
} from './EmbeddedAssetsContract';

const formatParam = (value: unknown): string => {
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
        return String(value);
    }
    return JSON.stringify(value) ?? '';
};

const tr = (key: string, params: Record<string, unknown> = {}): string => (
    Object.entries(params).reduce((text, [name, value]) => `${text} ${name}=${formatParam(value)}`, key)
);

function skillEntry(name: string, scope: TauriTavernSkillScope): TauriTavernSkillIndexEntry {
    return {
        scope,
        name,
        description: '',
        tags: [],
        installedHash: 'hash',
        fileCount: 1,
        totalBytes: 1,
        hasScripts: false,
        hasBinary: false,
        installedAt: '2026-01-01T00:00:00Z',
    };
}

function embeddedSkill(name: string): EmbeddedSkillItem {
    return {
        skillName: name,
        sourceScopeLabel: 'Global',
        fileName: `${name}.skill`,
    };
}

function deferredInitial(): { promise: Promise<EmbeddedAssetsInitial>; resolve: (value: EmbeddedAssetsInitial) => void } {
    const holder: { resolve: ((value: EmbeddedAssetsInitial) => void) | null } = { resolve: null };
    const promise = new Promise<EmbeddedAssetsInitial>((resolve) => {
        holder.resolve = resolve;
    });
    return {
        promise,
        resolve: (value) => holder.resolve?.(value),
    };
}

function emptyInitial(): EmbeddedAssetsInitial {
    return {
        targetInfo: { kind: 'preset', name: 'Preset A', subtitle: 'Chat Completion' },
        profiles: [],
        skills: [],
        states: [],
        machines: [],
        predicates: [],
        embeddedProfiles: [],
        embeddedSkills: [],
        embeddedStates: [],
        embeddedMachines: [],
        embeddedPredicates: [],
    };
}

function createWorld(options: {
    profiles?: TauriTavernAgentProfileSummary[];
    skills?: TauriTavernSkillIndexEntry[];
    embeddedProfiles?: EmbeddedAssetsRead['profiles'];
    embeddedSkills?: EmbeddedAssetsRead['skills'];
    machines?: string[];
    predicates?: string[];
    embeddedMachines?: EmbeddedAssetsRead['machines'];
    embeddedPredicates?: EmbeddedAssetsRead['predicates'];
    failLoad?: boolean;
} = {}) {
    const world = {
        embedded: {
            target: { kind: 'preset' as const, name: 'Preset A', subtitle: 'Chat Completion' },
            profiles: options.embeddedProfiles ?? [],
            skills: options.embeddedSkills ?? [],
            states: [] as EmbeddedStateItem[],
            machines: options.embeddedMachines ?? [],
            predicates: options.embeddedPredicates ?? [],
        },
        embeddedProfileIds: [] as string[],
        embeddedSkillNames: [] as string[],
        embeddedMachineNames: [] as string[],
        embeddedPredicateNames: [] as string[],
        removedProfileIds: [] as string[],
        removedSkillNames: [] as string[],
        removedMachineNames: [] as string[],
        removedPredicateNames: [] as string[],
        toasts: [] as string[],
        errors: [] as string[],
    };
    const initial: EmbeddedAssetsInitial = {
        targetInfo: world.embedded.target,
        profiles: options.profiles ?? [
            { id: 'default-writer', displayName: 'Default Writer', directRunnable: true },
            { id: 'writer-two', displayName: 'Writer Two', directRunnable: true },
        ],
        skills: buildSkillOptions(options.skills ?? [
            skillEntry('lore-helper', { kind: 'global' }),
            skillEntry('beta-tool', { kind: 'character', characterId: 'char-1' }),
        ]),
        states: [],
        machines: options.machines ?? [],
        predicates: options.predicates ?? [],
        embeddedProfiles: world.embedded.profiles,
        embeddedSkills: world.embedded.skills,
        embeddedStates: world.embedded.states,
        embeddedMachines: world.embedded.machines,
        embeddedPredicates: world.embedded.predicates,
    };
    const actions: EmbeddedAssetsActions = {
        embedProfile: (profileId) => {
            world.embeddedProfileIds.push(profileId);
            world.embedded.profiles = [
                ...world.embedded.profiles.filter((item) => item.profile.id !== profileId),
                { profile: { id: profileId, displayName: profileId } },
            ];
            return Promise.resolve(profileId);
        },
        embedSkill: (skill) => {
            world.embeddedSkillNames.push(skill.name);
            world.embedded.skills = [
                ...world.embedded.skills.filter((item) => item.skillName !== skill.name),
                embeddedSkill(skill.name),
            ];
            return Promise.resolve();
        },
        embedState: () => Promise.resolve('scene'),
        embedMachine: (machineName) => {
            world.embeddedMachineNames.push(machineName);
            world.embedded.machines = [
                ...world.embedded.machines.filter((item) => item.name !== machineName),
                { name: machineName, stateCount: 2, transitionCount: 1, hasHooks: false },
            ];
            return Promise.resolve(machineName);
        },
        embedPredicateSet: (setName) => {
            world.embeddedPredicateNames.push(setName);
            world.embedded.predicates = [
                ...world.embedded.predicates.filter((item) => item.name !== setName),
                { name: setName, groupCount: 1, entryCount: 3 },
            ];
            return Promise.resolve(setName);
        },
        removeProfile: (profileId) => {
            world.removedProfileIds.push(profileId);
            world.embedded.profiles = world.embedded.profiles.filter((item) => item.profile.id !== profileId);
            return Promise.resolve();
        },
        removeSkill: (skillName) => {
            world.removedSkillNames.push(skillName);
            world.embedded.skills = world.embedded.skills.filter((item) => item.skillName !== skillName);
            return Promise.resolve();
        },
        removeState: () => Promise.resolve(),
        removeMachine: (machineName) => {
            world.removedMachineNames.push(machineName);
            world.embedded.machines = world.embedded.machines.filter((item) => item.name !== machineName);
            return Promise.resolve();
        },
        removePredicateSet: (setName) => {
            world.removedPredicateNames.push(setName);
            world.embedded.predicates = world.embedded.predicates.filter((item) => item.name !== setName);
            return Promise.resolve();
        },
        readEmbedded: () => ({
            target: world.embedded.target,
            profiles: world.embedded.profiles,
            skills: world.embedded.skills,
            states: world.embedded.states,
            machines: world.embedded.machines,
            predicates: world.embedded.predicates,
        }),
        toastSuccess: (message) => {
            world.toasts.push(message);
        },
        reportError: (error) => {
            const message = error instanceof Error ? error.message : 'unknown error';
            world.errors.push(message);
            return message;
        },
    };
    const initialLoad = options.failLoad
        ? Promise.reject(new Error('read failed'))
        : Promise.resolve(initial);
    return { actions, initialLoad, world };
}

afterEach(() => {
    cleanup();
});

test('shows a loading state until the initial load resolves', async () => {
    const pending = deferredInitial();
    const { actions } = createWorld();
    render(
        <EmbeddedAssetsApp
            initialLoad={pending.promise}
            actions={actions}
            tr={tr}
            onRequestClose={() => undefined}
        />,
    );

    expect(screen.getByRole('status')).toBeDefined();
    await act(async () => {
        pending.resolve(emptyInitial());
        await Promise.resolve();
    });
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getAllByText('Preset A').length).toBeGreaterThan(0);
});

test('embeds the selected profile and refreshes the embedded list', async () => {
    const { actions, initialLoad, world } = createWorld();
    const user = userEvent.setup();
    render(<EmbeddedAssetsApp initialLoad={initialLoad} actions={actions} tr={tr} onRequestClose={() => undefined} />);
    await waitFor(() => expect(screen.getAllByText('Preset A').length).toBeGreaterThan(0));

    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: 'selectProfile' });
    // The built-in default profile is never embeddable.
    expect(select.querySelectorAll('option')).toHaveLength(1);
    expect(select.value).toBe('writer-two');

    await user.click(screen.getByRole('button', { name: /embedProfile/ }));
    await waitFor(() => expect(world.embeddedProfileIds).toEqual(['writer-two']));
    expect(world.toasts).toEqual(['embeddedProfile id=writer-two']);
    // The refreshed embedded list renders the new profile row (name + id).
    expect(screen.getAllByText('writer-two').length).toBeGreaterThanOrEqual(2);
});

test('embeds the auto-selected skill and removes embedded items', async () => {
    const { actions, initialLoad, world } = createWorld({
        embeddedSkills: [embeddedSkill('lore-helper')],
    });
    const user = userEvent.setup();
    render(<EmbeddedAssetsApp initialLoad={initialLoad} actions={actions} tr={tr} onRequestClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText('lore-helper')).toBeDefined());

    // Skills sort by name, so beta-tool is the auto-selected first option.
    const skillSelect = screen.getByRole<HTMLSelectElement>('combobox', { name: 'selectSkill' });
    expect(skillSelect.value).toContain('beta-tool');

    await user.click(screen.getByRole('button', { name: /embedSkill/ }));
    await waitFor(() => expect(world.embeddedSkillNames).toEqual(['beta-tool']));
    expect(world.toasts[0]).toContain('embeddedSkill');

    const removeButtons = screen.getAllByRole('button', { name: 'removeEmbeddedAsset' });
    expect(removeButtons).toHaveLength(2);
    const [firstRemoveButton] = removeButtons;
    if (!firstRemoveButton) {
        throw new Error('expected the first embedded skill remove button');
    }
    await user.click(firstRemoveButton);
    await waitFor(() => expect(world.removedSkillNames).toEqual(['lore-helper']));
    expect(screen.getAllByRole('button', { name: 'removeEmbeddedAsset' })).toHaveLength(1);
});

test('carries a state machine and a conditional set, then takes them back off', async () => {
    const { actions, initialLoad, world } = createWorld({
        machines: ['flow-two', 'flow-one'],
        predicates: ['tones'],
        embeddedPredicates: [{ name: 'tones', groupCount: 2, entryCount: 5 }],
    });
    const user = userEvent.setup();
    render(<EmbeddedAssetsApp initialLoad={initialLoad} actions={actions} tr={tr} onRequestClose={() => undefined} />);
    await waitFor(() => expect(screen.getAllByText('Preset A').length).toBeGreaterThan(0));

    // The lists arrive in store order, and the first entry is what is selected.
    const machineSelect = screen.getByRole<HTMLSelectElement>('combobox', { name: 'selectMachine' });
    expect(machineSelect.value).toBe('flow-two');
    await user.click(screen.getByRole('button', { name: /embedMachine/ }));
    await waitFor(() => expect(world.embeddedMachineNames).toEqual(['flow-two']));
    expect(world.toasts).toContain('embeddedMachine name=flow-two');
    expect(screen.getAllByText('flow-two').length).toBeGreaterThanOrEqual(2);

    // The carried rows report what is in them, not just their names. (Subtitles
    // run through the real translator, not this test's `tr` stub.)
    expect(screen.getByText(/2 stages/)).toBeDefined();
    expect(screen.getByText(/2 groups/)).toBeDefined();

    const removeButtons = screen.getAllByRole('button', { name: 'removeEmbeddedAsset' });
    expect(removeButtons).toHaveLength(2);
    const [firstRemoveButton] = removeButtons;
    if (!firstRemoveButton) {
        throw new Error('expected the first embedded remove button');
    }
    await user.click(firstRemoveButton);
    await waitFor(() => expect(world.removedMachineNames).toEqual(['flow-two']));
    expect(screen.getAllByRole('button', { name: 'removeEmbeddedAsset' })).toHaveLength(1);
});

test('shows an inline error when the initial load fails', async () => {
    const { actions, initialLoad, world } = createWorld({ failLoad: true });
    render(<EmbeddedAssetsApp initialLoad={initialLoad} actions={actions} tr={tr} onRequestClose={() => undefined} />);
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('read failed'));
    expect(world.errors).toEqual(['read failed']);
    expect(screen.queryByRole('status')).toBeNull();
});
