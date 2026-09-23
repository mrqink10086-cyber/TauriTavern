// @ts-check

/**
 * First-party recall's host ABI.
 *
 * Floor binding is the one call the frontend owes recall. The host indexes a
 * published state version before it announces the version id — otherwise a
 * binding would look for facts that are not there yet — but only the chat knows
 * which floor the version landed on.
 *
 * @typedef {{ safeInvoke: (command: string, args?: object) => Promise<any> }} Transport
 * @typedef {{ stateId: string; floor: number }} RecallFloorBinding
 */

/**
 * @param {Transport} transport
 */
export function createRecallApi({ safeInvoke }) {
    return {
        /**
         * Record which floor each published state version landed on.
         *
         * One call covers the message that was just saved and a whole-chat
         * backfill: they are the same operation, and re-binding a version to the
         * floor it already has changes nothing.
         *
         * @param {{ stableChatId: string; bindings: RecallFloorBinding[] }} input
         * @returns {Promise<number>} how many records actually moved
         */
        async bindStateFloors({ stableChatId, bindings }) {
            const changed = await safeInvoke('recall_bind_state_floors', {
                dto: { stableChatId, bindings },
            });
            return Number(changed) || 0;
        },
    };
}

/** @param {Transport} context */
export function installRecallApi(context) {
    const host = window.__TAURITAVERN__;
    if (!host) throw new Error('TauriTavern host ABI is not installed');
    host.api ??= {};
    host.api.recall = createRecallApi(context);
}
