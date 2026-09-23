/**
 * The predicate tab: saved sets on the rail, one editor, one preview.
 *
 * Same contract as the declaration and machine tabs — the draft is local, the
 * save call normalizes, the backend refuses with its own words. Groups and
 * standing entries are edited in place; the preview runs the draft against
 * assumed state and never writes.
 */

import { DEFAULT_PREDICATE_NAME } from './state-examples';
import { StateBindingSection } from './StateBindingSection';
import { StateGuide } from './StateGuide';
import {
    predicateConfigBusy,
    predicateDraftIsDirty,
    type PredicateConfigController,
    type PredicateConfigSnapshot,
} from './state-predicate-controller';
import { PredicateConstantsSection, PredicateEntryCard } from './StatePredicateEntries';
import { PredicatePreviewSection } from './StatePredicatePreview';
import { StateDocumentJson } from './StateDocumentJson';
import { ExportAssetButton, ImportAssetButton } from './StateAssetFileButtons';
import type { Tr } from './AgentSystemPanelContract';

const ISSUE_KEYS = {
    duplicateGroupId: 'predicateIssueDuplicateGroupId',
    duplicateEntryId: 'predicateIssueDuplicateEntryId',
    entryConditionIncomplete: 'predicateIssueConditionIncomplete',
    effectTagsRequired: 'predicateIssueEffectTagsRequired',
} as const;

export function StatePredicatePanel({
    snapshot,
    controller,
    tr,
}: {
    snapshot: PredicateConfigSnapshot;
    controller: PredicateConfigController;
    tr: Tr;
}) {
    const busy = predicateConfigBusy(snapshot);
    const dirty = predicateDraftIsDirty(snapshot);
    const draft = snapshot.draft;

    return (
        <div className="ttas-state-layout">
            <aside className="ttas-list ttas-side-list">
                <div className="ttas-list-header">
                    <h4>{tr('statePredicatesTab')}</h4>
                    <span>{snapshot.names.length}</span>
                </div>
                {snapshot.names.length === 0 && <p className="ttas-field-hint">{tr('predicateNone')}</p>}
                {snapshot.names.map((name) => (
                    <button
                        key={name}
                        type="button"
                        className={snapshot.selectedName === name ? 'active' : ''}
                        onClick={() => void controller.selectSet(name)}
                    >
                        <strong>{name}</strong>
                    </button>
                ))}
            </aside>

            <section className="ttas-panel ttas-state-editor">
                <StateGuide
                    title={tr('predicateGuideTitle')}
                    lead={tr('predicateGuideLead')}
                    points={[
                        tr('predicateGuideGroups'),
                        tr('predicateGuideConstants'),
                        tr('predicateGuidePreview'),
                        tr('predicateGuideStart'),
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
                        <span>{tr('predicateName')}</span>
                        <input
                            className="text_pole"
                            value={snapshot.newName}
                            placeholder={DEFAULT_PREDICATE_NAME}
                            onChange={(event) => controller.setNewName(event.target.value)}
                        />
                    </label>
                    <button
                        type="button"
                        className="menu_button ttas-primary-button"
                        disabled={busy || !snapshot.newName.trim()}
                        onClick={() => void controller.createSet()}
                    >
                        <i className="fa-solid fa-plus"></i>
                        <span>{tr('predicateCreate')}</span>
                    </button>
                    <ImportAssetButton
                        tr={tr}
                        label="predicateImport"
                        accept="application/json,.json"
                        disabled={busy}
                        onText={controller.importPredicateSet}
                    />
                    {snapshot.selectedName && (
                        <button
                            type="button"
                            className="menu_button ttas-danger-button"
                            disabled={busy}
                            onClick={() => void controller.deleteSet()}
                        >
                            <i className="fa-solid fa-trash"></i>
                            <span>{tr('delete')}</span>
                        </button>
                    )}
                </div>

                {!draft && <p className="ttas-field-hint">{tr('predicatePick')}</p>}

                {draft && (
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

                        <div className="ttas-machine-images-head">
                            <span>{tr('predicateGroups')}</span>
                            <button
                                type="button"
                                className="menu_button"
                                disabled={busy}
                                onClick={() => controller.addGroup()}
                            >
                                <i className="fa-solid fa-plus"></i>
                                <span>{tr('predicateAddGroup')}</span>
                            </button>
                        </div>
                        <p className="ttas-field-hint">{tr('predicateGroupsHint')}</p>
                        {draft.groups.length === 0 && <p className="ttas-field-hint">{tr('predicateNone')}</p>}
                        {draft.groups.map((group, groupIndex) => (
                            <div className="ttas-predicate-group" key={groupIndex} data-ttas-predicate-group={group.id}>
                                <div className="ttas-machine-row">
                                    <input
                                        className="text_pole"
                                        aria-label={tr('predicateGroupId')}
                                        placeholder={tr('predicateGroupIdPlaceholder')}
                                        value={group.id}
                                        onChange={(event) => controller.updateGroup(groupIndex, { id: event.target.value })}
                                    />
                                    <input
                                        className="text_pole"
                                        aria-label={tr('predicateGroupLabel')}
                                        placeholder={tr('predicateGroupLabelPlaceholder')}
                                        value={group.labelText}
                                        onChange={(event) => controller.updateGroup(groupIndex, { labelText: event.target.value })}
                                    />
                                    <button
                                        type="button"
                                        className="menu_button ttas-machine-icon-button"
                                        title={tr('delete')}
                                        onClick={() => controller.removeGroup(groupIndex)}
                                    >
                                        <i className="fa-solid fa-xmark"></i>
                                    </button>
                                </div>
                                <div className="ttas-machine-images-head">
                                    <span>{tr('predicateGroupEntries')}</span>
                                    <button
                                        type="button"
                                        className="menu_button"
                                        onClick={() => controller.addEntry({ kind: 'group', groupIndex })}
                                    >
                                        <i className="fa-solid fa-plus"></i>
                                        <span>{tr('predicateAddEntry')}</span>
                                    </button>
                                </div>
                                {group.entries.length === 0 && <p className="ttas-field-hint">{tr('predicateNone')}</p>}
                                {group.entries.map((entry, entryIndex) => (
                                    <PredicateEntryCard
                                        key={entryIndex}
                                        target={{ kind: 'group', groupIndex }}
                                        entryIndex={entryIndex}
                                        entry={entry}
                                        controller={controller}
                                        tr={tr}
                                    />
                                ))}
                            </div>
                        ))}

                        <PredicateConstantsSection snapshot={snapshot} controller={controller} tr={tr} />

                        <PredicatePreviewSection snapshot={snapshot} controller={controller} tr={tr} />

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
                                {dirty ? tr('predicateUnsaved') : tr('predicateInSync')}
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
                                label="predicateExport"
                                disabled={busy}
                                onClick={() => void controller.exportPredicateSet()}
                            />
                        </div>
                    </>
                )}
            </section>
        </div>
    );
}
