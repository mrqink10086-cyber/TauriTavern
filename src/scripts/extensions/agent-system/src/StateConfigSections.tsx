/**
 * Two sections of the state declaration editor that stand on their own.
 *
 * They were part of the panel until it grew past the file-size limit, and both
 * read the draft without owning any of it: the theme box edits a stylesheet, the
 * issue list repeats what the save would refuse.
 */

import type { StateConfigController } from './state-config-controller';
import type { StateConfigIssue, StateDeclaration } from './state-config-model';
import type { Tr } from './AgentSystemPanelContract';

/**
 * The theme stylesheet: the second of the panel's three style levels.
 *
 * The hint carries the two things a first-time writer cannot guess — that the
 * selectors are prefixed for them, and which at-rules are refused — and the
 * check list below carries the rest. The box holds the source as typed; the
 * compiled sheet is what gets stored.
 */
export function ThemeSection({
    draft,
    controller,
    tr,
}: {
    draft: StateDeclaration;
    controller: StateConfigController;
    tr: Tr;
}) {
    const css = String(draft.panels?.css ?? '');
    return (
        <div className="ttas-section">
            <div className="ttas-section-title">
                <i className="fa-solid fa-palette"></i>
                <h4>{tr('stateDeclarationTheme')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('stateDeclarationThemeHint')}</p>
            <textarea
                className="text_pole ttas-state-script-source"
                rows={8}
                aria-label={tr('stateDeclarationTheme')}
                value={css}
                placeholder={tr('stateDeclarationThemePlaceholder')}
                onChange={(event) => controller.setThemeCss(event.target.value)}
            />
        </div>
    );
}

export function IssuesList({ issues, tr }: { issues: readonly StateConfigIssue[]; tr: Tr }) {
    if (issues.length === 0) {
        return null;
    }
    return (
        <div className="ttas-state-issues">
            <div className="ttas-section-title">
                <i className="fa-solid fa-circle-info"></i>
                <h4>{tr('stateDeclarationIssues')}</h4>
            </div>
            <ul>
                {issues.map((issue, index) => (
                    <li key={index}>
                        <code>{issue.path}</code>
                        <span>{issue.message}</span>
                    </li>
                ))}
            </ul>
        </div>
    );
}
