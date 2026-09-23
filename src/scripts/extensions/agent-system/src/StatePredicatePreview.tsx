/**
 * The evaluation preview: assumed field values in, the backend's deterministic
 * answer out. Nothing here writes to a chat — the preview only reads what the
 * evaluator reports.
 *
 * Every entry that was a candidate but did not make it is listed with its
 * reason — `unavailable`, `inhibited`, `requirement_missing`, `group_lost`,
 * `over_budget` — the evaluator's own words. A preview that could not explain a
 * short block would be no better than guessing, and "why is my entry missing" is
 * the question this panel exists to answer.
 */

import type { Tr } from './AgentSystemPanelContract';
import type {
    PredicateConfigController,
    PredicateConfigSnapshot,
} from './state-predicate-controller';

export function PredicatePreviewSection({
    snapshot,
    controller,
    tr,
}: {
    snapshot: PredicateConfigSnapshot;
    controller: PredicateConfigController;
    tr: Tr;
}) {
    const evaluation = snapshot.preview;
    return (
        <div className="ttas-section" data-ttas-predicate-section="preview">
            <div className="ttas-section-title">
                <i className="fa-solid fa-play"></i>
                <h4>{tr('predicatePreview')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('predicatePreviewHint')}</p>
            <div className="ttas-machine-images-head">
                <span>{tr('predicatePreviewFields')}</span>
                <button
                    type="button"
                    className="menu_button"
                    onClick={() => controller.addPreviewField()}
                >
                    <i className="fa-solid fa-plus"></i>
                    <span>{tr('machinePreviewAddField')}</span>
                </button>
            </div>
            {snapshot.previewFields.map((field, index) => (
                <div className="ttas-machine-row" key={index}>
                    <input
                        className="text_pole"
                        aria-label={tr('machinePreviewFieldKey')}
                        placeholder={tr('machinePreviewFieldKey')}
                        value={field.keyText}
                        onChange={(event) => controller.updatePreviewField(index, { keyText: event.target.value })}
                    />
                    <input
                        className="text_pole"
                        aria-label={tr('machinePreviewFieldValues')}
                        placeholder={tr('machinePreviewFieldValues')}
                        value={field.valuesText}
                        onChange={(event) => controller.updatePreviewField(index, { valuesText: event.target.value })}
                    />
                    <button
                        type="button"
                        className="menu_button ttas-machine-icon-button"
                        title={tr('delete')}
                        onClick={() => controller.removePreviewField(index)}
                    >
                        <i className="fa-solid fa-xmark"></i>
                    </button>
                </div>
            ))}
            <div className="ttas-state-footer">
                <button
                    type="button"
                    className="menu_button ttas-primary-button"
                    disabled={!snapshot.draft || snapshot.previewing}
                    onClick={() => void controller.runPreview()}
                >
                    <i className="fa-solid fa-play"></i>
                    <span>{tr('machinePreviewRun')}</span>
                </button>
            </div>
            {evaluation && (
                <div className="ttas-machine-result">
                    <p className="ttas-machine-result-line">
                        <strong>{tr('predicatePreviewSelected')}</strong>
                        {evaluation.selected.length === 0
                            ? tr('predicatePreviewNone')
                            : evaluation.selected.map((selected) => (
                                <span key={`${selected.groupId ?? ''}/${selected.entryId}`}>
                                    {` ${selected.entryId}${selected.groupId ? ` (${selected.groupId})` : ''}`}
                                </span>
                            ))}
                    </p>
                    {evaluation.selected.map((selected) => (
                        <pre className="ttas-machine-result-block" key={`body-${selected.groupId ?? ''}/${selected.entryId}`}>
                            {selected.content}
                        </pre>
                    ))}
                    {evaluation.skipped.length > 0 && (
                        <p className="ttas-machine-result-line">
                            <strong>{tr('predicatePreviewSkipped')}</strong>
                            {evaluation.skipped.map((skipped) => (
                                <span key={skipped.entryId}>
                                    {` ${skipped.entryId} (${skipped.reason})`}
                                </span>
                            ))}
                        </p>
                    )}
                </div>
            )}
        </div>
    );
}
