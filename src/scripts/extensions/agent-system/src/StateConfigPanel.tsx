import {
    isScriptModuleName,
    type StateDeclaration,
    type StatePanelSpec,
} from './state-config-model';
import { ExportAssetButton, ImportAssetButton } from './StateAssetFileButtons';
import { StateBindingSection } from './StateBindingSection';
import { IssuesList, ThemeSection } from './StateConfigSections';
import { ImageSetEditor, RemoveRowButton } from './StateConfigBits';
import { DeclarationFieldsSection } from './StateDeclarationFields';
import { StateLimitsSection } from './StateLimitsSection';
import { StateDocumentJson } from './StateDocumentJson';
import { PanelProseRow } from './StatePanelProseRow';
import { scriptModulesOf } from './state-config-ops';
import { DEFAULT_DECLARATION_NAME } from './state-examples';
import { StateGuide } from './StateGuide';
import {
    stateConfigBusy,
    stateConfigDraftIsDirty,
    type StateConfigController,
    type StateConfigSnapshot,
} from './state-config-controller';
import type { Tr } from './AgentSystemPanelContract';

export type StateConfigPanelProps = {
    snapshot: StateConfigSnapshot;
    controller: StateConfigController;
    tr: Tr;
};

/**
 * The state declaration editor.
 *
 * The shape checks shown here are the cheap ones; every semantic rule — an
 * overlapping panel, a fallback candidate that can never be reached, an unknown
 * comparison — is answered by the save call, and its message is what the user
 * reads.
 */
export function StateConfigPanel({ snapshot, controller, tr }: StateConfigPanelProps) {
    const dirty = stateConfigDraftIsDirty(snapshot);
    const busy = stateConfigBusy(snapshot);
    const { draft } = snapshot;

    return (
        <div className="ttas-state-layout">
            <aside className="ttas-list ttas-side-list">
                <div className="ttas-list-header">
                    <h4>{tr('stateDeclarations')}</h4>
                    <span>{snapshot.names.length}</span>
                </div>
                {snapshot.names.length === 0 && <p className="ttas-empty">{tr('stateDeclarationNone')}</p>}
                {snapshot.names.map((name) => (
                    <button
                        key={name}
                        type="button"
                        className={snapshot.selectedName === name ? 'active' : ''}
                        onClick={() => void controller.selectDeclaration(name)}
                    >
                        <strong>{name}</strong>
                    </button>
                ))}
            </aside>

            <section className="ttas-panel ttas-state-editor">
                <StateGuide
                    title={tr('stateDeclarationGuideTitle')}
                    lead={tr('stateDeclarationGuideLead')}
                    points={[
                        tr('stateDeclarationGuideWrite'),
                        tr('stateDeclarationGuidePanel'),
                        tr('stateDeclarationGuideMachine'),
                        tr('stateDeclarationGuideStart'),
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
                        <span>{tr('stateDeclarationName')}</span>
                        <input
                            className="text_pole"
                            value={snapshot.newName}
                            placeholder={DEFAULT_DECLARATION_NAME}
                            onChange={(event) => controller.setNewName(event.target.value)}
                        />
                    </label>
                    <button
                        type="button"
                        className="menu_button ttas-primary-button"
                        disabled={busy || !snapshot.newName.trim()}
                        onClick={() => void controller.createDeclaration()}
                    >
                        <i className="fa-solid fa-plus"></i>
                        <span>{tr('stateDeclarationCreate')}</span>
                    </button>
                    <button
                        type="button"
                        className="menu_button"
                        disabled={busy || !snapshot.newName.trim()}
                        onClick={() => void controller.createDeclaration('seraphina')}
                    >
                        <i className="fa-solid fa-user-nurse"></i>
                        <span>{tr('stateDeclarationCreateSeraphina')}</span>
                    </button>
                    <ImportAssetButton
                        tr={tr}
                        label="stateDeclarationImport"
                        accept="application/json,.json"
                        disabled={busy}
                        onText={controller.importDeclaration}
                    />
                </div>
                {snapshot.error && (
                    <div className="ttas-error">
                        <i className="fa-solid fa-triangle-exclamation"></i>
                        <pre>{snapshot.error}</pre>
                    </div>
                )}
                {snapshot.notice && (
                    <p className="ttas-state-notice">
                        <i className="fa-solid fa-circle-check"></i>
                        {snapshot.notice}
                    </p>
                )}
                {!draft ? (
                    <p className="ttas-empty">{tr('stateDeclarationPick')}</p>
                ) : (
                    <>
                        <DeclarationFieldsSection draft={draft} controller={controller} tr={tr} />
                        <StateLimitsSection draft={draft} controller={controller} tr={tr} />
                        <SharedScriptsSection draft={draft} snapshot={snapshot} controller={controller} tr={tr} />
                        <PanelsSection draft={draft} controller={controller} tr={tr} />
                        <ThemeSection draft={draft} controller={controller} tr={tr} />
                        <StateDocumentJson
                            value={snapshot.draftJson}
                            error={snapshot.jsonError}
                            readOnly={busy}
                            onChange={controller.setDraftJson}
                            onRefresh={controller.refreshDraftJson}
                            onApply={controller.applyDraftJson}
                            tr={tr}
                        />
                        <IssuesList issues={snapshot.issues} tr={tr} />
                        <footer className="ttas-state-footer">
                            <span className="ttas-field-hint">
                                {dirty ? tr('stateDeclarationUnsaved') : tr('stateDeclarationInSync')}
                            </span>
                            <button
                                type="button"
                                className="menu_button ttas-primary-button"
                                disabled={busy || !dirty}
                                onClick={() => void controller.save()}
                            >
                                <i className="fa-solid fa-floppy-disk"></i>
                                <span>{tr('save')}</span>
                            </button>
                            <ExportAssetButton
                                tr={tr}
                                label="stateDeclarationExport"
                                disabled={busy}
                                onClick={() => void controller.exportDeclaration()}
                            />
                            <button
                                type="button"
                                className="menu_button ttas-danger-button"
                                disabled={busy}
                                onClick={() => void controller.deleteDeclaration()}
                            >
                                <i className="fa-solid fa-trash"></i>
                                <span>{tr('delete')}</span>
                            </button>
                        </footer>
                    </>
                )}
            </section>
        </div>
    );
}

/**
 * The modules picture sets may import.
 *
 * One place for logic several sets need: a set's script stays one line while
 * the rules live here, once.
 */
function SharedScriptsSection({
    draft,
    snapshot,
    controller,
    tr,
}: {
    draft: StateDeclaration;
    snapshot: StateConfigSnapshot;
    controller: StateConfigController;
    tr: Tr;
}) {
    const modules = Object.entries(scriptModulesOf(draft));
    const newName = snapshot.newScriptName.trim();
    return (
        <div className="ttas-section">
            <div className="ttas-section-title">
                <i className="fa-solid fa-code"></i>
                <h4>{tr('stateDeclarationScripts')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('stateDeclarationScriptsHint')}</p>
            {modules.length === 0 && <p className="ttas-field-hint">{tr('stateDeclarationScriptNone')}</p>}
            {modules.map(([name, source]) => (
                <div className="ttas-state-script-card" key={name}>
                    <div className="ttas-state-script-head">
                        <code>{name}</code>
                        <RemoveRowButton
                            label={tr('delete')}
                            onClick={() => controller.removeScriptModule(name)}
                        />
                    </div>
                    <textarea
                        className="text_pole ttas-state-script-source"
                        rows={4}
                        value={source}
                        onChange={(event) => controller.setScriptModuleSource(name, event.target.value)}
                    />
                </div>
            ))}
            <div className="ttas-state-new">
                <label className="ttas-field">
                    <span>{tr('stateDeclarationScriptName')}</span>
                    <input
                        className="text_pole"
                        value={snapshot.newScriptName}
                        placeholder={tr('stateDeclarationScriptNamePlaceholder')}
                        onChange={(event) => controller.setNewScriptName(event.target.value)}
                    />
                </label>
                <button
                    type="button"
                    className="menu_button"
                    disabled={!isScriptModuleName(newName)}
                    onClick={() => controller.createScriptModule()}
                >
                    <i className="fa-solid fa-plus"></i>
                    <span>{tr('stateDeclarationAddScript')}</span>
                </button>
                <ImportAssetButton
                    tr={tr}
                    label="stateDeclarationScriptImport"
                    accept="text/javascript,.js"
                    onText={(text, fileName) => controller.importScriptModule(fileName, text)}
                />
            </div>
        </div>
    );
}

function PanelsSection({
    draft,
    controller,
    tr,
}: {
    draft: StateDeclaration;
    controller: StateConfigController;
    tr: Tr;
}) {
    const panels = draft.panels?.panels ?? [];
    return (
        <div className="ttas-section">
            <div className="ttas-section-title">
                <i className="fa-solid fa-table-columns"></i>
                <h4>{tr('stateDeclarationPanels')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('stateDeclarationPanelsHint')}</p>
            {panels.map((panel, panelIndex) => (
                <PanelCard
                    key={panelIndex}
                    panel={panel}
                    panelIndex={panelIndex}
                    controller={controller}
                    tr={tr}
                />
            ))}
            <button type="button" className="menu_button" onClick={() => controller.addPanel()}>
                <i className="fa-solid fa-plus"></i>
                <span>{tr('stateDeclarationAddPanel')}</span>
            </button>
        </div>
    );
}

function PanelCard({
    panel,
    panelIndex,
    controller,
    tr,
}: {
    panel: StatePanelSpec;
    panelIndex: number;
    controller: StateConfigController;
    tr: Tr;
}) {
    const fields = panel.fields ?? [];
    return (
        <div className="ttas-state-card">
            <div className="ttas-state-row-fields">
                <input
                    className="text_pole"
                    value={panel.title}
                    placeholder={tr('stateDeclarationPanelTitle')}
                    onChange={(event) => controller.updatePanel(panelIndex, { title: event.target.value })}
                />
                <input
                    className="text_pole"
                    value={panel.match}
                    placeholder={tr('stateDeclarationPatternPlaceholder')}
                    onChange={(event) => controller.updatePanel(panelIndex, { match: event.target.value })}
                />
                <select
                    className="text_pole"
                    value={panel.rail ?? 'left'}
                    onChange={(event) => controller.updatePanel(panelIndex, {
                        rail: event.target.value === 'right' ? 'right' : 'left',
                    })}
                >
                    <option value="left">{tr('stateDeclarationRailLeft')}</option>
                    <option value="right">{tr('stateDeclarationRailRight')}</option>
                </select>
                <RemoveRowButton label={tr('delete')} onClick={() => controller.removePanel(panelIndex)} />
            </div>

            <PanelProseRow panel={panel} panelIndex={panelIndex} controller={controller} tr={tr} />

            <ImageSetEditor
                label={tr('stateDeclarationPanelBackground')}
                images={panel.background ?? null}
                target={{ kind: 'background', panelIndex }}
                controller={controller}
                tr={tr}
            />

            {fields.map((field, fieldIndex) => (
                <div className="ttas-state-card-field" key={fieldIndex}>
                    <div className="ttas-state-row-fields">
                        <input
                            className="text_pole"
                            value={field.pattern}
                            placeholder={tr('stateDeclarationPatternPlaceholder')}
                            onChange={(event) => controller.updatePanelField(panelIndex, fieldIndex, {
                                pattern: event.target.value,
                            })}
                        />
                        <input
                            className="text_pole"
                            value={field.label ?? ''}
                            placeholder={tr('stateDeclarationLabelPlaceholder')}
                            onChange={(event) => controller.updatePanelField(panelIndex, fieldIndex, {
                                label: event.target.value,
                            })}
                        />
                        <select
                            className="text_pole"
                            value={field.render ?? 'text'}
                            onChange={(event) => controller.updatePanelField(panelIndex, fieldIndex, {
                                render: event.target.value === 'image' ? 'image' : 'text',
                            })}
                        >
                            <option value="text">{tr('stateDeclarationRenderText')}</option>
                            <option value="image">{tr('stateDeclarationRenderImage')}</option>
                        </select>
                        <RemoveRowButton
                            label={tr('delete')}
                            onClick={() => controller.removePanelField(panelIndex, fieldIndex)}
                        />
                    </div>
                    <ImageSetEditor
                        label={tr('stateDeclarationFieldImages')}
                        images={field.images ?? null}
                        target={{ kind: 'field', panelIndex, fieldIndex }}
                        controller={controller}
                        tr={tr}
                    />
                </div>
            ))}
            <button type="button" className="menu_button" onClick={() => controller.addPanelField(panelIndex)}>
                <i className="fa-solid fa-plus"></i>
                <span>{tr('stateDeclarationAddField')}</span>
            </button>

            <label className="ttas-field ttas-state-script">
                <span>{tr('stateDeclarationTemplate')}</span>
                <textarea
                    className="text_pole ttas-state-script-source"
                    rows={6}
                    value={String(panel.templateSource ?? '')}
                    placeholder={tr('stateDeclarationTemplatePlaceholder')}
                    onChange={(event) => controller.setPanelTemplate(panelIndex, event.target.value)}
                />
            </label>
            <p className="ttas-field-hint">{tr('stateDeclarationTemplateHint')}</p>
        </div>
    );
}
