/**
 * Pieces the state declaration form reuses.
 *
 * The picture editor is the largest block of the form and belongs to every
 * panel field as much as to a panel's own background; keeping it — and the row
 * button it shares with the other rows — in its own file is what stops the form
 * itself from growing past the size a reader can hold.
 */

import { useRef, useState } from 'react';
import type { ChangeEvent } from 'react';
import {
    STATE_CONDITION_OPS,
    STATE_IMAGE_FITS,
    conditionValueText,
    type StateImageFit,
    type StateImageSet,
} from './state-config-model';
import { importStateImage } from './state-config-image-import';
import { reportAgentSystemError } from './host-api';
import type { StateConfigController } from './state-config-controller';
import type { StateImageTarget } from './state-config-ops';
import type { Tr } from './AgentSystemPanelContract';

/**
 * The label for one fit.
 *
 * Written out rather than composed, so the two names are the two keys the
 * translations actually carry and a third fit cannot be added without a word
 * for it.
 */
function fitLabelKey(fit: StateImageFit): 'stateDeclarationImageFitCover' | 'stateDeclarationImageFitContain' {
    return fit === 'contain' ? 'stateDeclarationImageFitContain' : 'stateDeclarationImageFitCover';
}

export function RemoveRowButton({ label, onClick }: { label: string; onClick: () => void }) {
    return (
        <button type="button" className="menu_button ttas-state-icon-button" title={label} onClick={onClick}>
            <i className="fa-solid fa-xmark"></i>
        </button>
    );
}


/**
 * The pictures one element may show.
 *
 * A candidate without a condition is the fallback, and the backend requires it
 * last: a fallback in the middle would hide every candidate after it, which is
 * a configuration error rather than a subtle preference.
 *
 * The script below the candidates is the set's own decision, and the note says
 * so when it is set: a script that takes over silently would leave the
 * conditions looking like they still decide.
 */
export function ImageSetEditor({
    label,
    images,
    target,
    controller,
    tr,
}: {
    label: string;
    images: StateImageSet | null;
    target: StateImageTarget;
    controller: StateConfigController;
    tr: Tr;
}) {
    const candidates = images?.candidates ?? [];
    const fit: StateImageFit = images?.fit === 'contain' ? 'contain' : 'cover';
    const script = String(images?.conditionScript?.script ?? '');
    const fileInputRef = useRef<HTMLInputElement | null>(null);
    const importTargetRef = useRef<number | null>(null);
    const [importing, setImporting] = useState<number | null>(null);

    const requestImport = (candidateIndex: number) => {
        importTargetRef.current = candidateIndex;
        fileInputRef.current?.click();
    };

    const onImportFileChosen = async (event: ChangeEvent<HTMLInputElement>) => {
        const input = event.currentTarget;
        const file = input.files?.[0] ?? null;
        const candidateIndex = importTargetRef.current;
        importTargetRef.current = null;
        // Cleared either way, so picking the same file again still fires `change`.
        input.value = '';
        if (!file || candidateIndex === null) {
            return;
        }

        setImporting(candidateIndex);
        try {
            const source = await importStateImage(file, tr);
            controller.setImageCandidateSource(target, candidateIndex, source);
        } catch (error) {
            reportAgentSystemError(error);
        } finally {
            setImporting(null);
        }
    };

    return (
        <div className="ttas-state-images">
            <div className="ttas-state-images-head">
                <span>{label}</span>
                {/* How the picture fills its box: a scene wants the panel
                    covered, a portrait wants itself shown whole. It sits with
                    the label because it is a fact about the whole set. */}
                <select
                    className="text_pole ttas-state-image-fit"
                    value={fit}
                    title={tr('stateDeclarationImageFitHint')}
                    onChange={(event) => controller.setImageFit(target, event.target.value as StateImageFit)}
                >
                    {STATE_IMAGE_FITS.map((name) => (
                        <option key={name} value={name}>{tr(fitLabelKey(name))}</option>
                    ))}
                </select>
                <button type="button" className="menu_button" onClick={() => controller.addImageCandidate(target)}>
                    <i className="fa-solid fa-image"></i>
                    <span>{tr('stateDeclarationAddImage')}</span>
                </button>
            </div>
            {candidates.length === 0 && <p className="ttas-field-hint">{tr('stateDeclarationNoImages')}</p>}
            {candidates.map((candidate, candidateIndex) => {
                const condition = candidate.when ?? null;
                const valueText = conditionValueText(condition);
                const field = condition?.field ?? '';
                const op = condition?.op ?? 'eq';
                return (
                    <div className="ttas-state-candidate" key={candidateIndex}>
                        <input
                            className="text_pole"
                            value={candidate.source}
                            placeholder={tr('stateDeclarationImageSource')}
                            onChange={(event) => controller.setImageCandidateSource(
                                target,
                                candidateIndex,
                                event.target.value,
                            )}
                        />
                        <button
                            type="button"
                            className="menu_button ttas-state-icon-button"
                            title={tr('stateDeclarationImportImage')}
                            disabled={importing !== null}
                            onClick={() => requestImport(candidateIndex)}
                        >
                            <i className="fa-solid fa-folder-open"></i>
                        </button>
                        <input
                            className="text_pole"
                            value={field}
                            placeholder={tr('stateDeclarationConditionField')}
                            onChange={(event) => controller.setImageCandidateCondition(target, candidateIndex, {
                                field: event.target.value,
                                op,
                                valueText,
                            })}
                        />
                        <select
                            className="text_pole"
                            value={op}
                            onChange={(event) => controller.setImageCandidateCondition(target, candidateIndex, {
                                field,
                                op: event.target.value,
                                valueText,
                            })}
                        >
                            {STATE_CONDITION_OPS.map((name) => (
                                <option key={name} value={name}>{name}</option>
                            ))}
                        </select>
                        <input
                            className="text_pole"
                            value={valueText}
                            placeholder={tr('stateDeclarationConditionValue')}
                            onChange={(event) => controller.setImageCandidateCondition(target, candidateIndex, {
                                field,
                                op,
                                valueText: event.target.value,
                            })}
                        />
                        <RemoveRowButton
                            label={tr('delete')}
                            onClick={() => controller.removeImageCandidate(target, candidateIndex)}
                        />
                    </div>
                );
            })}
            {candidates.length > 0 && <p className="ttas-field-hint">{tr('stateDeclarationFallbackHint')}</p>}

            <label className="ttas-field ttas-state-script">
                <span>{tr('stateDeclarationConditionScript')}</span>
                <textarea
                    className="text_pole ttas-state-script-source"
                    rows={4}
                    value={script}
                    placeholder={tr('stateDeclarationConditionScriptPlaceholder')}
                    onChange={(event) => controller.setImageConditionScript(target, event.target.value)}
                />
            </label>
            {script.trim()
                ? (
                    <p className="ttas-state-script-active">
                        <i className="fa-solid fa-triangle-exclamation"></i>
                        {tr('stateDeclarationScriptWins')}
                    </p>
                )
                : <p className="ttas-field-hint">{tr('stateDeclarationConditionScriptHint')}</p>}

            <input
                ref={fileInputRef}
                type="file"
                accept="image/*,video/*"
                className="displayNone"
                onChange={(event) => { void onImportFileChosen(event); }}
            />
        </div>
    );
}
