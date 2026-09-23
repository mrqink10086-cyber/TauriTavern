import type { AgentSystemTr } from './i18n';

/**
 * The whole document as text, for the editors whose tables grow by one row.
 *
 * A list of three hundred fields is not something a row-at-a-time form is good
 * at, so every one of these editors offers its document as JSON: paste a whole
 * document in, or take the current one out, edit it, and put it back.
 *
 * The box is a view of the draft — it is re-printed whenever the draft changes —
 * so what is typed here is only ever applied by the button below it. That is also
 * why a broken paste is reported in place: it leaves the draft exactly as it was.
 */
export type StateDocumentJsonProps = {
    value: string;
    error: string;
    readOnly: boolean;
    onChange: (value: string) => void;
    onRefresh: () => void;
    onApply: () => void;
    tr: AgentSystemTr;
};

export function StateDocumentJson({ value, error, readOnly, onChange, onRefresh, onApply, tr }: StateDocumentJsonProps) {
    return (
        <div className="ttas-section ttas-json-section">
            <div className="ttas-pane-header">
                <div className="ttas-section-title">
                    <i className="fa-solid fa-code"></i>
                    <h4>{tr('advancedJson')}</h4>
                </div>
                <div className="ttas-toolbar">
                    <button type="button" className="menu_button" disabled={readOnly} onClick={onRefresh}>
                        {tr('refreshJson')}
                    </button>
                    <button type="button" className="menu_button" disabled={readOnly} onClick={onApply}>
                        {tr('applyJson')}
                    </button>
                </div>
            </div>
            {error && (
                <div className="ttas-error">
                    <i className="fa-solid fa-triangle-exclamation"></i>
                    <span>{error}</span>
                </div>
            )}
            <textarea
                className="text_pole ttas-json"
                value={value}
                readOnly={readOnly}
                spellCheck={false}
                onChange={(event) => onChange(event.target.value)}
            ></textarea>
        </div>
    );
}
