/**
 * Per-field access to the chat's state.
 *
 * Rows are key patterns, not a field list: a pattern covers what it matches (and
 * `角色/**` covers a whole branch), which is also how a user pastes a batch of
 * keys at once. Rows are shown grouped by the pattern's top-level segment —
 * presentation only, every edit keeps its original row index. Two ways to
 * populate the grid: paste key patterns, or expand the state declaration bound
 * to the current chat into one row per declared field.
 *
 * The switches are the whole feature and none of them is self-explanatory, so
 * the guide at the top says what each one does, and carries the same sentences
 * as hover titles on the boxes themselves.
 */

import { useState } from 'react';

import {
    isBuiltinProfile,
    parseNumberInput,
    type Tr,
} from './AgentSystemPanelContract';
import type { AgentSystemPanelController } from './AgentSystemPanelController';
import { stateAccessRows } from './profile-draft-ops';
import { groupStateAccessRows, type StateAccessRow, type StateAccessSlot } from './profile-state-access';
import type { ProfileSectionProps } from './ProfileResourcesOutputSections';
import { StateGuide } from './StateGuide';

type StateAccessSwitch = 'inject' | 'visible' | 'writable';

function stateAccessSwitchLabel(switchName: StateAccessSwitch) {
    switch (switchName) {
        case 'inject':
            return 'stateAccessInject' as const;
        case 'visible':
            return 'stateAccessVisible' as const;
        default:
            return 'stateAccessWritable' as const;
    }
}

/** What each switch does, said once: the guide lists it, the boxes hover it. */
const SWITCH_GUIDE_KEYS = {
    inject: 'stateAccessGuideInject',
    visible: 'stateAccessGuideVisible',
    writable: 'stateAccessGuideWritable',
} as const;

function asInjectionSlot(value: string): StateAccessSlot {
    return value === 'before' || value === 'after' ? value : 'atDepth';
}

function stateAccessSwitchPatch(
    switchName: StateAccessSwitch,
    value: boolean,
): Partial<StateAccessRow> {
    switch (switchName) {
        case 'inject':
            return { inject: value };
        case 'visible':
            return { visible: value };
        default:
            return { writable: value };
    }
}

function StateAccessRowLine({
    index,
    row,
    controller,
    tr,
    builtin,
}: {
    index: number;
    row: StateAccessRow;
    controller: AgentSystemPanelController;
    tr: Tr;
    builtin: boolean;
}) {
    const slot = row.injectSlot;
    return (
        <div className="ttas-state-access-row">
            <input
                className="text_pole"
                value={row.pattern}
                placeholder={tr('stateAccessPatternPlaceholder')}
                disabled={builtin}
                onChange={(event) => controller.updateStateAccessRow(index, {
                    pattern: event.target.value,
                })}
            />
            {(['inject', 'visible', 'writable'] as const).map((switchName) => (
                <label className="checkbox_label" key={switchName} title={tr(SWITCH_GUIDE_KEYS[switchName])}>
                    <input
                        type="checkbox"
                        checked={row[switchName]}
                        disabled={builtin}
                        onChange={(event) => controller.updateStateAccessRow(
                            index,
                            stateAccessSwitchPatch(switchName, event.target.checked),
                        )}
                    />
                    <span>{tr(stateAccessSwitchLabel(switchName))}</span>
                </label>
            ))}
            <select
                className="text_pole"
                value={slot}
                title={tr('stateAccessSlot')}
                disabled={builtin || !row.inject}
                onChange={(event) => controller.updateStateAccessRow(index, {
                    injectSlot: asInjectionSlot(event.target.value),
                })}
            >
                <option value="before">{tr('stateAccessSlotBefore')}</option>
                <option value="after">{tr('stateAccessSlotAfter')}</option>
                <option value="atDepth">{tr('stateAccessSlotAtDepth')}</option>
            </select>
            <input
                className="text_pole"
                type="number"
                min="0"
                title={tr('stateAccessDepth')}
                value={row.injectDepth}
                disabled={builtin || !row.inject || slot !== 'atDepth'}
                onChange={(event) => controller.updateStateAccessRow(index, {
                    injectDepth: parseNumberInput(event.target.value),
                })}
            />
            <button
                type="button"
                className="menu_button ttas-state-icon-button"
                title={tr('delete')}
                disabled={builtin}
                onClick={() => controller.removeStateAccessRow(index)}
            >
                <i className="fa-solid fa-xmark"></i>
            </button>
        </div>
    );
}

export function ProfileStateAccessSection({ snapshot, controller, tr }: ProfileSectionProps) {
    const { draft } = snapshot;
    const builtin = isBuiltinProfile(draft);
    const [patternsCsv, setPatternsCsv] = useState('');
    const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
    const rows = stateAccessRows(draft);
    const groups = groupStateAccessRows(rows);

    function toggleGroup(key: string): void {
        const next = new Set(collapsed);
        if (next.has(key)) {
            next.delete(key);
        } else {
            next.add(key);
        }
        setCollapsed(next);
    }

    return (
        <div className="ttas-section" data-ttas-profile-section="state-access">
            <div className="ttas-section-title">
                <i className="fa-solid fa-clipboard-list"></i>
                <h4>{tr('stateAccess')}</h4>
            </div>
            <StateGuide
                title={tr('stateAccessGuideTitle')}
                lead={tr('stateAccessHint')}
                points={[
                    tr('stateAccessGuideInject'),
                    tr('stateAccessGuideVisible'),
                    tr('stateAccessGuideWritable'),
                    tr('stateAccessGuideGate'),
                    tr('stateAccessGuideHowTo'),
                ]}
            />
            {rows.length === 0 && <p className="ttas-field-hint">{tr('stateAccessNone')}</p>}
            {groups.map((group) => {
                const key = group.category ?? 'other';
                const isCollapsed = collapsed.has(key);
                const label = group.regex
                    ? tr('stateAccessGroupRegex')
                    : (group.category ?? tr('stateAccessGroupOther'));
                return (
                    <div className="ttas-state-access-group" key={key}>
                        <button
                            type="button"
                            className="menu_button ttas-state-access-group-toggle"
                            onClick={() => toggleGroup(key)}
                        >
                            <i className={`fa-solid ${isCollapsed ? 'fa-chevron-right' : 'fa-chevron-down'}`}></i>
                            <span>{label}</span>
                            <span className="ttas-field-hint">{group.rows.length}</span>
                        </button>
                        {!isCollapsed && group.rows.map(({ row, index }) => (
                            <StateAccessRowLine
                                key={index}
                                index={index}
                                row={row}
                                controller={controller}
                                tr={tr}
                                builtin={builtin}
                            />
                        ))}
                    </div>
                );
            })}
            {rows.length > 0 && <p className="ttas-field-hint">{tr('stateAccessSlotHint')}</p>}
            {snapshot.chatDeclarationName && (
                <p className="ttas-field-hint">
                    {tr('stateAccessChatExpanded', { name: snapshot.chatDeclarationName })}
                </p>
            )}
            <div className="ttas-state-new">
                <label className="ttas-field">
                    <span>{tr('stateAccessExpand')}</span>
                    <select
                        className="text_pole"
                        value={snapshot.stateDeclarationChoice}
                        disabled={builtin || snapshot.stateDeclarationNames.length === 0}
                        onChange={(event) => controller.setStateDeclarationChoice(event.target.value)}
                    >
                        {snapshot.stateDeclarationNames.length === 0 && (
                            <option value="">{tr('stateAccessDeclarationNone')}</option>
                        )}
                        {snapshot.stateDeclarationNames.map((name) => (
                            <option key={name} value={name}>{name}</option>
                        ))}
                    </select>
                </label>
                <button
                    type="button"
                    className="menu_button"
                    disabled={builtin || !snapshot.stateDeclarationChoice}
                    onClick={() => void controller.expandStateAccessFromDeclaration()}
                >
                    <i className="fa-solid fa-list-ul"></i>
                    <span>{tr('stateAccessExpandRows')}</span>
                </button>
                <button
                    type="button"
                    className="menu_button"
                    disabled={builtin}
                    title={tr('stateAccessExpandChatHint')}
                    onClick={() => void controller.expandStateAccessFromChatDeclaration()}
                >
                    <i className="fa-solid fa-comments"></i>
                    <span>{tr('stateAccessExpandChat')}</span>
                </button>
            </div>
            <p className="ttas-field-hint">{tr('stateAccessExpandHint')}</p>
            <div className="ttas-state-new">
                <label className="ttas-field">
                    <span>{tr('stateAccessAddPatterns')}</span>
                    <input
                        className="text_pole"
                        value={patternsCsv}
                        placeholder={tr('stateAccessAddPatternsPlaceholder')}
                        disabled={builtin}
                        onChange={(event) => setPatternsCsv(event.target.value)}
                    />
                </label>
                <button
                    type="button"
                    className="menu_button"
                    disabled={builtin}
                    onClick={() => {
                        controller.addStateAccessRows(patternsCsv);
                        setPatternsCsv('');
                    }}
                >
                    <i className="fa-solid fa-plus"></i>
                    <span>{tr('stateAccessAdd')}</span>
                </button>
                <button
                    type="button"
                    className="menu_button"
                    disabled={builtin}
                    onClick={() => controller.addStateAccessRows()}
                >
                    <span>{tr('stateAccessAddOne')}</span>
                </button>
            </div>
        </div>
    );
}
