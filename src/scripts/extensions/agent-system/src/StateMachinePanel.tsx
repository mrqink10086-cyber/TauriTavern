/**
 * The state machine tab: saved machines on the rail, one editor, one preview.
 *
 * Same contract as the declaration tab — the draft is local, the save call
 * normalizes, the backend refuses with its own words. Script hooks are not
 * edited here; a loaded hook is passed through untouched.
 */

import { machineConfigBusy, machineDraftIsDirty, type MachineConfigController, type MachineConfigSnapshot } from './state-machine-controller';
import { MachineTransitionCard } from './StateMachineTransitions';
import { MachinePreviewSection } from './StateMachinePreview';
import { StateDocumentJson } from './StateDocumentJson';
import { ExportAssetButton, ImportAssetButton } from './StateAssetFileButtons';
import { DEFAULT_MACHINE_NAME } from './state-examples';
import { StateBindingSection } from './StateBindingSection';
import { StateGuide } from './StateGuide';
import type { Tr } from './AgentSystemPanelContract';

const ISSUE_KEYS = {
    duplicateState: 'machineIssueDuplicateState',
    unknownState: 'machineIssueUnknownState',
    emptyTransition: 'machineIssueEmptyTransition',
} as const;

export function StateMachinePanel({
    snapshot,
    controller,
    tr,
}: {
    snapshot: MachineConfigSnapshot;
    controller: MachineConfigController;
    tr: Tr;
}) {
    const busy = machineConfigBusy(snapshot);
    const dirty = machineDraftIsDirty(snapshot);

    return (
        <div className="ttas-state-layout">
            <aside className="ttas-list ttas-side-list">
                <div className="ttas-list-header">
                    <h4>{tr('stateMachineTab')}</h4>
                    <span>{snapshot.names.length}</span>
                </div>
                {snapshot.names.length === 0 && <p className="ttas-field-hint">{tr('machineNone')}</p>}
                {snapshot.names.map((name) => (
                    <button
                        key={name}
                        type="button"
                        className={snapshot.selectedName === name ? 'active' : ''}
                        onClick={() => void controller.selectMachine(name)}
                    >
                        <strong>{name}</strong>
                    </button>
                ))}
            </aside>

            <section className="ttas-panel ttas-state-editor">
                <StateGuide
                    title={tr('machineGuideTitle')}
                    lead={tr('machineGuideLead')}
                    points={[
                        tr('machineGuideStates'),
                        tr('machineGuideTransitions'),
                        tr('machineGuidePreview'),
                        tr('machineGuideStart'),
                    ]}
                />
                <StateBindingSection
                    selectedName={snapshot.selectedName}
                    binding={snapshot.binding}
                    tr={tr}
                    onBind={(scope) => void controller.bindSelectedTo(scope)}
                />
                <div className="ttas-state-new">
                    <label className="ttas-field">
                        <span>{tr('machineName')}</span>
                        <input
                            className="text_pole"
                            value={snapshot.newName}
                            placeholder={DEFAULT_MACHINE_NAME}
                            onChange={(event) => controller.setNewName(event.target.value)}
                        />
                    </label>
                    <button
                        type="button"
                        className="menu_button ttas-primary-button"
                        disabled={busy || !snapshot.newName.trim()}
                        onClick={() => void controller.createMachine()}
                    >
                        <i className="fa-solid fa-plus"></i>
                        <span>{tr('machineCreate')}</span>
                    </button>
                    <ImportAssetButton
                        tr={tr}
                        label="machineImport"
                        accept="application/json,.json"
                        disabled={busy}
                        onText={controller.importMachine}
                    />
                    {snapshot.selectedName && (
                        <button
                            type="button"
                            className="menu_button ttas-danger-button"
                            disabled={busy}
                            onClick={() => void controller.deleteMachine()}
                        >
                            <i className="fa-solid fa-trash"></i>
                            <span>{tr('delete')}</span>
                        </button>
                    )}
                </div>

                {!snapshot.draft && <p className="ttas-field-hint">{tr('machinePick')}</p>}

                {snapshot.draft && (
                    <>
                        {snapshot.error && <p className="ttas-error">{snapshot.error}</p>}
                        {snapshot.notice && <p className="ttas-field-hint">{snapshot.notice}</p>}
                        {snapshot.issues.length > 0 && (
                            <div className="ttas-state-issues">
                                <strong>{tr('stateDeclarationIssues')}</strong>
                                <ul>
                                    {snapshot.issues.map((issue, index) => (
                                        <li key={index}>
                                            <code>{tr(ISSUE_KEYS[issue.code], { target: issue.target })}</code>
                                        </li>
                                    ))}
                                </ul>
                            </div>
                        )}

                        <label className="ttas-field">
                            <span>{tr('machineInitial')}</span>
                            <input
                                className="text_pole"
                                value={snapshot.draft.initialText}
                                placeholder="day"
                                onChange={(event) => controller.updateInitial(event.target.value)}
                            />
                        </label>
                        <p className="ttas-field-hint">{tr('machineInitialHint')}</p>

                        <div className="ttas-machine-images-head">
                            <span>{tr('machineStates')}</span>
                            <button
                                type="button"
                                className="menu_button"
                                disabled={busy}
                                onClick={() => controller.addState()}
                            >
                                <i className="fa-solid fa-plus"></i>
                                <span>{tr('machineAddState')}</span>
                            </button>
                        </div>
                        {snapshot.draft.states.map((state, index) => (
                            <div className="ttas-machine-row" key={index}>
                                <input
                                    className="text_pole"
                                    aria-label={tr('machineStateId')}
                                    placeholder={tr('machineStateIdPlaceholder')}
                                    value={state.id}
                                    onChange={(event) => controller.updateState(index, { id: event.target.value })}
                                />
                                <input
                                    className="text_pole"
                                    aria-label={tr('machineStateLabel')}
                                    placeholder={tr('machineStateLabelPlaceholder')}
                                    value={state.labelText}
                                    onChange={(event) => controller.updateState(index, { labelText: event.target.value })}
                                />
                                <label className="checkbox_label">
                                    <input
                                        type="checkbox"
                                        checked={state.terminal}
                                        onChange={(event) => controller.updateState(index, { terminal: event.target.checked })}
                                    />
                                    <span>{tr('machineStateTerminal')}</span>
                                </label>
                                <button
                                    type="button"
                                    className="menu_button ttas-machine-icon-button"
                                    title={tr('delete')}
                                    onClick={() => controller.removeState(index)}
                                >
                                    <i className="fa-solid fa-xmark"></i>
                                </button>
                            </div>
                        ))}

                        <div className="ttas-machine-images-head">
                            <span>{tr('machineTransitions')}</span>
                            <button
                                type="button"
                                className="menu_button"
                                disabled={busy}
                                onClick={() => controller.addTransition()}
                            >
                                <i className="fa-solid fa-plus"></i>
                                <span>{tr('machineAddTransition')}</span>
                            </button>
                        </div>
                        <p className="ttas-field-hint">{tr('machineTransitionsHint')}</p>
                        {snapshot.draft.transitions.map((transition, index) => (
                            <MachineTransitionCard
                                key={index}
                                transitionIndex={index}
                                transition={transition}
                                controller={controller}
                                tr={tr}
                            />
                        ))}

                        <MachinePreviewSection snapshot={snapshot} controller={controller} tr={tr} />

                        <StateDocumentJson
                            value={snapshot.draftJson}
                            error={snapshot.jsonError}
                            readOnly={busy}
                            onChange={controller.setDraftJson}
                            onRefresh={controller.refreshDraftJson}
                            onApply={controller.applyDraftJson}
                            tr={tr}
                        />

                        <div className="ttas-state-footer">
                            <span className="ttas-field-hint">
                                {dirty ? tr('machineUnsaved') : tr('machineInSync')}
                            </span>
                            <button
                                type="button"
                                className="menu_button ttas-primary-button"
                                disabled={busy}
                                onClick={() => void controller.save()}
                            >
                                <i className="fa-solid fa-floppy-disk"></i>
                                <span>{tr('save')}</span>
                            </button>
                            <ExportAssetButton
                                tr={tr}
                                label="machineExport"
                                disabled={busy}
                                onClick={() => void controller.exportMachine()}
                            />
                        </div>
                    </>
                )}
            </section>
        </div>
    );
}
