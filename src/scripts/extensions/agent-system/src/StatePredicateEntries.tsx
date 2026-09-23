/**
 * One predicate entry: what it says, when it applies, and what it does to the
 * other entries.
 *
 * The condition rows are the flat comparison only — a loaded composition is
 * passed through untouched and marked as such, so nothing about it can drift.
 * The premise (availability) is optional and only appears once it is switched on,
 * because "not a candidate at all" and "inhibited" are different answers and an
 * always-present empty row would blur them.
 */

import type { Tr } from './AgentSystemPanelContract';
import type {
    PredicateConfigController,
    PredicateConfigSnapshot,
} from './state-predicate-controller';
import type { PredicateConditionDraft, PredicateEntryDraft, PredicateEntryTarget } from './state-predicate-model';

const OPS = ['eq', 'neq', 'in', 'not_in', 'contains', 'exists', 'missing'] as const;

function ConditionRow({
    condition,
    tr,
    label,
    onChange,
}: {
    condition: PredicateConditionDraft;
    tr: Tr;
    label: string;
    onChange: (patch: Partial<PredicateConditionDraft>) => void;
}) {
    if (condition.compose) {
        const kind = 'all' in condition.compose ? 'all' : 'any' in condition.compose ? 'any' : 'not';
        return <p className="ttas-field-hint">{tr('predicateComposeKept', { kind })}</p>;
    }
    return (
        <div className="ttas-machine-row">
            <input
                className="text_pole"
                aria-label={label}
                placeholder={tr('machineConditionField')}
                value={condition.fieldText}
                onChange={(event) => onChange({ fieldText: event.target.value })}
            />
            <select
                aria-label={tr('machineConditionOp')}
                value={condition.op}
                onChange={(event) => onChange({ op: event.target.value })}
            >
                {OPS.map((op) => <option key={op} value={op}>{op}</option>)}
            </select>
            <input
                className="text_pole"
                aria-label={tr('machineConditionValue')}
                placeholder={tr('machineConditionValue')}
                value={condition.valueText}
                onChange={(event) => onChange({ valueText: event.target.value })}
            />
        </div>
    );
}

export function PredicateEntryCard({
    target,
    entryIndex,
    entry,
    controller,
    tr,
}: {
    target: PredicateEntryTarget;
    entryIndex: number;
    entry: PredicateEntryDraft;
    controller: PredicateConfigController;
    tr: Tr;
}) {
    return (
        <div className="ttas-predicate-entry" data-ttas-predicate-entry={entry.id || entryIndex}>
            <div className="ttas-machine-row">
                <input
                    className="text_pole"
                    aria-label={tr('predicateEntryId')}
                    placeholder={tr('predicateEntryIdPlaceholder')}
                    value={entry.id}
                    onChange={(event) => controller.updateEntry(target, entryIndex, { id: event.target.value })}
                />
                <input
                    className="text_pole"
                    aria-label={tr('predicateEntryLabel')}
                    placeholder={tr('predicateEntryLabelPlaceholder')}
                    value={entry.labelText}
                    onChange={(event) => controller.updateEntry(target, entryIndex, { labelText: event.target.value })}
                />
                <input
                    className="text_pole"
                    aria-label={tr('predicateEntryPriority')}
                    placeholder={tr('predicateEntryPriority')}
                    value={entry.priorityText}
                    onChange={(event) => controller.updateEntry(target, entryIndex, { priorityText: event.target.value })}
                />
                <button
                    type="button"
                    className="menu_button ttas-machine-icon-button"
                    title={tr('delete')}
                    onClick={() => controller.removeEntry(target, entryIndex)}
                >
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </div>
            <textarea
                className="text_pole textarea_compact"
                rows={3}
                aria-label={tr('predicateEntryContent')}
                placeholder={tr('predicateEntryContentPlaceholder')}
                value={entry.content}
                onChange={(event) => controller.updateEntry(target, entryIndex, { content: event.target.value })}
            ></textarea>
            <div className="ttas-machine-row">
                <input
                    className="text_pole"
                    aria-label={tr('predicateEntryTags')}
                    placeholder={tr('predicateEntryTagsPlaceholder')}
                    value={entry.tagsText}
                    onChange={(event) => controller.updateEntry(target, entryIndex, { tagsText: event.target.value })}
                />
                <select
                    aria-label={tr('predicateEntrySource')}
                    value={entry.sourceKind}
                    onChange={(event) => controller.updateEntry(target, entryIndex, {
                        sourceKind: event.target.value === 'state' ? 'state' : 'constant',
                    })}
                >
                    <option value="constant">{tr('predicateSourceConstant')}</option>
                    <option value="state">{tr('predicateSourceState')}</option>
                </select>
            </div>
            {entry.sourceKind === 'state' && (
                <ConditionRow
                    condition={entry.condition ?? { fieldText: '', op: 'eq', valueText: '' }}
                    tr={tr}
                    label={tr('predicateEntrySourceCondition')}
                    onChange={(patch) => controller.updateEntryCondition(target, entryIndex, patch)}
                />
            )}

            <label className="checkbox_label ttas-field">
                <span>{tr('predicateEntryAvailability')}</span>
                <input
                    type="checkbox"
                    checked={entry.availability !== null}
                    onChange={(event) => controller.setEntryAvailability(target, entryIndex, event.target.checked)}
                />
            </label>
            {entry.availability && (
                <ConditionRow
                    condition={entry.availability}
                    tr={tr}
                    label={tr('predicateEntryAvailability')}
                    onChange={(patch) => controller.updateEntryAvailability(target, entryIndex, patch)}
                />
            )}

            <div className="ttas-machine-images-head">
                <span>{tr('predicateEntryEffects')}</span>
                <button
                    type="button"
                    className="menu_button"
                    onClick={() => controller.addEffect(target, entryIndex)}
                >
                    <i className="fa-solid fa-plus"></i>
                    <span>{tr('predicateAddEffect')}</span>
                </button>
            </div>
            <p className="ttas-field-hint">{tr('predicateEntryEffectsHint')}</p>
            {entry.effects.map((effect, effectIndex) => (
                <div className="ttas-machine-row" key={effectIndex}>
                    <select
                        aria-label={tr('predicateEffectKind')}
                        value={effect.kind}
                        onChange={(event) => controller.updateEffect(target, entryIndex, effectIndex, {
                            kind: event.target.value === 'require' ? 'require' : 'inhibit',
                        })}
                    >
                        <option value="inhibit">{tr('predicateEffectInhibit')}</option>
                        <option value="require">{tr('predicateEffectRequire')}</option>
                    </select>
                    <input
                        className="text_pole"
                        aria-label={tr('predicateEffectTags')}
                        placeholder={tr('predicateEffectTagsPlaceholder')}
                        value={effect.tagsText}
                        onChange={(event) => controller.updateEffect(target, entryIndex, effectIndex, {
                            tagsText: event.target.value,
                        })}
                    />
                    <button
                        type="button"
                        className="menu_button ttas-machine-icon-button"
                        title={tr('delete')}
                        onClick={() => controller.removeEffect(target, entryIndex, effectIndex)}
                    >
                        <i className="fa-solid fa-xmark"></i>
                    </button>
                </div>
            ))}
        </div>
    );
}

/** The standing entries: the ones that never compete. */
export function PredicateConstantsSection({
    snapshot,
    controller,
    tr,
}: {
    snapshot: PredicateConfigSnapshot;
    controller: PredicateConfigController;
    tr: Tr;
}) {
    const draft = snapshot.draft;
    if (!draft) {
        return null;
    }
    const target: PredicateEntryTarget = { kind: 'constants' };
    return (
        <div className="ttas-section" data-ttas-predicate-section="constants">
            <div className="ttas-machine-images-head">
                <span>{tr('predicateConstants')}</span>
                <button
                    type="button"
                    className="menu_button"
                    onClick={() => controller.addEntry(target)}
                >
                    <i className="fa-solid fa-plus"></i>
                    <span>{tr('predicateAddEntry')}</span>
                </button>
            </div>
            <p className="ttas-field-hint">{tr('predicateConstantsHint')}</p>
            {draft.constants.length === 0 && <p className="ttas-field-hint">{tr('predicateNone')}</p>}
            {draft.constants.map((entry, index) => (
                <PredicateEntryCard
                    key={index}
                    target={target}
                    entryIndex={index}
                    entry={entry}
                    controller={controller}
                    tr={tr}
                />
            ))}
        </div>
    );
}
