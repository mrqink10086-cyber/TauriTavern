import { isBuiltinProfile } from './AgentSystemPanelContract';
import { recallViewOf } from './profile-recall';
import type { ProfileSectionProps } from './ProfilePolicyToolsSections';

/**
 * What this Agent does with the recall its chat's extensions produced.
 *
 * The blocks are written once, before the run starts, and frozen with the rest of
 * its input — so there is no switch here for recalling again. A second retrieval
 * would be the same question asked of the same index with the same context, at
 * the cost of another round trip and one more chance for the two answers to
 * differ. What is left to decide is only who reads what was already recalled.
 *
 * Nothing here trims a block either: a recall block is the extension's answer to
 * "what does this story need to remember", and a cut applied on the way through
 * would quietly turn that answer into a partial one.
 */
export function ProfileRecallSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const recall = recallViewOf(draft);

    return (
        <div className="ttas-section" data-ttas-profile-section="recall">
            <div className="ttas-section-title">
                <i className="fa-solid fa-brain"></i>
                <h4>{tr('recall')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('recallHint')}</p>
            <label className="ttas-switch-row">
                <input
                    type="checkbox"
                    checked={recall.inject}
                    disabled={builtin}
                    onChange={(event) => controller.setRecallField('inject', event.target.checked)}
                />
                <span>
                    <strong>{tr('recallInject')}</strong>
                    <small>{tr('recallInjectHint')}</small>
                </span>
            </label>
            <label className="ttas-field">
                <span>{tr('recallSources')}</span>
                <input
                    className="text_pole"
                    type="text"
                    value={recall.sourcesCsv}
                    disabled={builtin}
                    placeholder={tr('recallSourcesPlaceholder')}
                    onChange={(event) => controller.setRecallField('sources', event.target.value)}
                />
                <small>{tr('recallSourcesHint')}</small>
            </label>
            <label className="ttas-switch-row">
                <input
                    type="checkbox"
                    checked={recall.subagentInherits}
                    disabled={builtin}
                    onChange={(event) => controller.setRecallField('subagent', event.target.checked)}
                />
                <span>
                    <strong>{tr('recallSubagentInherits')}</strong>
                    <small>{tr('recallSubagentHint')}</small>
                </span>
            </label>
        </div>
    );
}
