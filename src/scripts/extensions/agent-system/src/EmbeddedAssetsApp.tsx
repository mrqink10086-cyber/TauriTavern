import { useEffect, useState } from 'react';

import { AGENT_TOGGLE_ICON } from './agent-icon';
import { DEFAULT_PROFILE_ID } from './constants';
import {
    type EmbeddedAssetsActions,
    type EmbeddedAssetsInitial,
    type EmbeddedAssetsRead,
    embeddedMachineSubtitle,
    embeddedPredicateSubtitle,
    embeddedSkillSubtitle,
    embeddedStateSubtitle,
    profileDisplayName,
    skillOptionLabel,
} from './EmbeddedAssetsContract';
import { EmbedAssetSection, EmbeddedAssetGroup, type EmbeddedEntry } from './EmbeddedAssetSections';
import type { AgentSystemMessageKey, AgentSystemTr } from './i18n';

export type EmbeddedAssetsAppProps = {
    initialLoad: Promise<EmbeddedAssetsInitial>;
    actions: EmbeddedAssetsActions;
    tr: AgentSystemTr;
    onRequestClose: () => void;
};

/**
 * Selections and the lists they point at, plus the target this panel is about.
 *
 * Every kind is here twice: what can be carried (`profiles`…`predicates`) and
 * what already is (`embeddedStates`…). The two lists are what the panel is for.
 */
type PanelState = Omit<EmbeddedAssetsInitial, 'targetInfo'> & {
    targetInfo: EmbeddedAssetsInitial['targetInfo'] | null;
    initialized: boolean;
    loading: boolean;
    saving: boolean;
    error: string;
    selectedProfileId: string;
    selectedSkillKey: string;
    selectedStateName: string;
    selectedMachineName: string;
    selectedPredicateName: string;
};

const EMPTY_LISTS: Omit<PanelState, 'initialized' | 'loading' | 'saving' | 'error' | 'targetInfo'> = {
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
    selectedProfileId: '',
    selectedSkillKey: '',
    selectedStateName: '',
    selectedMachineName: '',
    selectedPredicateName: '',
};

const INITIAL_STATE: PanelState = {
    ...EMPTY_LISTS,
    targetInfo: null,
    initialized: false,
    loading: true,
    saving: false,
    error: '',
};

function embeddableProfilesOf(state: PanelState): TauriTavernAgentProfileSummary[] {
    return state.profiles.filter((profile) => profile.id !== DEFAULT_PROFILE_ID);
}

/**
 * Point every selection at something that exists.
 *
 * The lists change under the panel after every mutation, so a selection that
 * names a deleted entry falls back to the first one rather than dangling.
 */
function withSyncedSelections(state: PanelState): PanelState {
    const embeddable = embeddableProfilesOf(state);
    const pick = (current: string, known: string[], first: string | undefined): string => (
        known.includes(current) ? current : (first ?? '')
    );
    const selectedProfileId = pick(state.selectedProfileId, embeddable.map((profile) => profile.id), embeddable[0]?.id);
    const selectedSkillKey = pick(state.selectedSkillKey, state.skills.map((skill) => skill.key), state.skills[0]?.key);
    const selectedStateName = pick(state.selectedStateName, state.states, state.states[0]);
    const selectedMachineName = pick(state.selectedMachineName, state.machines, state.machines[0]);
    const selectedPredicateName = pick(state.selectedPredicateName, state.predicates, state.predicates[0]);

    if (selectedProfileId === state.selectedProfileId
        && selectedSkillKey === state.selectedSkillKey
        && selectedStateName === state.selectedStateName
        && selectedMachineName === state.selectedMachineName
        && selectedPredicateName === state.selectedPredicateName) {
        return state;
    }
    return {
        ...state,
        selectedProfileId,
        selectedSkillKey,
        selectedStateName,
        selectedMachineName,
        selectedPredicateName,
    };
}

export function EmbeddedAssetsApp({ initialLoad, actions, tr, onRequestClose }: EmbeddedAssetsAppProps) {
    const [state, setState] = useState<PanelState>(INITIAL_STATE);

    // The composition root starts the load once per dialog; this effect only
    // subscribes to that promise, so StrictMode cannot duplicate Host reads.
    useEffect(() => {
        let live = true;
        initialLoad.then(
            (data) => {
                if (!live) {
                    return;
                }
                setState((current) => withSyncedSelections({
                    ...current,
                    ...data,
                    initialized: true,
                    loading: false,
                    error: '',
                }));
            },
            (error: unknown) => {
                if (!live) {
                    return;
                }
                const message = actions.reportError(error);
                setState((current) => ({ ...current, loading: false, error: message }));
            },
        );
        return () => {
            live = false;
        };
    }, [initialLoad, actions]);

    function applyEmbedded(embedded: EmbeddedAssetsRead): void {
        setState((current) => withSyncedSelections({
            ...current,
            embeddedProfiles: embedded.profiles,
            embeddedSkills: embedded.skills,
            embeddedStates: embedded.states,
            embeddedMachines: embedded.machines,
            embeddedPredicates: embedded.predicates,
        }));
    }

    // User-triggered failures are reported (inline + toastr) and rethrown so
    // the dev-log capture still observes them, matching prior semantics.
    async function runAssetAction(action: () => Promise<void>): Promise<void> {
        setState((current) => ({ ...current, saving: true, error: '' }));
        try {
            await action();
            applyEmbedded(actions.readEmbedded());
        } catch (error) {
            const message = actions.reportError(error);
            setState((current) => ({ ...current, error: message }));
            throw error;
        } finally {
            setState((current) => ({ ...current, saving: false }));
        }
    }

    /**
     * Carry one saved document, reporting what it was carried under.
     *
     * An empty name is the one case with nothing to do: the buttons are disabled
     * without a selection, so this is the guard behind that, not a path to warn on.
     */
    async function carry(
        name: string,
        embed: (name: string) => Promise<string>,
        done: (name: string) => string,
    ): Promise<void> {
        if (!name) {
            return;
        }
        await runAssetAction(async () => {
            actions.toastSuccess(done(await embed(name)));
        });
    }

    /** Take one carried entry back off, reporting what was removed. */
    async function takeBack(
        entry: EmbeddedEntry,
        remove: (key: string) => Promise<void>,
        done: (key: string) => string,
    ): Promise<void> {
        await runAssetAction(async () => {
            await remove(entry.key);
            actions.toastSuccess(done(entry.key));
        });
    }

    const embeddableProfiles = embeddableProfilesOf(state);
    const selectedSkill = state.skills.find((skill) => skill.key === state.selectedSkillKey) ?? null;
    const carried = {
        profiles: state.embeddedProfiles.map((item): EmbeddedEntry => ({
            key: item.profile.id,
            name: profileDisplayName(item),
            subtitle: item.profile.id,
            icon: 'fa-id-card-clip',
        })),
        skills: state.embeddedSkills.map((item): EmbeddedEntry => ({
            key: item.skillName,
            name: item.skillName,
            subtitle: embeddedSkillSubtitle(item),
            icon: 'fa-book-bookmark',
        })),
        states: state.embeddedStates.map((item): EmbeddedEntry => ({
            key: item.name,
            name: item.name,
            subtitle: embeddedStateSubtitle(item),
            icon: 'fa-table-columns',
        })),
        machines: state.embeddedMachines.map((item): EmbeddedEntry => ({
            key: item.name,
            name: item.name,
            subtitle: embeddedMachineSubtitle(item),
            icon: 'fa-diagram-project',
        })),
        predicates: state.embeddedPredicates.map((item): EmbeddedEntry => ({
            key: item.name,
            name: item.name,
            subtitle: embeddedPredicateSubtitle(item),
            icon: 'fa-filter',
        })),
    };
    const alreadyCarried = {
        profiles: carried.profiles.some((entry) => entry.key === state.selectedProfileId),
        skills: selectedSkill !== null && carried.skills.some((entry) => entry.key === selectedSkill.name),
        states: carried.states.some((entry) => entry.key === state.selectedStateName),
        machines: carried.machines.some((entry) => entry.key === state.selectedMachineName),
        predicates: carried.predicates.some((entry) => entry.key === state.selectedPredicateName),
    };
    const label = (kind: keyof typeof alreadyCarried, embedKey: AgentSystemMessageKey): string => (
        alreadyCarried[kind] ? tr('updateEmbeddedAsset') : tr(embedKey)
    );

    const targetInfo = state.targetInfo;
    const targetTypeLabel = !targetInfo
        ? ''
        : targetInfo.kind === 'preset' ? tr('targetPreset') : tr('targetCharacter');

    return (
        <div className="ttas-root ttas-embed-panel">
            <header className="ttas-embed-titlebar">
                <div className="ttas-embed-title-icon" dangerouslySetInnerHTML={{ __html: AGENT_TOGGLE_ICON }} />
                <div className="ttas-embed-title-copy">
                    <span>{targetTypeLabel || tr('agentAssets')}</span>
                    <h3>{tr('agentAssets')}</h3>
                    {targetInfo && <p>{targetInfo.name}</p>}
                </div>
                <button type="button" className="menu_button menu_button_icon ttas-embed-close" aria-label={tr('close')} onClick={onRequestClose}>
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </header>

            <main className="ttas-embed-body">
                {state.loading && !state.initialized ? (
                    <div className="ttas-embed-loading" role="status" aria-live="polite">
                        <i className="fa-solid fa-spinner fa-spin"></i>
                        <span>{tr('embedAssetPanelLoading')}</span>
                    </div>
                ) : (
                    <>
                        {targetInfo && (
                            <div className="ttas-embed-target">
                                <i className={`fa-solid ${targetInfo.kind === 'preset' ? 'fa-sliders' : 'fa-id-card'}`}></i>
                                <div>
                                    <span>{targetTypeLabel}</span>
                                    <strong>{targetInfo.name}</strong>
                                    {targetInfo.subtitle && <small>{targetInfo.subtitle}</small>}
                                </div>
                            </div>
                        )}

                        {state.error && (
                            <div className="ttas-embed-error" role="alert">
                                <i className="fa-solid fa-triangle-exclamation"></i>
                                <span>{state.error}</span>
                            </div>
                        )}

                        <EmbedAssetSection
                            icon="fa-id-card-clip"
                            actionIcon="fa-file-arrow-down"
                            title={tr('profiles')}
                            label={tr('selectProfile')}
                            options={embeddableProfiles.map((profile) => ({
                                value: profile.id,
                                text: profile.displayName || profile.id,
                            }))}
                            selected={state.selectedProfileId}
                            emptyHint={tr('noEmbeddableProfiles')}
                            actionLabel={label('profiles', 'embedProfile')}
                            disabled={state.saving}
                            onSelect={(value) => setState((current) => ({ ...current, selectedProfileId: value }))}
                            onAction={() => void carry(
state.selectedProfileId,
                                actions.embedProfile,
                                (id) => tr('embeddedProfile', { id }),
                            )}
                        />

                        <EmbedAssetSection
                            icon="fa-book-bookmark"
                            actionIcon="fa-file-zipper"
                            title={tr('skills')}
                            label={tr('selectSkill')}
                            options={state.skills.map((skill) => ({
                                value: skill.key,
                                text: skillOptionLabel(skill),
                            }))}
                            selected={state.selectedSkillKey}
                            emptyHint={tr('noSkillsInstalled')}
                            actionLabel={label('skills', 'embedSkill')}
                            disabled={state.saving}
                            onSelect={(value) => setState((current) => ({ ...current, selectedSkillKey: value }))}
                            onAction={() => {
                                const skill = selectedSkill;
                                void carry(
                                    skill?.name ?? '',
                                    async () => {
                                        if (skill) {
                                            await actions.embedSkill(skill);
                                        }
                                        return skill?.name ?? '';
                                    },
                                    (name) => tr('embeddedSkill', { name: skill ? skillOptionLabel(skill) : name }),
                                );
                            }}
                        />

                        <EmbedAssetSection
                            icon="fa-table-columns"
                            actionIcon="fa-file-arrow-down"
                            title={tr('scenes')}
                            label={tr('selectScene')}
                            options={state.states.map((name) => ({ value: name, text: name }))}
                            selected={state.selectedStateName}
                            emptyHint={tr('noStateDeclarations')}
                            actionLabel={label('states', 'embedState')}
                            disabled={state.saving}
                            onSelect={(value) => setState((current) => ({ ...current, selectedStateName: value }))}
                            onAction={() => void carry(
state.selectedStateName,
                                actions.embedState,
                                (name) => tr('embeddedState', { name }),
                            )}
                        />

                        <EmbedAssetSection
                            icon="fa-diagram-project"
                            actionIcon="fa-file-arrow-down"
                            title={tr('stateMachineTab')}
                            label={tr('selectMachine')}
                            options={state.machines.map((name) => ({ value: name, text: name }))}
                            selected={state.selectedMachineName}
                            emptyHint={tr('noStateMachines')}
                            actionLabel={label('machines', 'embedMachine')}
                            disabled={state.saving}
                            onSelect={(value) => setState((current) => ({ ...current, selectedMachineName: value }))}
                            onAction={() => void carry(
state.selectedMachineName,
                                actions.embedMachine,
                                (name) => tr('embeddedMachine', { name }),
                            )}
                        />

                        <EmbedAssetSection
                            icon="fa-filter"
                            actionIcon="fa-file-arrow-down"
                            title={tr('statePredicatesTab')}
                            label={tr('selectPredicateSet')}
                            options={state.predicates.map((name) => ({ value: name, text: name }))}
                            selected={state.selectedPredicateName}
                            emptyHint={tr('noStatePredicateSets')}
                            actionLabel={label('predicates', 'embedPredicateSet')}
                            disabled={state.saving}
                            onSelect={(value) => setState((current) => ({ ...current, selectedPredicateName: value }))}
                            onAction={() => void carry(
state.selectedPredicateName,
                                actions.embedPredicateSet,
                                (name) => tr('embeddedPredicate', { name }),
                            )}
                        />

                        <section className="ttas-embed-card ttas-embed-current">
                            <div className="ttas-embed-section-title">
                                <i className="fa-solid fa-layer-group"></i>
                                <h4>{tr('embeddedAssets')}</h4>
                            </div>

                            <EmbeddedAssetGroup
                                title={tr('embeddedProfiles')}
                                entries={carried.profiles}
                                emptyHint={tr('noEmbeddedProfiles')}
                                disabled={state.saving}
                                onRemove={(entry) => void takeBack(
                                    entry,
                                    actions.removeProfile,
                                    (id) => tr('removedEmbeddedProfile', { id }),
                                )}
                                tr={tr}
                            />
                            <EmbeddedAssetGroup
                                title={tr('embeddedSkills')}
                                entries={carried.skills}
                                emptyHint={tr('noEmbeddedSkills')}
                                disabled={state.saving}
                                onRemove={(entry) => void takeBack(
                                    entry,
                                    actions.removeSkill,
                                    (name) => tr('removedEmbeddedSkill', { name }),
                                )}
                                tr={tr}
                            />
                            <EmbeddedAssetGroup
                                title={tr('embeddedStates')}
                                entries={carried.states}
                                emptyHint={tr('noEmbeddedScenes')}
                                disabled={state.saving}
                                onRemove={(entry) => void takeBack(
                                    entry,
                                    actions.removeState,
                                    (name) => tr('removedEmbeddedScene', { name }),
                                )}
                                tr={tr}
                            />
                            <EmbeddedAssetGroup
                                title={tr('embeddedMachines')}
                                entries={carried.machines}
                                emptyHint={tr('noEmbeddedMachines')}
                                disabled={state.saving}
                                onRemove={(entry) => void takeBack(
                                    entry,
                                    actions.removeMachine,
                                    (name) => tr('removedEmbeddedMachine', { name }),
                                )}
                                tr={tr}
                            />
                            <EmbeddedAssetGroup
                                title={tr('embeddedPredicateSets')}
                                entries={carried.predicates}
                                emptyHint={tr('noEmbeddedPredicateSets')}
                                disabled={state.saving}
                                onRemove={(entry) => void takeBack(
                                    entry,
                                    actions.removePredicateSet,
                                    (name) => tr('removedEmbeddedPredicateSet', { name }),
                                )}
                                tr={tr}
                            />
                        </section>
                    </>
                )}
            </main>
        </div>
    );
}
