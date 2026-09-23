/**
 * The declared fields of a scene, with the switches each one carries.
 *
 * A field's own switches are the default every Agent gets; a Profile's access
 * rows are the exceptions. Keeping them on the field row is what makes the field
 * list answer "what is this and who gets it" in one place.
 */

import { RemoveRowButton } from './StateConfigBits';
import type { Tr } from './AgentSystemPanelContract';
import type { StateConfigController } from './state-config-controller';
import type { StateDeclaration } from './state-config-model';
import { fieldAccessOf } from './state-field-access';

/**
 * One switch on a field row.
 *
 * A field carries its own answer to "is this pushed into the prompt, may the
 * model change it", so the decision lives where the field is named rather than
 * in a second list a reader has to correlate. A Profile can still override it;
 * this is the default it overrides.
 */
function FieldSwitch({
    id,
    checked,
    label,
    onChange,
}: {
    id: string;
    checked: boolean;
    label: string;
    onChange: (checked: boolean) => void;
}) {
    return (
        <label className="ttas-state-row-switch" htmlFor={id}>
            <input
                id={id}
                type="checkbox"
                checked={checked}
                onChange={(event) => onChange(event.target.checked)}
            />
            <span>{label}</span>
        </label>
    );
}

export function DeclarationFieldsSection({
    draft,
    controller,
    tr,
}: {
    draft: StateDeclaration;
    controller: StateConfigController;
    tr: Tr;
}) {
    return (
        <div className="ttas-section">
            <div className="ttas-section-title">
                <i className="fa-solid fa-list"></i>
                <h4>{tr('stateDeclarationFields')}</h4>
            </div>
            <p className="ttas-field-hint">{tr('stateDeclarationFieldsHint')}</p>
            {draft.fields.map((field, index) => {
                const access = fieldAccessOf(field);
                return (
                    <div className="ttas-state-field-row" key={index}>
                        <div className="ttas-state-row-inputs">
                            <input
                                className="text_pole"
                                value={field.pattern}
                                placeholder={tr('stateDeclarationPatternPlaceholder')}
                                onChange={(event) => controller.updateField(index, { pattern: event.target.value })}
                            />
                            <input
                                className="text_pole"
                                value={field.label}
                                placeholder={tr('stateDeclarationLabelPlaceholder')}
                                onChange={(event) => controller.updateField(index, { label: event.target.value })}
                            />
                            <RemoveRowButton label={tr('delete')} onClick={() => controller.removeField(index)} />
                        </div>
                        <div className="ttas-state-row-switches">
                            <label className="ttas-state-row-initial">
                                <span>{tr('stateDeclarationInitial')}</span>
                                <input
                                    className="text_pole"
                                    value={(field.initial ?? []).join(', ')}
                                    placeholder={tr('stateDeclarationInitialPlaceholder')}
                                    onChange={(event) => controller.updateFieldInitial(index, event.target.value)}
                                />
                            </label>
                            <FieldSwitch
                                id={`ttas-state-field-${index}-inject`}
                                checked={access.inject}
                                label={tr('stateAccessInject')}
                                onChange={(checked) => controller.updateFieldAccess(index, { inject: checked })}
                            />
                            <FieldSwitch
                                id={`ttas-state-field-${index}-visible`}
                                checked={access.visible}
                                label={tr('stateAccessVisible')}
                                onChange={(checked) => controller.updateFieldAccess(index, { visible: checked })}
                            />
                            <FieldSwitch
                                id={`ttas-state-field-${index}-writable`}
                                checked={access.writable}
                                label={tr('stateAccessWritable')}
                                onChange={(checked) => controller.updateFieldAccess(index, { writable: checked })}
                            />
                            <span className="ttas-state-row-switches-hint">{tr('stateDeclarationFieldAccessHint')}</span>
                        </div>
                    </div>
                );
            })}
            <button type="button" className="menu_button" onClick={() => controller.addField()}>
                <i className="fa-solid fa-plus"></i>
                <span>{tr('stateDeclarationAddField')}</span>
            </button>
        </div>
    );
}

