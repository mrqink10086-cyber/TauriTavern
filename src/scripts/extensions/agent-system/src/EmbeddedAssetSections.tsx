import type { AgentSystemTr } from './i18n';

/**
 * The two shapes every carried asset is listed in.
 *
 * Five kinds of asset are carried the same way — pick one that is saved, carry
 * it, or take a carried one back off — so the markup lives here once instead of
 * once per kind. The panel only hands in what differs: the icon, the wording,
 * and the entries.
 */
export type EmbedAssetOption = { value: string; text: string };

export type EmbedAssetSectionProps = {
    icon: string;
    actionIcon: string;
    title: string;
    label: string;
    options: EmbedAssetOption[];
    selected: string;
    emptyHint: string;
    actionLabel: string;
    disabled: boolean;
    onSelect: (value: string) => void;
    onAction: () => void;
};

export function EmbedAssetSection(props: EmbedAssetSectionProps) {
    return (
        <section className="ttas-embed-card">
            <div className="ttas-embed-section-title">
                <i className={`fa-solid ${props.icon}`}></i>
                <h4>{props.title}</h4>
            </div>
            <div className="ttas-embed-action-row">
                <label className="ttas-field">
                    <span>{props.label}</span>
                    <select
                        value={props.selected}
                        disabled={props.disabled || props.options.length === 0}
                        onChange={(event) => props.onSelect(event.target.value)}
                    >
                        {props.options.map((option) => (
                            <option key={option.value} value={option.value}>{option.text}</option>
                        ))}
                    </select>
                </label>
                <button
                    type="button"
                    className="menu_button menu_button_icon ttas-primary-button"
                    disabled={props.disabled || !props.selected}
                    onClick={props.onAction}
                >
                    <i className={`fa-solid ${props.disabled ? 'fa-spinner fa-spin' : props.actionIcon}`}></i>
                    <span>{props.actionLabel}</span>
                </button>
            </div>
            {props.options.length === 0 && <p className="ttas-embed-empty">{props.emptyHint}</p>}
        </section>
    );
}

/** One carried asset, as the list shows it. */
export type EmbeddedEntry = {
    /** What a remove call takes: an id for some kinds, the name for the others. */
    key: string;
    name: string;
    subtitle: string;
    icon: string;
};

export type EmbeddedAssetGroupProps = {
    title: string;
    entries: EmbeddedEntry[];
    emptyHint: string;
    disabled: boolean;
    onRemove: (entry: EmbeddedEntry) => void;
    tr: AgentSystemTr;
};

export function EmbeddedAssetGroup(props: EmbeddedAssetGroupProps) {
    return (
        <div className="ttas-embedded-group">
            <h5>{props.title}</h5>
            {props.entries.length > 0 ? (
                <div className="ttas-embedded-list">
                    {props.entries.map((entry) => (
                        <div key={entry.key} className="ttas-embedded-item">
                            <i className={`fa-solid ${entry.icon}`}></i>
                            <div>
                                <strong>{entry.name}</strong>
                                <span>{entry.subtitle}</span>
                            </div>
                            <button
                                type="button"
                                className="menu_button menu_button_icon ttas-danger-button"
                                title={props.tr('removeEmbeddedAsset')}
                                aria-label={props.tr('removeEmbeddedAsset')}
                                disabled={props.disabled}
                                onClick={() => props.onRemove(entry)}
                            >
                                <i className="fa-solid fa-xmark"></i>
                            </button>
                        </div>
                    ))}
                </div>
            ) : (
                <p className="ttas-embed-empty">{props.emptyHint}</p>
            )}
        </div>
    );
}
