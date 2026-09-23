import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { AgentSystemPanelApp } from './AgentSystemPanelApp';
import {
    createAgentSystemPanelController,
    type AgentSystemPanelController,
} from './AgentSystemPanelController';
import { createRunHistoryController } from './RunHistoryController';
import { createRunRetentionController } from './RunRetentionController';
import { defaultProfile } from './profile-model';
import type { AgentSystemSettings } from './settings-store';
import {
    createPanelWorld,
    machineConfigFor,
    predicateConfigFor,
    settings,
    stateConfigFor,
    summary,
    tr,
} from './PanelTestWorld';

const disposables: Array<{ dispose: () => void }> = [];

function renderPanel(controller: AgentSystemPanelController, stateConfig = stateConfigFor(new Map())): void {
    const runHistory = createRunHistoryController({
        listRuns: () => Promise.resolve({ runs: [] }),
        currentChatRunFilter: () => Promise.resolve({
            chatRef: { kind: 'group', chatId: 'group' },
            stableChatId: 'stable-group',
        }),
        openRun: () => undefined,
        resumeRun: () => Promise.resolve(),
    });
    const runRetention = createRunRetentionController({
        getRetentionApi: () => ({
            readSettings: () => Promise.resolve({
                autoPruneEnabled: false,
                keepRecentTerminalRuns: 100,
                keepFullRecentRuns: 20,
            }),
            updateSettings: () => Promise.reject(new Error('unused')),
            planPrune: () => Promise.reject(new Error('unused')),
            applyPrune: () => Promise.reject(new Error('unused')),
        }),
        confirmAction: () => Promise.resolve(false),
        notifySuccess: () => undefined,
        notifyWarning: () => undefined,
        tr: (key: string) => key,
    });
    const machineConfig = machineConfigFor();
    const predicateConfig = predicateConfigFor();
    disposables.push(controller, runHistory, runRetention, stateConfig, machineConfig, predicateConfig);
    render(
        <AgentSystemPanelApp
            controller={controller}
            runHistory={runHistory}
            runRetention={runRetention}
            stateConfig={stateConfig}
            machineConfig={machineConfig}
            predicateConfig={predicateConfig}
            tr={tr}
            onRequestClose={() => undefined}
        />,
    );
}

afterEach(() => {
    cleanup();
    disposables.splice(0).forEach((disposable) => disposable.dispose());
});

test('repairs profile list issues and renders the refreshed list', async () => {
    const { deps, state } = createPanelWorld();
    state.listResults.push(
        {
            profiles: [summary(defaultProfile())],
            issues: [
                {
                    profileId: 'broken-json',
                    fileName: 'broken-json.json',
                    kind: 'invalidJson',
                    recommendedAction: 'delete',
                    message: 'Invalid JSON',
                },
                {
                    profileId: 'bad-schema',
                    fileName: 'bad-schema.json',
                    kind: 'invalidFileIdentity',
                    recommendedAction: 'normalizeIdentity',
                    message: 'Invalid profile kind',
                },
            ],
        },
        {
            profiles: [summary(defaultProfile()), {
                id: 'bad-schema',
                displayName: 'bad-schema',
                directRunnable: true,
            }],
            issues: [],
        },
    );
    const controller = createAgentSystemPanelController(deps);
    renderPanel(controller);

    await act(async () => controller.init());

    expect(state.repairs).toEqual([
        { profileId: 'broken-json', action: 'delete' },
        { profileId: 'bad-schema', action: 'normalizeIdentity' },
    ]);
    expect(state.confirmations).toHaveLength(1);
    expect(state.warnings).toEqual([
        'deletedCorruptAgentProfile id=broken-json',
        'normalizedAgentProfileIdentity id=bad-schema',
    ]);
    expect(screen.getAllByText('bad-schema').length).toBeGreaterThan(0);
});

test('keeps a profile editable when its prompt preview fails', async () => {
    const profile = defaultProfile('dangling-writer');
    profile.displayName = 'Dangling Writer';
    profile.preset = {
        mode: 'ref',
        ref: { apiId: 'openai', name: 'Missing Writer Preset' },
        required: true,
    };
    const { deps, state } = createPanelWorld(profile);
    state.presetOptions = ['Missing Writer Preset'];
    state.resolveError = new Error('agent.profile_preset_missing');
    state.health = {
        profileId: profile.id,
        previewAvailable: true,
        promptAssemblyAvailable: false,
        directRunAvailable: false,
        subAgentAvailable: false,
        diagnostics: [{
            code: 'agent.profile_preset_missing',
            severity: 'error',
            path: '$.preset.ref.name',
            message: 'required preset is missing',
            resource: { kind: 'preset', apiId: 'openai', name: 'Missing Writer Preset' },
            blocks: ['promptAssembly', 'directRun', 'subAgent'],
            repairActions: ['selectPreset'],
        }],
    };
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);
    await act(async () => controller.init());

    await waitFor(() => expect(screen.getByText('agentProfilePresetMissing name=Missing Writer Preset')).toBeDefined());
    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Editable Writer');

    expect(displayName.value).toBe('Editable Writer');
    expect(controller.getSnapshot().profilePreviewError).toBe('agent.profile_preset_missing');
});

test('expands the access grid from the chosen declaration', async () => {
    const profile = defaultProfile('writer');
    const { deps, declarations } = createPanelWorld(profile);
    declarations.set('scene', {
        fields: [
            { pattern: '环境/日期', label: '日期' },
            { pattern: '环境/时间', label: '时间' },
        ],
    });
    const controller = createAgentSystemPanelController(deps);
    await act(async () => controller.init());

    expect(controller.getSnapshot().stateDeclarationChoice).toBe('scene');
    await act(async () => controller.expandStateAccessFromDeclaration());

    const rows = controller.getSnapshot().draft.stateAccess?.entries ?? [];
    expect(rows.map((row) => row.pattern)).toEqual(['环境/日期', '环境/时间']);
    expect(rows.every((row) => !row.inject && !row.visible && !row.writable)).toBe(true);

    // Expanding again must not stack duplicate rows the backend would refuse.
    await act(async () => controller.expandStateAccessFromDeclaration());
    expect((controller.getSnapshot().draft.stateAccess?.entries ?? []).map((row) => row.pattern)).toEqual([
        '环境/日期',
        '环境/时间',
    ]);
});

test('expands the access grid from the chat-bound declaration', async () => {
    const profile = defaultProfile('writer');
    const { deps } = createPanelWorld(profile);
    deps.resolveChatStateDeclaration = () => Promise.resolve({
        name: 'bound-scene',
        fields: [
            { pattern: '环境/日期', label: '日期' },
            { pattern: '环境/时间', label: '时间' },
        ],
    });
    const controller = createAgentSystemPanelController(deps);
    await act(async () => controller.init());

    await act(async () => controller.expandStateAccessFromChatDeclaration());

    expect((controller.getSnapshot().draft.stateAccess?.entries ?? []).map((row) => row.pattern)).toEqual([
        '环境/日期',
        '环境/时间',
    ]);
    expect(controller.getSnapshot().chatDeclarationName).toBe('bound-scene');
});

test('warns when the chat has no bound declaration to expand', async () => {
    const { deps, state } = createPanelWorld(defaultProfile('writer'));
    const controller = createAgentSystemPanelController(deps);
    disposables.push(controller);
    await act(async () => controller.init());

    await act(async () => controller.expandStateAccessFromChatDeclaration());

    expect(state.warnings).toEqual(['stateAccessChatDeclarationMissing']);
    expect(controller.getSnapshot().draft.stateAccess?.entries ?? []).toEqual([]);
});

test('supplemental catalog failures do not block profile editing', async () => {
    const profile = defaultProfile('writer');
    const { deps, state } = createPanelWorld(profile);
    deps.listPresetOptions = () => { throw new Error('preset catalog unavailable'); };
    deps.listModelTargets = () => { throw new Error('model catalog unavailable'); };
    deps.listTools = () => Promise.reject(new Error('tool catalog unavailable'));
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);

    await act(async () => controller.init());

    expect(controller.getSnapshot().initialized).toBe(true);
    expect(state.subscribers).toEqual({ profiles: 1, modelTargets: 1, llmConnections: 1 });
    expect(state.errors).toEqual(expect.arrayContaining([
        'preset catalog unavailable',
        'model catalog unavailable',
        'tool catalog unavailable',
    ]));
    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Still editable');
    expect(displayName.value).toBe('Still editable');
});

test('tool catalog keeps available disabled tools visible', async () => {
    const profile = defaultProfile('writer');
    profile.tools.allow = [];
    const { deps } = createPanelWorld(profile);
    deps.listTools = () => Promise.resolve({
        tools: [{
            id: 'builtin:workspace.read_file',
            nativeName: 'workspace.read_file',
            title: 'Read workspace file',
            description: 'Reads a workspace file.',
            inputSchema: {},
            source: 'builtin',
        }],
        diagnostics: [],
    });
    const controller = createAgentSystemPanelController(deps);
    renderPanel(controller);

    await act(async () => controller.init());

    expect(screen.getAllByText('Read workspace file').length).toBeGreaterThan(0);
});

test('failed profile selection keeps the previous selection and draft together', async () => {
    const writer = defaultProfile('writer');
    const reviewer = defaultProfile('reviewer');
    reviewer.displayName = 'Reviewer';
    const { deps, state } = createPanelWorld(writer);
    state.definitions.set(reviewer.id, reviewer);
    const patchSettings = deps.patchSettings;
    deps.patchSettings = (current, patch) => patch.editingProfileId === reviewer.id
        ? Promise.reject(new Error('settings write failed'))
        : patchSettings(current, patch);
    const controller = createAgentSystemPanelController(deps);
    disposables.push(controller);
    await controller.init();

    await expect(controller.selectProfile(reviewer.id)).rejects.toThrow('settings write failed');

    expect(controller.getSnapshot().editingProfileId).toBe(writer.id);
    expect(controller.getSnapshot().draft.id).toBe(writer.id);
    expect(state.errors).toContain('settings write failed');
});

test('a saved profile remains committed when its list refresh fails', async () => {
    const profile = defaultProfile('writer');
    const { deps, state } = createPanelWorld(profile);
    const controller = createAgentSystemPanelController(deps);
    disposables.push(controller);
    await controller.init();
    deps.getProfilesApi().list = () => Promise.reject(new Error('profile list refresh failed'));
    controller.setIdentityField('displayName', 'Saved Writer');

    await controller.saveProfile();

    expect(state.definitions.get(profile.id)?.displayName).toBe('Saved Writer');
    expect(controller.getSnapshot().draft.displayName).toBe('Saved Writer');
    expect(state.errors).toContain('profile list refresh failed');
});

test('a dirty draft rejects an external overwrite and save', async () => {
    const profile = defaultProfile('writer');
    profile.preset = {
        mode: 'ref',
        ref: { apiId: 'openai', name: 'Old Preset' },
        required: true,
    };
    const { deps, state } = createPanelWorld(profile);
    state.presetOptions = ['Old Preset', 'New Preset'];
    const controller = createAgentSystemPanelController(deps);
    const user = userEvent.setup();
    renderPanel(controller);
    await act(async () => controller.init());

    const displayName = screen.getByRole<HTMLInputElement>('textbox', { name: 'displayName' });
    await user.clear(displayName);
    await user.type(displayName, 'Unsaved local edit');
    const changed = structuredClone(profile);
    if (changed.preset.mode !== 'ref' || !changed.preset.ref) {
        throw new Error('expected a preset reference');
    }
    changed.preset.ref.name = 'New Preset';
    state.definitions.set(profile.id, changed);

    act(() => state.profilesListener?.());
    await waitFor(() => expect(controller.getSnapshot().externalProfileChangePending).toBe(true));
    act(() => state.profilesListener?.());
    await waitFor(() => expect(state.listCount).toBe(3));

    expect(state.warnings).toEqual(['agentProfileExternalChangePending']);
    await expect(controller.saveProfile()).rejects.toThrow('agentProfileExternalChangeSaveBlocked');
    expect(state.saves).toEqual([]);
});

test('dispose and failed subscription setup leave no ghost listeners', async () => {
    const pending = createPanelWorld();
    const deferredSettings: { resolve: ((value: AgentSystemSettings) => void) | null } = { resolve: null };
    pending.deps.loadSettings = () => new Promise((resolve) => {
        deferredSettings.resolve = resolve;
    });
    const pendingController = createAgentSystemPanelController(pending.deps);
    disposables.push(pendingController);
    const initPromise = pendingController.init();
    await waitFor(() => expect(deferredSettings.resolve).not.toBeNull());
    pendingController.dispose();
    deferredSettings.resolve?.(settings('default-writer'));
    await initPromise;

    expect(pending.state.subscribers).toEqual({ profiles: 0, modelTargets: 0, llmConnections: 0 });
    expect(pendingController.getSnapshot().initialized).toBe(false);

    const failed = createPanelWorld();
    failed.deps.subscribeLlmConnectionsChanged = () => {
        throw new Error('LLM subscription failed');
    };
    const failedController = createAgentSystemPanelController(failed.deps);
    disposables.push(failedController);

    await expect(failedController.init()).rejects.toThrow('LLM subscription failed');
    expect(failed.state.subscribers).toEqual({ profiles: 0, modelTargets: 0, llmConnections: 0 });
    expect(failedController.getSnapshot().initialized).toBe(false);
});

test('the state access section says what each switch does', async () => {
    const { deps } = createPanelWorld();
    const controller = createAgentSystemPanelController(deps);
    renderPanel(controller);
    await act(async () => controller.init());

    expect(screen.getByText('stateAccessGuideTitle')).toBeTruthy();
    // The three switches are the whole feature and none is self-explanatory.
    expect(screen.getByText('stateAccessGuideInject')).toBeTruthy();
    expect(screen.getByText('stateAccessGuideVisible')).toBeTruthy();
    expect(screen.getByText('stateAccessGuideWritable')).toBeTruthy();
    // An empty grid constrains nothing; the first row turns it into the policy.
    expect(screen.getByText('stateAccessGuideGate')).toBeTruthy();
});

test('the state tab opens the declaration editor and creates a declaration', async () => {
    const world = createPanelWorld();
    const controller = createAgentSystemPanelController(world.deps);
    const user = userEvent.setup();
    renderPanel(controller, world.stateConfig);
    await act(async () => controller.init());

    await user.click(screen.getByRole('button', { name: 'stateDeclarations' }));
    await waitFor(() => expect(screen.getByText('stateDeclarationPick')).toBeTruthy());
    expect(screen.getByText('stateDeclarationNone')).toBeTruthy();
    const name = screen.getByRole<HTMLInputElement>('textbox', { name: 'stateDeclarationName' });
    await user.clear(name);
    await user.type(name, 'scene');
    await user.click(screen.getByRole('button', { name: 'stateDeclarationCreate' }));
    await waitFor(() => expect(screen.getByText('stateDeclarationFieldsHint')).toBeTruthy());
    // Nothing is stored under the new name yet, so the editor says so.
    expect(screen.getByText('stateDeclarationUnsaved')).toBeTruthy();
});
