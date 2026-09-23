/**
 * One transition card: position lists, its conditions and its effects.
 *
 * Conditions read either the state document (`field`) or the machine's own
 * positions (`active`) — the same two sources the backend's evaluator knows.
 * A composition condition loaded from disk is shown as a kept chip: this editor
 * does not rebuild compositions, and storing them back untouched is the only
 * way to guarantee they cannot drift.
 */

import { STATE_CONDITION_OPS } from './state-config-model';
import type {
    MachineActionDraft,
    MachineConditionDraft,
    MachineTransitionDraft,
} from './state-machine-model';
import type { MachineConfigController } from './state-machine-controller';
import type { Tr } from './AgentSystemPanelContract';

function composeTag(compose: NonNullable<MachineConditionDraft['compose']>): string {
    if ('all' in compose) {
        return 'all';
    }
    if ('any' in compose) {
        return 'any';
    }
    return 'not';
}

function ConditionRow({
    transitionIndex,
    conditionIndex,
    condition,
    controller,
    tr,
}: {
    transitionIndex: number;
    conditionIndex: number;
    condition: MachineConditionDraft;
    controller: MachineConfigController;
    tr: Tr;
}) {
    if (condition.compose) {
        return (
            <div className="ttas-machine-row">
                <span className="ttas-field-hint">
                    {tr('machineComposeKept', { kind: composeTag(condition.compose) })}
                </span>
                <button
                    type="button"
                    className="menu_button ttas-machine-icon-button"
                    title={tr('delete')}
                    onClick={() => controller.removeCondition(transitionIndex, conditionIndex)}
                >
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </div>
        );
    }
    return (
        <div className="ttas-machine-row">
            <select
                className="text_pole"
                aria-label={tr('machineConditionSource')}
                value={condition.sourceText}
                onChange={(event) => controller.updateCondition(transitionIndex, conditionIndex, {
                    sourceText: event.target.value,
                })}
            >
                <option value="field">{tr('machineConditionSourceField')}</option>
                <option value="active">{tr('machineConditionSourceActive')}</option>
            </select>
            <input
                className="text_pole"
                aria-label={tr('machineConditionField')}
                placeholder={condition.sourceText === 'active' ? 'night' : '环境/日期'}
                value={condition.fieldText}
                onChange={(event) => controller.updateCondition(transitionIndex, conditionIndex, {
                    fieldText: event.target.value,
                })}
            />
            <select
                className="text_pole ttas-machine-op"
                aria-label={tr('machineConditionOp')}
                value={condition.op}
                onChange={(event) => controller.updateCondition(transitionIndex, conditionIndex, {
                    op: event.target.value,
                })}
            >
                {STATE_CONDITION_OPS.map((op) => (
                    <option key={op} value={op}>{op}</option>
                ))}
            </select>
            {!['exists', 'missing'].includes(condition.op) && (
                <input
                    className="text_pole"
                    aria-label={tr('machineConditionValue')}
                    placeholder={tr('machineConditionValue')}
                    value={condition.valueText}
                    onChange={(event) => controller.updateCondition(transitionIndex, conditionIndex, {
                        valueText: event.target.value,
                    })}
                />
            )}
            <button
                type="button"
                className="menu_button ttas-machine-icon-button"
                title={tr('delete')}
                onClick={() => controller.removeCondition(transitionIndex, conditionIndex)}
            >
                <i className="fa-solid fa-xmark"></i>
            </button>
        </div>
    );
}

function ActionRow({
    transitionIndex,
    actionIndex,
    action,
    controller,
    tr,
}: {
    transitionIndex: number;
    actionIndex: number;
    action: MachineActionDraft;
    controller: MachineConfigController;
    tr: Tr;
}) {
    return (
        <div className="ttas-machine-row">
            <select
                className="text_pole"
                aria-label={tr('machineActionKind')}
                value={action.kind}
                onChange={(event) => controller.updateAction(transitionIndex, actionIndex, {
                    kind: event.target.value,
                })}
            >
                <option value="setField">{tr('machineActionSetField')}</option>
                <option value="clearField">{tr('machineActionClearField')}</option>
                <option value="emit">{tr('machineActionEmit')}</option>
            </select>
            <input
                className="text_pole"
                aria-label={tr('machineActionTarget')}
                placeholder={action.kind === 'emit' ? 'scene/ended' : '角色/*/位置'}
                value={action.targetText}
                onChange={(event) => controller.updateAction(transitionIndex, actionIndex, {
                    targetText: event.target.value,
                })}
            />
            {action.kind === 'setField' && (
                <input
                    className="text_pole"
                    aria-label={tr('machineActionValues')}
                    placeholder={tr('machineActionValues')}
                    value={action.valuesText}
                    onChange={(event) => controller.updateAction(transitionIndex, actionIndex, {
                        valuesText: event.target.value,
                    })}
                />
            )}
            <button
                type="button"
                className="menu_button ttas-machine-icon-button"
                title={tr('delete')}
                onClick={() => controller.removeAction(transitionIndex, actionIndex)}
            >
                <i className="fa-solid fa-xmark"></i>
            </button>
        </div>
    );
}

export function MachineTransitionCard({
    transitionIndex,
    transition,
    controller,
    tr,
}: {
    transitionIndex: number;
    transition: MachineTransitionDraft;
    controller: MachineConfigController;
    tr: Tr;
}) {
    return (
        <div className="ttas-machine-card">
            <div className="ttas-machine-row">
                <label className="ttas-field">
                    <span>{tr('machineFrom')}</span>
                    <input
                        className="text_pole"
                        value={transition.fromText}
                        placeholder="day"
                        onChange={(event) => controller.updateTransition(transitionIndex, {
                            fromText: event.target.value,
                        })}
                    />
                </label>
                <label className="ttas-field">
                    <span>{tr('machineTo')}</span>
                    <input
                        className="text_pole"
                        value={transition.toText}
                        placeholder="night"
                        onChange={(event) => controller.updateTransition(transitionIndex, {
                            toText: event.target.value,
                        })}
                    />
                </label>
                <label className="ttas-field ttas-machine-priority">
                    <span>{tr('machinePriority')}</span>
                    <input
                        className="text_pole"
                        type="number"
                        value={transition.priorityText}
                        onChange={(event) => controller.updateTransition(transitionIndex, {
                            priorityText: event.target.value,
                        })}
                    />
                </label>
                <button
                    type="button"
                    className="menu_button ttas-machine-icon-button"
                    title={tr('delete')}
                    onClick={() => controller.removeTransition(transitionIndex)}
                >
                    <i className="fa-solid fa-xmark"></i>
                </button>
            </div>
            <div className="ttas-machine-card-field">
                <div className="ttas-machine-images-head">
                    <span>{tr('machineConditions')}</span>
                    <button
                        type="button"
                        className="menu_button"
                        onClick={() => controller.addCondition(transitionIndex)}
                    >
                        <i className="fa-solid fa-plus"></i>
                        <span>{tr('machineAddCondition')}</span>
                    </button>
                </div>
                {transition.conditions.map((condition, conditionIndex) => (
                    <ConditionRow
                        key={conditionIndex}
                        transitionIndex={transitionIndex}
                        conditionIndex={conditionIndex}
                        condition={condition}
                        controller={controller}
                        tr={tr}
                    />
                ))}
            </div>
            <div className="ttas-machine-card-field">
                <div className="ttas-machine-images-head">
                    <span>{tr('machineActions')}</span>
                    <button
                        type="button"
                        className="menu_button"
                        onClick={() => controller.addAction(transitionIndex)}
                    >
                        <i className="fa-solid fa-plus"></i>
                        <span>{tr('machineAddAction')}</span>
                    </button>
                </div>
                {transition.actions.map((action, actionIndex) => (
                    <ActionRow
                        key={actionIndex}
                        transitionIndex={transitionIndex}
                        actionIndex={actionIndex}
                        action={action}
                        controller={controller}
                        tr={tr}
                    />
                ))}
            </div>
        </div>
    );
}
