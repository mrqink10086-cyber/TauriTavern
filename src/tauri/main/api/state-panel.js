// @ts-check

/**
 * The panel's data, straight from the host.
 *
 * The declaration travels with the request because the host owns its binding
 * (chat metadata, then character/group) and the panel reads the same declaration
 * the model does: one key space, one set of display names, one page of images.
 *
 * @param {{
 *   chatRef: any;
 *   stableChatId: string;
 *   declaration: any;
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 * }} input
 * @returns {Promise<{ stateId: string | null; panels: any[]; fields: any[]; themeCss: string; stateUpdated: boolean }>}
 */
export async function loadStatePanel(input) {
    const result = await input.safeInvoke('get_state_panel', {
        dto: {
            chatRef: input.chatRef,
            stableChatId: input.stableChatId,
            declaration: input.declaration,
        },
    });

    return {
        stateId: typeof result?.stateId === 'string' ? result.stateId : null,
        panels: Array.isArray(result?.panels) ? result.panels : [],
        // The chat's whole key space, which a panel's template may bind beyond
        // the rows its own `match` covers.
        fields: Array.isArray(result?.fields) ? result.fields : [],
        // The theme is text that was compiled when it was saved; a missing one
        // simply means this declaration has no theme.
        themeCss: typeof result?.themeCss === 'string' ? result.themeCss : '',
        // Only an explicit false marks the newest floor as one that was expected
        // to update state and did not; anything else (an older chat, a normal
        // reply) means the question was never asked.
        stateUpdated: result?.stateUpdated !== false,
    };
}

/**
 * The panel's prose write path.
 *
 * A prose block is a file, not a field: no key, no three states, no value
 * limit. It is named by the path the panel read it from, and the host refuses
 * a path this chat's declaration does not name. Clearing sends an empty text,
 * which is the same thing to every reader.
 *
 * @param {{
 *   chatRef: any;
 *   stableChatId: string;
 *   declaration: any;
 *   path: string;
 *   text: string;
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 * }} input
 * @returns {Promise<{ stateId: string; baseStateId: string | null; changed: boolean; path: string }>}
 */
export async function updateStateProse(input) {
    const dto = {
        chatRef: input.chatRef,
        stableChatId: input.stableChatId,
        declaration: input.declaration,
        path: String(input.path ?? ''),
        text: String(input.text ?? ''),
    };

    const result = await input.safeInvoke('update_state_prose', { dto });

    return {
        stateId: String(result?.stateId ?? ''),
        baseStateId: typeof result?.baseStateId === 'string' ? result.baseStateId : null,
        changed: result?.changed === true,
        path: String(result?.path ?? dto.path),
    };
}

/**
 * The panel's write path.
 *
 * A person deciding a value in the interface is the writer: the per-field
 * writable authorization that bounds a model's own writes does not apply, and
 * the declaration does. The result names the published version, which the caller
 * binds to the floor it edited.
 *
 * @param {{
 *   chatRef: any;
 *   stableChatId: string;
 *   declaration: any;
 *   machine?: any;
 *   fields?: Array<{ key: string; value: string[] }>;
 *   remove?: string[];
 *   recalculate?: boolean;
 *   tokenizerModel?: string | null;
 *   safeInvoke: (command: string, args?: any) => Promise<any>;
 * }} input
 * @returns {Promise<{ stateId: string; baseStateId: string | null; changed: boolean; changeCount: number; writtenKeys: string[]; recalculatedKeys: string[] }>}
 */
export async function updateStateValues(input) {
    /** @type {{ chatRef: any; stableChatId: string; declaration: any; machine?: any; tokenizerModel?: string; fields: Array<{ key: string; value: string[] }>; remove: string[]; recalculate: boolean }} */
    const dto = {
        chatRef: input.chatRef,
        stableChatId: input.stableChatId,
        declaration: input.declaration,
        fields: Array.isArray(input.fields) ? input.fields : [],
        remove: Array.isArray(input.remove) ? input.remove : [],
        recalculate: input.recalculate === true,
    };
    if (input.machine) {
        dto.machine = input.machine;
    }
    // A scene may follow the model to pick the vocabulary its token ceilings are
    // counted in, and a click has no run behind it to read that from.
    const tokenizerModel = String(input.tokenizerModel ?? '').trim();
    if (tokenizerModel) {
        dto.tokenizerModel = tokenizerModel;
    }

    const result = await input.safeInvoke('update_state_values', { dto });

    return {
        stateId: String(result?.stateId ?? ''),
        baseStateId: typeof result?.baseStateId === 'string' ? result.baseStateId : null,
        changed: result?.changed === true,
        changeCount: Number(result?.changeCount ?? 0),
        writtenKeys: Array.isArray(result?.writtenKeys) ? result.writtenKeys : [],
        recalculatedKeys: Array.isArray(result?.recalculatedKeys) ? result.recalculatedKeys : [],
    };
}
