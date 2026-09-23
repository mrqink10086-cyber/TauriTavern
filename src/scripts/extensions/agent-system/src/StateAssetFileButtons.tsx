/**
 * Bringing an asset in from a file, and taking one out.
 *
 * The host's own file dialog does the choosing: a hidden input is the portable
 * way to ask for one, and the same code opens the system dialog inside the
 * desktop runtime. Choosing the same file twice in a row must fire again, which
 * is why the input is cleared before the read starts.
 */

import { useRef, useState, type ChangeEvent } from 'react';
import type { AgentSystemMessageKey } from './i18n';
import type { Tr } from './AgentSystemPanelContract';

export function ImportAssetButton({
    tr,
    label,
    accept,
    disabled = false,
    onText,
}: {
    tr: Tr;
    label: AgentSystemMessageKey;
    /** What the dialog offers, as the input's `accept` attribute. */
    accept: string;
    disabled?: boolean;
    /** The chosen file's text and name; the editor decides what to make of it. */
    onText: (text: string, fileName: string) => void | Promise<void>;
}) {
    const fileInput = useRef<HTMLInputElement | null>(null);
    const [reading, setReading] = useState(false);

    async function onFileChosen(event: ChangeEvent<HTMLInputElement>): Promise<void> {
        const file = event.target.files?.[0];
        event.target.value = '';
        if (!file) {
            return;
        }
        setReading(true);
        try {
            await onText(await file.text(), file.name);
        } finally {
            setReading(false);
        }
    }

    return (
        <>
            <button
                type="button"
                className="menu_button"
                disabled={disabled || reading}
                onClick={() => fileInput.current?.click()}
            >
                <i className="fa-solid fa-file-import"></i>
                <span>{tr(label)}</span>
            </button>
            <input
                ref={fileInput}
                type="file"
                accept={accept}
                hidden
                onChange={(event) => void onFileChosen(event)}
            />
        </>
    );
}

export function ExportAssetButton({
    tr,
    label,
    disabled = false,
    onClick,
}: {
    tr: Tr;
    label: AgentSystemMessageKey;
    disabled?: boolean;
    onClick: () => void;
}) {
    return (
        <button type="button" className="menu_button" disabled={disabled} onClick={onClick}>
            <i className="fa-solid fa-file-export"></i>
            <span>{tr(label)}</span>
        </button>
    );
}
