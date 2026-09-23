/**
 * A panel's prose block, as two inputs.
 *
 * A block of text somebody writes is not a field: it has no key, no switches and
 * no place in the value list, so it gets its own row rather than a place among
 * the fields. The path is the whole decision — a blank one means the panel has
 * no prose block at all.
 */

import type { Tr } from './AgentSystemPanelContract';
import type { StateConfigController } from './state-config-controller';
import type { StatePanelSpec } from './state-config-model';

export function PanelProseRow({
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
    return (
        <>
            <div className="ttas-state-row-fields">
                <input
                    className="text_pole"
                    value={panel.prose?.path ?? ''}
                    placeholder={tr('stateDeclarationProsePathPlaceholder')}
                    onChange={(event) => controller.updatePanelProse(panelIndex, { path: event.target.value })}
                />
                <input
                    className="text_pole"
                    value={panel.prose?.title ?? ''}
                    placeholder={tr('stateDeclarationProseTitle')}
                    onChange={(event) => controller.updatePanelProse(panelIndex, { title: event.target.value })}
                />
            </div>
            <p className="ttas-field-hint">{tr('stateDeclarationProseHint')}</p>
        </>
    );
}
