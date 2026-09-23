import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { EmbeddedAssetsApp } from './EmbeddedAssetsApp';
import {
    buildSkillOptions,
    type EmbeddedAssetsActions,
    type EmbeddedAssetsInitial,
    type EmbeddedAssetTargetInput,
    type SkillOption,
} from './EmbeddedAssetsContract';
import {
    embedMachine,
    embedPredicateSet,
    embedProfile,
    embedSkill,
    embedState,
    readEmbeddedAssets,
    removeEmbeddedMachine,
    removeEmbeddedPredicateSet,
    removeEmbeddedProfile,
    removeEmbeddedSkill,
    removeEmbeddedState,
} from './embedded-assets';
import { listStateDeclarations } from './state-config-api';
import { listStateMachines } from './state-machine-api';
import { listStatePredicateSets } from './state-predicate-api';
import { reportAgentSystemError, requireAgentApi, requireSkillApi } from './host-api';
import { translateAgentSystem as tr } from './i18n';

let activePanel: HTMLDialogElement | null = null;

async function loadInitial(target: EmbeddedAssetTargetInput): Promise<EmbeddedAssetsInitial> {
    const [profileResult, skills, declarations, machines, predicates, embedded] = await Promise.all([
        requireAgentApi().profiles.list(),
        requireSkillApi().list({ scope: { kind: 'all' } }),
        listStateDeclarations(),
        listStateMachines(),
        listStatePredicateSets(),
        Promise.resolve(readEmbeddedAssets(target)),
    ]);
    return {
        targetInfo: embedded.target,
        profiles: Array.isArray(profileResult?.profiles) ? profileResult.profiles : [],
        skills: buildSkillOptions(skills),
        states: declarations,
        machines,
        predicates,
        embeddedProfiles: embedded.profiles,
        embeddedSkills: embedded.skills,
        embeddedStates: embedded.states,
        embeddedMachines: embedded.machines,
        embeddedPredicates: embedded.predicates,
    };
}

function createActions(target: EmbeddedAssetTargetInput): EmbeddedAssetsActions {
    return {
        embedProfile: async (profileId) => {
            const result = await requireAgentApi().profiles.load({ profileId });
            const profile = result?.profile ?? null;
            if (!profile) {
                throw new Error(tr('agentProfileNotFound', { id: profileId }));
            }
            await embedProfile(target, profile);
            return profile.id;
        },
        embedSkill: (skill: SkillOption) => embedSkill(target, { scope: skill.scope, name: skill.name }),
        embedState: (stateName) => embedState(target, stateName),
        embedMachine: (machineName) => embedMachine(target, machineName),
        embedPredicateSet: (setName) => embedPredicateSet(target, setName),
        removeProfile: (profileId) => removeEmbeddedProfile(target, profileId),
        removeSkill: (skillName) => removeEmbeddedSkill(target, skillName),
        removeState: (stateName) => removeEmbeddedState(target, stateName),
        removeMachine: (machineName) => removeEmbeddedMachine(target, machineName),
        removePredicateSet: (setName) => removeEmbeddedPredicateSet(target, setName),
        readEmbedded: () => readEmbeddedAssets(target),
        toastSuccess: (message) => {
            window.toastr?.success?.(message);
        },
        reportError: reportAgentSystemError,
    };
}

export function openEmbeddedAssetsPanel(target: EmbeddedAssetTargetInput): void {
    if (activePanel?.open) {
        activePanel.focus();
        return;
    }
    if (typeof HTMLDialogElement === 'undefined') {
        throw new Error(tr('agentAssetsElementUnsupported'));
    }

    const dialog = document.createElement('dialog');
    if (typeof dialog.showModal !== 'function') {
        throw new Error(tr('agentAssetsDialogUnsupported'));
    }
    dialog.className = 'ttas-dialog ttas-embed-dialog';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');

    const mount = document.createElement('div');
    mount.className = 'ttas-popup-mount ttas-embed-popup-mount';
    dialog.appendChild(mount);
    document.body.appendChild(dialog);

    const root = createRoot(mount);
    let disposed = false;
    const cleanup = () => {
        if (disposed) {
            return;
        }
        disposed = true;
        root.unmount();
        dialog.remove();
        if (activePanel === dialog) {
            activePanel = null;
        }
    };
    dialog.addEventListener('close', cleanup, { once: true });
    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        dialog.close();
    });

    root.render(
        <StrictMode>
            <EmbeddedAssetsApp
                initialLoad={loadInitial(target)}
                actions={createActions(target)}
                tr={tr}
                onRequestClose={() => dialog.close()}
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
}
