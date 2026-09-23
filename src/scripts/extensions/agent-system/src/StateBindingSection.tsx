import type { Tr } from './AgentSystemPanelContract';
import type { StateBindingScope, StateBindingView } from './state-binding';

export type StateBindingSectionProps = {
    /** What the panel has selected; nothing selected means nothing to bind. */
    selectedName: string;
    /** Where the selected document is bound, or null when the runtime is unreadable. */
    binding: StateBindingView | null;
    tr: Tr;
    onBind: (scope: StateBindingScope) => void;
};

/**
 * Where the selected document is bound, and the two buttons that change it.
 *
 * A binding belongs to the chat or to the character, not to this panel: the
 * buttons toggle it and the panel re-reads it, so the same document shows the
 * same binding wherever it is opened. Both buttons are toggles — pressing the
 * one that already binds the selection removes the binding, which is why the
 * locked icon appears there.
 *
 * When the page runtime cannot be read at all, `binding` is null and the section
 * says so instead of offering buttons: a binding this window cannot see is one it
 * must not offer to change.
 */
export function StateBindingSection({ selectedName, binding, tr, onBind }: StateBindingSectionProps) {
    if (!binding) {
        return (
            <div className="ttas-state-binding">
                <p className="ttas-field-hint">{tr('stateBindingUnavailable')}</p>
            </div>
        );
    }

    const selected = selectedName.trim();
    const chatBound = Boolean(selected) && binding.chatName === selected;
    const entityBound = Boolean(selected) && binding.entityName === selected;
    const entityLabel = binding.entityScope === 'group'
        ? tr('stateBindingGroup')
        : tr('stateBindingCharacter');

    return (
        <div className="ttas-state-binding">
            <div className="ttas-state-binding-head">
                <span>{tr('stateBindingLabel')}</span>
                <button
                    type="button"
                    className={`menu_button${chatBound ? ' active' : ''}`}
                    disabled={!selected || !binding.chatAvailable}
                    onClick={() => onBind('chat')}
                >
                    <i className={`fa-solid ${chatBound ? 'fa-lock' : 'fa-unlock'}`}></i>
                    <span>{tr('stateBindingChat')}</span>
                </button>
                <button
                    type="button"
                    className={`menu_button${entityBound ? ' active' : ''}`}
                    disabled={!selected || !binding.entityScope}
                    onClick={() => onBind('entity')}
                >
                    <i className={`fa-solid ${entityBound ? 'fa-lock' : 'fa-unlock'}`}></i>
                    <span>{entityLabel}</span>
                </button>
            </div>
            <p className="ttas-field-hint">
                {tr('stateBindingChatCurrent', { name: binding.chatName || tr('stateBindingNone') })}
                {' · '}
                {tr('stateBindingEntityCurrent', { name: binding.entityName || tr('stateBindingNone') })}
            </p>
        </div>
    );
}
