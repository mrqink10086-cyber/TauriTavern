import { useEffect, useState } from 'react';
import { isBuiltinProfile } from './AgentSystemPanelContract';
import { errorText, tryHostApi } from './host-api';
import {
    worldInfoEntryCarried,
    worldInfoEntryRuleOf,
    worldInfoViewOf,
} from './profile-context-world-info';
import type { ProfileSectionProps } from './ProfilePolicyToolsSections';

/**
 * Which of the chat's activated World Info entries a delegated invocation reads.
 *
 * The list is the run's last scan — the entries the books activated for this chat
 * — because that set is what a SubAgent inherits from: the scan happens once,
 * before a run starts, and answers a question about the chat rather than about one
 * Agent. What is decided here is only who reads it.
 *
 * This is where a fixed style sheet or house rule belongs. A Skill has to be
 * opened on purpose, and anything a model summarises on the way in is no longer
 * the text that was written; an entry carried this way arrives word for word.
 */
export function ProfileWorldInfoSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const view = worldInfoViewOf(draft);
    // Read in render rather than in the effect: whether the host offers the
    // activation readout at all is a fact about the frame, and a section that can
    // say so should, instead of taking the panel down over it.
    const worldInfoApi = tryHostApi('worldInfo');
    const activationUnavailable = !worldInfoApi?.getLastActivation;
    const [activation, setActivation] = useState<TauriTavernWorldInfoActivationBatch | null>(null);
    const [failure, setFailure] = useState<string | null>(null);

    useEffect(() => {
        if (!worldInfoApi?.getLastActivation) {
            return undefined;
        }

        let live = true;
        worldInfoApi
            .getLastActivation()
            .then((batch) => {
                if (live) {
                    setActivation(batch);
                }
            })
            .catch((error: unknown) => {
                if (live) {
                    setFailure(errorText(error));
                }
            });
        return () => {
            live = false;
        };
    }, [worldInfoApi]);

    const entries = activation?.entries ?? [];

    return (
        <div className="ttas-section" data-ttas-profile-section="world-info">
            <div className="ttas-section-title">
                <i className="fa-solid fa-book-open"></i>
                <h4>{tr('worldInfoAccess')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('worldInfoHint')}</p>
            <label className="ttas-switch-row">
                <input
                    type="checkbox"
                    checked={view.subagentInherits}
                    disabled={builtin}
                    onChange={(event) => controller.setWorldInfoSubagentInherits(event.target.checked)}
                />
                <span>
                    <strong>{tr('worldInfoSubagentInherits')}</strong>
                    <small>{tr('worldInfoSubagentHint')}</small>
                </span>
            </label>
            {(activationUnavailable || failure) && (
                <p className="ttas-field-hint">
                    {failure ?? tr('worldInfoActivationUnavailable')}
                </p>
            )}
            {!failure && entries.length === 0 && (
                <p className="ttas-field-hint">{tr('worldInfoNoActivation')}</p>
            )}
            {entries.length > 0 && (
                <>
                    <p className="ttas-field-hint">{tr('worldInfoEntriesHint')}</p>
                    {entries.map((entry) => {
                        const rule = worldInfoEntryRuleOf(view, entry);
                        const carried = worldInfoEntryCarried(view, entry);
                        return (
                            <label
                                className="ttas-switch-row"
                                key={`${entry.world}.${entry.uid}`}
                            >
                                <input
                                    type="checkbox"
                                    checked={carried}
                                    disabled={builtin}
                                    onChange={(event) => controller.setWorldInfoEntry(entry, event.target.checked)}
                                />
                                <span>
                                    <strong>
                                        {entry.displayName || String(entry.uid)}
                                        {entry.constant && <em>{tr('worldInfoConstant')}</em>}
                                        {rule && <em>{tr('worldInfoExceptional')}</em>}
                                    </strong>
                                    <small>
                                        {entry.world}
                                        {entry.contentPreview ? ` — ${entry.contentPreview}` : ''}
                                    </small>
                                </span>
                            </label>
                        );
                    })}
                    {view.rules.length > 0 && (
                        <button
                            type="button"
                            className="ttas-link-button"
                            disabled={builtin}
                            onClick={() => controller.clearWorldInfoRules()}
                        >
                            {tr('worldInfoClearRules')}
                        </button>
                    )}
                </>
            )}
        </div>
    );
}
