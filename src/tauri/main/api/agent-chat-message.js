// @ts-check

const AUTO_COMMIT_TEXT_EXTENSIONS = new Set(['md', 'markdown', 'txt', 'text']);

/**
 * What a saved reply looks like after SillyTavern's own cleanup.
 *
 * @param {any} script the ST script context
 * @param {string} rawText
 * @param {string} [generationType]
 * @returns {string}
 */
export function prepareGeneratedReplyForSave(script, rawText, generationType) {
    // saveReply is a low-level chat writer. Legacy generation runs cleanup
    // before saveReply, so Agent commit must preserve that boundary here.
    const type = String(generationType || 'normal').trim() || 'normal';
    return script.cleanUpMessage({
        getMessage: rawText,
        isImpersonate: type === 'impersonate',
        isContinue: type === 'continue',
        displayIncompleteSentences: false,
    });
}

/**
 * The same text as the user sees it, without the trailing-sentence cleanup.
 *
 * @param {any} script
 * @param {string} rawText
 * @param {string} [generationType]
 * @returns {string}
 */
export function prepareGeneratedReplyForDisplay(script, rawText, generationType) {
    const type = String(generationType || 'normal').trim() || 'normal';
    return script.cleanUpMessage({
        getMessage: rawText,
        isImpersonate: type === 'impersonate',
        isContinue: type === 'continue',
        displayIncompleteSentences: true,
    });
}

/**
 * @param {any} value
 * @returns {string} `replace` or `append`
 */
export function normalizeCommitMode(value) {
    const mode = String(value || 'replace').trim();
    if (mode !== 'replace' && mode !== 'append') {
        throw new Error('agent.chat_commit_mode_invalid: mode must be replace or append');
    }
    return mode;
}

/**
 * @param {any} generationType
 * @param {any} mode
 * @returns {string}
 */
export function initialCommitSaveType(generationType, mode) {
    const type = String(generationType || 'normal').trim() || 'normal';
    if (mode === 'append' || type === 'append' || type === 'continue' || type === 'appendFinal') {
        return 'normal';
    }
    return type;
}

/**
 * @param {any} path
 * @returns {boolean}
 */
export function isAutoCommitTextPath(path) {
    const name = String(path || '').split('/').at(-1) || '';
    const dot = name.lastIndexOf('.');
    return dot > 0 && AUTO_COMMIT_TEXT_EXTENSIONS.has(name.slice(dot + 1).toLowerCase());
}

/**
 * @param {any} chat
 * @returns {number}
 */
export function getActiveMessageId(chat) {
    if (!Array.isArray(chat) || chat.length === 0) {
        throw new Error('agent.chat_commit_message_missing: saveReply did not create a chat message');
    }
    return chat.length - 1;
}

/**
 * @param {any} state the run's own bookkeeping object
 * @param {any[]} chat
 * @param {number} messageId
 * @param {number} lengthBefore
 * @returns {void}
 */
export function captureMessageTarget(state, chat, messageId, lengthBefore) {
    const message = chat[messageId];
    const swipeId = readMessageSwipeId(message);
    if (!message || typeof message !== 'object' || swipeId == null) {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }

    state.messageId = messageId;
    state.messageRef = message;
    state.swipeId = swipeId;
    // saveReply for type='swipe' / 'regenerate' against an existing
    // assistant message appends a new swipe in-place instead of pushing a new
    // chat entry. Rollback needs to distinguish those two cases.
    state.createdMessage = chat.length > lengthBefore;
    if (!state.createdMessage) {
        restoreAgentExtra(message, null);
    }
    mergeAgentExtra(message, { runId: state.runId });
}

/**
 * @param {any[]} chat
 * @param {any} state
 * @returns {void}
 */
export function assertActiveAgentMessage(chat, state) {
    const messageId = Number(state.messageId);
    if (!Array.isArray(chat) || chat.length - 1 !== messageId) {
        throw new Error('agent.chat_commit_message_mismatch: this run can only update its active chat message');
    }
    const message = chat[messageId];
    if (!message
        || message !== state.messageRef
        || readMessageSwipeId(message) !== state.swipeId) {
        throw new Error('agent.chat_commit_message_mismatch: active chat message changed during this run');
    }
}

/**
 * @param {any} script
 * @param {number} messageId
 * @param {string} [generationType]
 * @returns {Promise<void>}
 */
export async function finalizeGeneratedMessage(script, messageId, generationType) {
    if (typeof script.eventSource?.emit !== 'function' || !script.event_types) {
        throw new Error('agent.message_events_unavailable: SillyTavern message events are unavailable');
    }
    const type = String(generationType || 'normal').trim() || 'normal';
    await script.eventSource.emit(script.event_types.MESSAGE_RECEIVED, messageId, type);
    await script.finalizeMessageContent(messageId, script.event_types.CHARACTER_MESSAGE_RENDERED, type);
}

/**
 * @param {any[]} chat
 * @param {number} messageId
 * @param {any} payload
 * @param {any} file
 * @param {number} commitSeq
 * @param {any} [runState]
 * @returns {void}
 */
export function mergeAgentCommitExtraIntoMessage(chat, messageId, payload, file, commitSeq, runState = {}) {
    if (!Array.isArray(chat) || chat.length <= messageId) {
        throw new Error('agent.chat_commit_message_missing: active chat message is missing');
    }

    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }

    const previousAgent = message.extra?.tauritavern?.agent;
    const previousCommits = Array.isArray(previousAgent?.commits) ? previousAgent.commits : [];
    const chars = requireNonNegativeInteger(file?.chars, 'chars');
    const words = requireNonNegativeInteger(file?.words, 'words');
    const commit = {
        seq: commitSeq,
        commitId: payload.commitId,
        path: file.path,
        mode: normalizeCommitMode(payload.mode),
        reason: typeof payload.reason === 'string' ? payload.reason : undefined,
        chars,
        words,
        sha256: file.sha256,
    };
    const createdMessage = runState.createdMessage !== false;
    const swipeId = Number(runState.swipeId);
    const rollback = createdMessage || !Number.isInteger(swipeId) || swipeId < 0
        ? { strategy: 'deleteMessage' }
        : { strategy: 'deleteSwipe', swipeId };
    mergeAgentExtra(message, {
        version: 2,
        runId: payload.runId,
        workspaceId: payload.workspaceId,
        stableChatId: payload.stableChatId,
        profileId: payload.profileId ?? null,
        persistBaseStateId: payload.persistBaseStateId ?? null,
        persistStateStatus: 'not_committed',
        commitId: payload.commitId,
        commitSeq,
        commits: [...previousCommits, commit],
        rollback,
        artifacts: [{
            path: file.path,
            target: 'message_body',
            chars,
            words,
            sha256: file.sha256,
        }],
    });
}

/**
 * @param {any[]} chat
 * @param {number} messageId
 * @param {any} payload
 * @param {string} stateId
 * @returns {void}
 */
export function mergePersistentStateExtraIntoMessage(chat, messageId, payload, stateId) {
    if (!Array.isArray(chat) || chat.length <= messageId) {
        throw new Error('agent.persistent_state_message_missing: target chat message is missing');
    }

    const message = chat[messageId];
    if (!message || typeof message !== 'object') {
        throw new Error('agent.persistent_state_message_invalid: target chat message is invalid');
    }
    if (message.extra?.tauritavern?.agent?.runId !== payload.runId) {
        throw new Error('agent.persistent_state_message_mismatch: target message belongs to another run');
    }

    mergeAgentExtra(message, {
        persistStateId: stateId,
        persistBaseStateId: payload.baseStateId ?? null,
        persistStateStatus: 'committed',
        persistChangeCount: Number(payload.changeCount ?? 0),
        // The host only sends this when the floor was expected to write state and
        // did not; recording the positive case keeps the key shaped the same on
        // every agent floor, so a reader never has to guess what absence means.
        stateUpdated: payload.stateUpdated !== false,
    });
}

/**
 * Bind a state version a person edited to the floor it belongs to.
 *
 * A run's own commit matches the floor it wrote; an edit has no run to match, so
 * the floor is found instead: the newest message that already carries state, or
 * the newest message when the chat has no state chain yet — a first edit starts
 * one where the next run will look for it.
 *
 * @param {any[]} chat
 * @param {{ stateId: string; baseStateId?: string | null; changeCount?: number }} binding
 * @returns {number} the message index the version was bound to
 */
export function bindEditedStateToFloor(chat, binding) {
    const stateId = String(binding?.stateId ?? '');
    if (!stateId) {
        throw new Error('agent.persistent_state_id_missing: an edited state version needs an id');
    }
    if (!Array.isArray(chat) || chat.length === 0) {
        throw new Error('agent.persistent_state_chat_empty: a state edit needs a floor to belong to');
    }

    let target = chat.length - 1;
    for (let index = chat.length - 1; index >= 0; index -= 1) {
        if (typeof chat[index]?.extra?.tauritavern?.agent?.persistStateId === 'string') {
            target = index;
            break;
        }
    }

    mergeAgentExtra(chat[target], {
        persistStateId: stateId,
        persistBaseStateId: binding?.baseStateId ?? null,
        persistStateStatus: 'committed',
        persistChangeCount: Number(binding?.changeCount ?? 0),
        // The edit is what the panel now shows, so the floor is not stale.
        stateUpdated: true,
    });
    return target;
}

/**
 * @param {any} message
 * @returns {any}
 */
export function snapshotAgentExtra(message) {
    return structuredClone(message?.extra?.tauritavern?.agent ?? null);
}

/**
 * `snapshot === null` removes the agent block; anything else puts it back.
 *
 * @param {any} message
 * @param {any} snapshot
 * @returns {void}
 */
export function restoreAgentExtra(message, snapshot) {
    if (!message || typeof message !== 'object') {
        throw new Error('agent.chat_commit_message_invalid: active chat message is invalid');
    }
    if (snapshot === null) {
        const tauritavern = message.extra?.tauritavern;
        if (tauritavern && typeof tauritavern === 'object') {
            delete tauritavern.agent;
            if (Object.keys(tauritavern).length === 0) delete message.extra.tauritavern;
        }
    } else {
        message.extra ??= {};
        message.extra.tauritavern = {
            ...(message.extra.tauritavern || {}),
            agent: structuredClone(snapshot),
        };
    }
    syncActiveSwipeExtra(message);
}

/**
 * @param {any} script
 * @param {any} commitReason
 * @returns {Promise<void>}
 */
export async function persistActiveChat(script, commitReason) {
    const groupChats = await import('../../../scripts/group-chats.js');
    // Held in a local: the selection is a mutable module field, so the guard
    // above would not survive to the call.
    const groupId = groupChats.selected_group;
    if (groupId) {
        if (typeof groupChats.saveGroupChat !== 'function') {
            throw new Error('saveGroupChat is not available');
        }
        await groupChats.saveGroupChat(groupId, true, false, commitReason);
        return;
    }

    if (typeof script.saveChat !== 'function') {
        throw new Error('saveChat is not available');
    }
    await script.saveChat({ commitReason });
}

/**
 * @param {any} message
 * @returns {number | null}
 */
function readMessageSwipeId(message) {
    const swipeId = Number(message?.swipe_id);
    return Number.isInteger(swipeId) && swipeId >= 0 ? swipeId : null;
}

/**
 * Keep the active swipe's own `extra` in step with the message's.
 *
 * @param {any} message
 * @returns {void}
 */
function syncActiveSwipeExtra(message) {
    const swipeId = Number(message.swipe_id);
    if (Array.isArray(message.swipe_info) && Number.isInteger(swipeId) && message.swipe_info[swipeId]) {
        message.swipe_info[swipeId].extra = structuredClone(message.extra);
    }
}

/**
 * @param {any} message
 * @param {any} patch
 * @returns {void}
 */
function mergeAgentExtra(message, patch) {
    message.extra ??= {};
    message.extra.tauritavern = {
        ...message.extra.tauritavern,
        agent: {
            ...message.extra.tauritavern?.agent,
            ...patch,
        },
    };
    syncActiveSwipeExtra(message);
}

/**
 * @param {any} value
 * @param {string} key
 * @returns {number}
 */
function requireNonNegativeInteger(value, key) {
    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`agent.host_workspace_file_invalid: ${key} must be a non-negative integer`);
    }
    return number;
}
