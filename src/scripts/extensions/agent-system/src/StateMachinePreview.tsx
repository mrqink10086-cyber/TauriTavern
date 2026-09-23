/**
 * The evaluation preview: assumed positions and field values in, the backend's
 * deterministic run out. Nothing here writes to a chat — the preview only reads
 * what the evaluator reports.
 */

import type { MachineRunDto } from './state-machine-model';
import type { MachineConfigController, MachineConfigSnapshot } from './state-machine-controller';
import type { Tr } from './AgentSystemPanelContract';

function idsLabel(label: string, ids: readonly string[]): string {
    return ids.length > 0 ? `${label}: ${ids.join(', ')}` : '';
}

export function MachinePreviewSection({
    snapshot,
    controller,
    tr,
}: {
    snapshot: MachineConfigSnapshot;
    controller: MachineConfigController;
    tr: Tr;
}) {
    const run = snapshot.preview;
    return (
        <div className="ttas-section" data-ttas-machine-section="preview">
            <div className="ttas-section-title">
                <i className="fa-solid fa-play"></i>
                <h4>{tr('machinePreview')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('machinePreviewHint')}</p>
            <label className="ttas-field">
                <span>{tr('machinePreviewActive')}</span>
                <input
                    className="text_pole"
                    value={snapshot.previewActiveText}
                    placeholder={tr('machinePreviewActive')}
                    onChange={(event) => controller.setPreviewActive(event.target.value)}
                />
            </label>
            <div className="ttas-machine-images-head">
                <span>{tr('machinePreviewFields')}</span>
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
            {run && <MachineRunResult run={run} tr={tr} />}
        </div>
    );
}

function MachineRunResult({ run, tr }: { run: MachineRunDto; tr: Tr }) {
    const { evaluation } = run;
    return (
        <div className="ttas-machine-result">
            <p className="ttas-machine-result-line">
                <strong>{tr('machinePreviewActiveAfter')}</strong>
                {[...evaluation.active].join(', ') || '—'}
            </p>
            {evaluation.applied.length > 0 && (
                <p className="ttas-machine-result-line">
                    <strong>{tr('machinePreviewApplied')}</strong>
                    {evaluation.applied.map((applied) => (
                        <span key={applied.index}>
                            {` #${applied.index + 1} [${applied.from.join(', ')}] -> [${applied.to.join(', ')}]`}
                        </span>
                    ))}
                </p>
            )}
            {evaluation.skipped.length > 0 && (
                <p className="ttas-machine-result-line">
                    <strong>{tr('machinePreviewSkipped')}</strong>
                    {evaluation.skipped.map((skipped) => (
                        <span key={skipped.index}>{` #${skipped.index + 1} (${skipped.reason})`}</span>
                    ))}
                </p>
            )}
            {evaluation.writes.length > 0 && (
                <p className="ttas-machine-result-line">
                    <strong>{tr('machinePreviewWrites')}</strong>
                    {evaluation.writes.map((write) => (
                        <span key={write.key}>{` ${write.key} = [${write.values.join(', ')}]`}</span>
                    ))}
                </p>
            )}
            {evaluation.events.length > 0 && (
                <p className="ttas-machine-result-line">
                    <strong>{tr('machinePreviewEvents')}</strong>
                    {idsLabel('', evaluation.events)}
                </p>
            )}
            {evaluation.hooks.length > 0 && (
                <p className="ttas-machine-result-line">
                    <strong>{tr('machinePreviewHooks')}</strong>
                    {evaluation.hooks.map((hook) => (
                        <span key={hook.index}>{` #${hook.index + 1}`}</span>
                    ))}
                </p>
            )}
            {run.errors.length > 0 && (
                <div className="ttas-state-issues">
                    <ul>
                        {run.errors.map((error, index) => (
                            <li key={index}>
                                <code>{error.code}</code>
                                {error.target ? `${error.target}: ` : ''}
                                {error.message}
                            </li>
                        ))}
                    </ul>
                </div>
            )}
        </div>
    );
}
