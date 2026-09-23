import type { AgentSystemPanelControllerDeps } from './AgentSystemPanelContract';
import { defaultProfile } from './profile-model';
import { createStateConfigController, type StateConfigController } from './state-config-controller';
import type { StateDeclaration } from './state-config-model';
import { createMachineConfigController, type MachineConfigController } from './state-machine-controller';
import { createPredicateConfigController, type PredicateConfigController } from './state-predicate-controller';
import type { StateBindingView } from './state-binding';
import type { AgentSystemSettings } from './settings-store';

type ProfileListResult = Awaited<ReturnType<TauriTavernAgentProfilesApi['list']>>;

/**
 * What the binding stubs report: a chat with nothing bound to it and no active
 * entity. They exist so the editors render their binding buttons; the binding
 * itself belongs to the page runtime, which a unit test has no reason to model.
 */
const NO_BINDING: StateBindingView = {
    chatAvailable: true,
    entityScope: null,
    chatName: '',
    entityName: '',
};

function formatParam(value: unknown): string {
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
        return String(value);
    }
    return JSON.stringify(value) ?? '';
}

export function tr(key: string, params: Record<string, unknown> = {}): string {
    return [key, ...Object.entries(params).map(([name, value]) => `${name}=${formatParam(value)}`)].join(' ');
}

export function settings(editingProfileId: string): AgentSystemSettings {
    return {
        agentModeEnabled: true,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId,
        activeTab: 'profiles',
        runTimelineHeightPx: null,
        defaultSceneSeedVersion: 0,
    };
}

export function summary(profile: TauriTavernAgentProfileDefinition): TauriTavernAgentProfileSummary {
    return {
        id: profile.id,
        displayName: profile.displayName,
        ...(profile.description !== undefined ? { description: profile.description } : {}),
        directRunnable: profile.run.directRunnable,
    };
}

export function healthyProfile(profileId: string): TauriTavernAgentProfileHealth {
    return {
        profileId,
        previewAvailable: true,
        promptAssemblyAvailable: true,
        directRunAvailable: true,
        subAgentAvailable: true,
        diagnostics: [],
    };
}

export function createPanelWorld(selectedProfile = defaultProfile()) {
    const definitions = new Map<string, TauriTavernAgentProfileDefinition>();
    const builtin = defaultProfile();
    definitions.set(builtin.id, builtin);
    definitions.set(selectedProfile.id, selectedProfile);

    const state = {
        settings: settings(selectedProfile.id),
        definitions,
        listResults: [] as ProfileListResult[],
        listCount: 0,
        repairs: [] as Array<{ profileId: string; action: 'delete' | 'normalizeIdentity' }>,
        confirmations: [] as string[],
        confirm: true,
        warnings: [] as string[],
        errors: [] as string[],
        saves: [] as TauriTavernAgentProfileDefinition[],
        presetOptions: [] as string[],
        health: healthyProfile(selectedProfile.id),
        resolveError: null as Error | null,
        profilesListener: null as (() => void) | null,
        subscribers: { profiles: 0, modelTargets: 0, llmConnections: 0 },
    };

    const profilesApi: TauriTavernAgentProfilesApi = {
        list: () => {
            state.listCount += 1;
            const queued = state.listResults.shift();
            return Promise.resolve(queued ?? {
                profiles: [...state.definitions.values()].map(summary),
                issues: [],
            });
        },
        load: (input) => {
            const profileId = typeof input === 'string' ? input : input.profileId;
            const profile = state.definitions.get(profileId);
            return Promise.resolve({ profile: profile ? structuredClone(profile) : null });
        },
        diagnose: () => Promise.resolve(state.health),
        resolveSystemPrompt: () => state.resolveError
            ? Promise.reject(state.resolveError)
            : Promise.resolve({ agentSystemPrompt: 'Resolved Agent system prompt.' }),
        repairFile: (input) => {
            state.repairs.push(input);
            return Promise.resolve();
        },
        retargetPresetRefs: () => Promise.resolve({ updated: 0, profileIds: [] }),
        save: (input) => {
            const profile = 'profile' in input ? input.profile : input;
            state.saves.push(profile);
            state.definitions.set(profile.id, structuredClone(profile));
            return Promise.resolve();
        },
        delete: (input) => {
            state.definitions.delete(typeof input === 'string' ? input : input.profileId);
            return Promise.resolve();
        },
    };

    const declarations = new Map<string, StateDeclaration>();
    const stateConfig = stateConfigFor(declarations, () => Promise.resolve(state.confirm));

    const deps: AgentSystemPanelControllerDeps = {
        loadSettings: () => Promise.resolve(state.settings),
        patchSettings: (current, patch) => {
            state.settings = { ...current, ...patch };
            return Promise.resolve(state.settings);
        },
        getProfilesApi: () => profilesApi,
        listTools: () => Promise.resolve({ tools: [], diagnostics: [] }),
        listPresetOptions: () => state.presetOptions,
        listModelTargets: () => [],
        listStateDeclarations: () => Promise.resolve([...declarations.keys()]),
        loadStateDeclaration: (name) => {
            const found = declarations.get(name);
            return found ? Promise.resolve(found) : Promise.reject(new Error(`missing declaration ${name}`));
        },
        resolveChatStateDeclaration: () => Promise.resolve(null),
        saveModelTargetConnection: () => Promise.resolve(),
        subscribeProfilesChanged: (listener) => {
            state.subscribers.profiles += 1;
            state.profilesListener = listener;
            return () => {
                state.subscribers.profiles -= 1;
            };
        },
        subscribeModelTargetsChanged: () => {
            state.subscribers.modelTargets += 1;
            return () => {
                state.subscribers.modelTargets -= 1;
            };
        },
        subscribeLlmConnectionsChanged: () => {
            state.subscribers.llmConnections += 1;
            return () => {
                state.subscribers.llmConnections -= 1;
            };
        },
        confirmAction: (message) => {
            state.confirmations.push(message);
            return Promise.resolve(state.confirm);
        },
        downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
        notifyError: (error) => {
            state.errors.push(error instanceof Error ? error.message : 'unknown error');
        },
        notifyWarning: (message) => {
            state.warnings.push(message);
        },
        notifySuccess: () => undefined,
        onRunsTabActivated: () => undefined,
        onStateTabActivated: () => {
            void stateConfig.init();
        },
        onMachinesTabActivated: () => undefined,
        onPredicatesTabActivated: () => undefined,
        tr,
    };
    return { deps, state, stateConfig, declarations };
}

/** A predicate editor behind the tab, stubbed to an empty store. */
export function predicateConfigFor(): PredicateConfigController {
    return createPredicateConfigController({
        listSets: () => Promise.resolve([]),
        getSet: (name) => Promise.reject(new Error(`missing predicate set ${name}`)),
        saveSet: (name) => Promise.reject(new Error(`unused save ${name}`)),
        deleteSet: (name) => Promise.reject(new Error(`unused delete ${name}`)),
        evaluate: () => Promise.reject(new Error('unused evaluate')),
        confirmAction: () => Promise.resolve(false),
        notifyError: () => undefined,
        notifySuccess: () => undefined,
        downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
        readBinding: () => Promise.resolve(NO_BINDING),
        toggleBinding: () => Promise.resolve(false),
        tr,
    });
}

/** A machine editor behind the tab, stubbed to an empty store. */
export function machineConfigFor(): MachineConfigController {
    return createMachineConfigController({
        listMachines: () => Promise.resolve([]),
        getMachine: (name) => Promise.reject(new Error(`missing machine ${name}`)),
        saveMachine: (name) => Promise.reject(new Error(`unused save ${name}`)),
        deleteMachine: (name) => Promise.reject(new Error(`unused delete ${name}`)),
        evaluate: () => Promise.reject(new Error('unused evaluate')),
        confirmAction: () => Promise.resolve(false),
        notifyError: () => undefined,
        notifySuccess: () => undefined,
        downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
        readBinding: () => Promise.resolve(NO_BINDING),
        toggleBinding: () => Promise.resolve(false),
        tr,
    });
}

/** A state declaration store behind the editor, for the tab's own tests. */
export function stateConfigFor(
    declarations: Map<string, StateDeclaration>,
    confirmAction: () => Promise<boolean> = () => Promise.resolve(false),
): StateConfigController {
    return createStateConfigController({
        listDeclarations: () => Promise.resolve([...declarations.keys()]),
        getDeclaration: (name) => {
            const found = declarations.get(name);
            return found ? Promise.resolve(found) : Promise.reject(new Error(`missing declaration ${name}`));
        },
        saveDeclaration: (name, declaration) => {
            declarations.set(name, declaration);
            return Promise.resolve();
        },
        deleteDeclaration: (name) => {
            declarations.delete(name);
            return Promise.resolve();
        },
        confirmAction,
        notifyError: () => undefined,
        notifySuccess: () => undefined,
        downloadBlob: () => Promise.resolve({ mode: 'browser-download', completed: true }),
        pickFilePath: null,
        readBinding: () => Promise.resolve(NO_BINDING),
        toggleBinding: () => Promise.resolve(false),
        tr,
    });
}
