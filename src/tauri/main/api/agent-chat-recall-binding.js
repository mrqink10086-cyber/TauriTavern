// @ts-check

import { createRecallApi } from './recall.js';

/**
 * Tell recall which floor a published state version landed on.
 *
 * The host indexes a version before it announces the version id, so the records
 * already exist; only the chat knows the floor they belong to. Binding is
 * replayable with the same call, which is why a failure here only warns — the
 * commit the user is waiting on has already succeeded by this point.
 *
 * @param {{
 *   safeInvoke: (command: string, args?: object) => Promise<any>;
 *   payload: any;
 *   stateId: string;
 *   floor: number;
 * }} input
 */
export async function bindRecallFloor({ safeInvoke, payload, stateId, floor }) {
    const stableChatId = String(payload?.stableChatId || '').trim();
    if (!stableChatId) return;
    try {
        await createRecallApi({ safeInvoke }).bindStateFloors({
            stableChatId,
            bindings: [{ stateId, floor }],
        });
    } catch (error) {
        console.warn('Recall: failed to record the floor of a published state version', error);
    }
}
