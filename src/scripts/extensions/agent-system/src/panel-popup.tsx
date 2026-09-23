import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { AgentSystemPanelApp } from './AgentSystemPanelApp';
import { createAgentSystemPanelController } from './AgentSystemPanelController';
import { CHAT_COMPLETION_PRESET_API_ID } from './AgentSystemPanelContract';
import { createRunHistoryController, type RunHistoryListInput } from './RunHistoryController';
import { createRunRetentionController } from './RunRetentionController';
import { readStateBinding, toggleStateBinding } from './state-binding';
import {
    deleteStateDeclaration,
    getStateDeclaration,
    listStateDeclarations,
    saveStateDeclaration,
} from './state-config-api';
import { createStateConfigController } from './state-config-controller';
import {
    deleteStateMachine,
    evaluateStateMachine,
    getStateMachine,
    listStateMachines,
    saveStateMachine,
} from './state-machine-api';
import { createMachineConfigController } from './state-machine-controller';
import {
    deleteStatePredicateSet,
    evaluateStatePredicateSet,
    getStatePredicateSet,
    listStatePredicateSets,
    saveStatePredicateSet,
} from './state-predicate-api';
import { createPredicateConfigController } from './state-predicate-controller';
import { resolveChatStateDeclaration } from './chat-state-declaration';
import {
    confirmAction,
    errorText,
    reportAgentSystemError,
    requireAgentApi,
    requireHostApi,
    requireSillyTavernContext,
} from './host-api';
import { translateAgentSystem as tr } from './i18n';
import {
    listSavedModelTargets,
    saveModelTargetAsLlmConnection,
    subscribeModelTargetChanges,
} from './model-target-connection';
import { openAgentRunTimelineDialog } from './run-timeline-panel';
import { loadSettings, patchSettings } from './settings-store';
import { downloadBlobWithRuntime } from '../../../file-export.js';
import { subscribeAgentProfilesChanged } from '../../../tauritavern/agent/agent-profile-events.js';
import { subscribeLlmConnectionsChanged } from '../../../tauritavern/agent/llm-connection-events.js';
import { resumeAgentRun } from '../../../tauritavern/agent/agent-run-retry.js';
import { isTauriEnv, openDialog } from '../../../../tauri-bridge.js';

let activePanel: HTMLDialogElement | null = null;

/**
 * The host's own file dialog, when there is one.
 *
 * A plain browser has no dialog that returns a path, and a path is the only
 * thing the backend can count tokens with, so this reports "no dialog" rather
 * than handing back a file it cannot name.
 */
function hostPickFilePath(): ((extensions: readonly string[]) => Promise<string | null>) | null {
    if (!isTauriEnv) {
        return null;
    }
    return async (extensions) => {
        const selected: unknown = await openDialog({
            multiple: false,
            directory: false,
            filters: [{ name: 'File', extensions: [...extensions] }],
        });
        return typeof selected === 'string' && selected.trim() ? selected : null;
    };
}

type SillyTavernPresetManager = {
    getAllPresets: () => string[];
    findPreset: (name: string) => unknown;
};

function listPresetOptions(): string[] {
    const context = requireSillyTavernContext() as {
        getPresetManager?: (apiId: string) => SillyTavernPresetManager | null | undefined;
    };
    const manager = context.getPresetManager?.(CHAT_COMPLETION_PRESET_API_ID);
    if (!manager) {
        throw new Error(tr('presetManagerUnavailable'));
    }
    return manager
        .getAllPresets()
        .map((name) => String(name || '').trim())
        .filter((name) => name && manager.findPreset(name) !== 'gui')
        .sort((a, b) => a.localeCompare(b));
}

async function currentChatRunFilter(): Promise<{ chatRef: TauriTavernChatRef; stableChatId: string }> {
    const chat = requireHostApi('chat');
    const chatRef = chat.current.ref();
    if (!plainObject(chatRef)) {
        throw new Error('agent.run_history_current_chat_invalid: current chat ref must be an object');
    }
    const stableChatIdValue = await chat.current.handle().stableId();
    if (typeof stableChatIdValue !== 'string') {
        throw new Error('agent.run_history_current_chat_invalid: stableChatId must be a string');
    }
    const stableChatId = stableChatIdValue.trim();
    if (!stableChatId) {
        throw new Error('agent.run_history_current_chat_invalid: stableChatId is required');
    }
    return { chatRef, stableChatId };
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function requireRetentionApi(): TauriTavernAgentRetentionApi {
    const agent = requireHostApi('agent');
    const api = agent.retention;
    if (typeof api?.readSettings !== 'function'
        || typeof api.updateSettings !== 'function'
        || typeof api.planPrune !== 'function'
        || typeof api.applyPrune !== 'function') {
        throw new Error(tr('hostAgentRetentionApiUnavailable'));
    }
    return api;
}

export function openAgentSystemPanel(): void {
    if (activePanel?.open) {
        activePanel.focus();
        return;
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('agentSystemElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('agentSystemDialogUnsupported'));
    }
    dialog.className = 'ttas-dialog';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');
    const mount = document.createElement('div');
    mount.className = 'ttas-popup-mount';
    dialog.appendChild(mount);
    document.body.appendChild(dialog);

    const runHistory = createRunHistoryController({
        listRuns: (input: RunHistoryListInput) => {
            const agent = requireHostApi('agent');
            return agent.listRuns(input);
        },
        currentChatRunFilter,
        async resumeRun(runId) {
            dialog.close();
            try {
                return await resumeAgentRun(runId);
            } catch (error) {
                reportAgentSystemError(error);
                throw error;
            }
        },
        openRun: (run) => {
            try {
                openAgentRunTimelineDialog(run);
            } catch (error) {
                console.error('[AgentSystem] Failed to open Agent run timeline', error);
                window.toastr?.error?.(errorText(error), tr('agentSystem'));
            }
        },
    });

    const runRetention = createRunRetentionController({
        getRetentionApi: requireRetentionApi,
        confirmAction,
        notifySuccess: (message) => window.toastr?.success?.(message, tr('agentSystem')),
        notifyWarning: (message) => window.toastr?.warning?.(message, tr('agentSystem')),
        tr,
    });

    const stateConfig = createStateConfigController({
        listDeclarations: listStateDeclarations,
        getDeclaration: getStateDeclaration,
        saveDeclaration: saveStateDeclaration,
        deleteDeclaration: deleteStateDeclaration,
        confirmAction,
        notifyError: reportAgentSystemError,
        notifySuccess: (message) => window.toastr?.success?.(message, tr('agentSystem')),
        downloadBlob: (blob, fileName) => downloadBlobWithRuntime(blob, fileName, {
            fallbackName: fileName,
        }),
        pickFilePath: hostPickFilePath(),
        readBinding: () => readStateBinding('declaration'),
        toggleBinding: (scope, name) => toggleStateBinding('declaration', scope, name),
        tr,
    });

    const machineConfig = createMachineConfigController({
        listMachines: listStateMachines,
        getMachine: getStateMachine,
        saveMachine: saveStateMachine,
        deleteMachine: deleteStateMachine,
        evaluate: evaluateStateMachine,
        confirmAction,
        notifyError: reportAgentSystemError,
        notifySuccess: (message) => window.toastr?.success?.(message, tr('agentSystem')),
        downloadBlob: (blob, fileName) => downloadBlobWithRuntime(blob, fileName, {
            fallbackName: fileName,
        }),
        readBinding: () => readStateBinding('machine'),
        toggleBinding: (scope, name) => toggleStateBinding('machine', scope, name),
        tr,
    });

    const predicateConfig = createPredicateConfigController({
        listSets: listStatePredicateSets,
        getSet: getStatePredicateSet,
        saveSet: saveStatePredicateSet,
        deleteSet: deleteStatePredicateSet,
        evaluate: evaluateStatePredicateSet,
        confirmAction,
        notifyError: reportAgentSystemError,
        notifySuccess: (message) => window.toastr?.success?.(message, tr('agentSystem')),
        downloadBlob: (blob, fileName) => downloadBlobWithRuntime(blob, fileName, {
            fallbackName: fileName,
        }),
        readBinding: () => readStateBinding('predicates'),
        toggleBinding: (scope, name) => toggleStateBinding('predicates', scope, name),
        tr,
    });

    const controller = createAgentSystemPanelController({
        loadSettings,
        patchSettings,
        getProfilesApi: () => requireAgentApi().profiles,
        listTools: async () => {
            const api = requireAgentApi().tools;
            if (typeof api?.list !== 'function') {
                throw new Error(tr('hostAgentToolApiUnavailable'));
            }
            const result = await api.list();
            return {
                tools: result.tools,
                diagnostics: Array.isArray(result.diagnostics) ? result.diagnostics : [],
            };
        },
        listPresetOptions,
        listModelTargets: listSavedModelTargets,
        listStateDeclarations: () => listStateDeclarations(),
        loadStateDeclaration: (name) => getStateDeclaration(name),
        resolveChatStateDeclaration,
        saveModelTargetConnection: saveModelTargetAsLlmConnection,
        subscribeProfilesChanged: subscribeAgentProfilesChanged,
        subscribeModelTargetsChanged: subscribeModelTargetChanges,
        subscribeLlmConnectionsChanged,
        confirmAction,
        downloadBlob: (blob, fileName) => downloadBlobWithRuntime(blob, fileName, {
            fallbackName: 'agent-profile.json',
        }),
        notifyError: reportAgentSystemError,
        notifyWarning: (message) => window.toastr?.warning?.(message),
        notifySuccess: (message) => window.toastr?.success?.(message),
        onRunsTabActivated: () => {
            void runHistory.refresh();
            void runRetention.refresh();
        },
        onStateTabActivated: () => {
            void stateConfig.init();
        },
        onMachinesTabActivated: () => {
            void machineConfig.init();
        },
        onPredicatesTabActivated: () => {
            void predicateConfig.init();
        },
        tr,
    });

    const root = createRoot(mount);
    let disposed = false;
    const cleanup = () => {
        if (disposed) {
            return;
        }
        disposed = true;
        controller.dispose();
        stateConfig.dispose();
        machineConfig.dispose();
        runHistory.dispose();
        runRetention.dispose();
        root.unmount();
        dialog.remove();
        if (activePanel === dialog) {
            activePanel = null;
        }
    };
    // Closing is the last chance to keep unsaved work, and no other path asks.
    let closing = false;
    const requestClose = () => {
        if (closing) {
            return;
        }
        closing = true;
        void (async () => {
            try {
                const answers = await Promise.all([
                    stateConfig.confirmPendingEdits(),
                    machineConfig.confirmPendingEdits(),
                    predicateConfig.confirmPendingEdits(),
                ]);
                if (answers.every(Boolean) && !disposed) {
                    dialog.close();
                }
            } finally {
                closing = false;
            }
        })();
    };

    dialog.addEventListener('close', cleanup, { once: true });
    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        requestClose();
    });

    root.render(
        <StrictMode>
            <AgentSystemPanelApp
                controller={controller}
                runHistory={runHistory}
                runRetention={runRetention}
                stateConfig={stateConfig}
                machineConfig={machineConfig}
                predicateConfig={predicateConfig}
                tr={tr}
                onRequestClose={requestClose}
            />
        </StrictMode>,
    );
    activePanel = dialog;

    try {
        dialog.showModal();
    } catch (error) {
        cleanup();
        throw error;
    }

    // The composition root owns one initialization. The controller reports
    // failures; the asynchronous rethrow also feeds the dev-log capture.
    void controller.init().catch((error: unknown) => {
        queueMicrotask(() => {
            throw error;
        });
    });
}
