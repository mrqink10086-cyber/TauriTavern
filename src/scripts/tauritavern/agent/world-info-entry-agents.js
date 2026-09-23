// @ts-check
/**
 * The Agent side of one World Info entry, read and written through the profile
 * channel.
 *
 * Which Agents read an entry is a decision about the entry, and the Agent list
 * is known without a run, so the entry editor can offer it directly. The rules
 * live on each Profile's context policy; this module only carries them there,
 * and the panel reads the same rows back through its own view of the draft.
 */

import {
    normalizeAgentContextPolicy,
    worldInfoEntryCarriedBy,
    worldInfoPolicyWithEntry,
} from './agent-context-policy.js';

/** The Profile every install ships with; its policy is not user-editable. */
const BUILTIN_PROFILE_ID = 'default-writer';

function profilesApi() {
    return window.__TAURITAVERN__?.api?.agent?.profiles ?? null;
}

/** Whether the entry editor can offer the control at all. */
export function agentEntryCarriersAvailable() {
    const api = profilesApi();
    return typeof api?.list === 'function'
        && typeof api?.load === 'function'
        && typeof api?.save === 'function';
}

/**
 * Every Agent, with whether it carries that entry today.
 *
 * @param {{ world: string, uid: number }} entry Named the way the scan names it.
 * @returns {Promise<Array<{profileId: string, displayName: string, builtin: boolean, subagentInherits: boolean, carried: boolean, exceptional: boolean}>>}
 */
export async function loadAgentEntryCarriers(entry) {
    const api = profilesApi();
    if (typeof api?.list !== 'function' || typeof api?.load !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }

    const result = await api.list();
    const summaries = Array.isArray(result?.profiles) ? result.profiles : [];

    return Promise.all(summaries.map(async (summary) => {
        const profileId = String(summary?.id ?? '').trim();
        const loaded = await api.load({ profileId });
        const profile = loaded?.profile;
        if (!profile) {
            throw new Error(`agent.profile_not_found: Agent profile not found: ${profileId}`);
        }

        const policy = normalizeAgentContextPolicy(profile.context);
        const book = String(entry?.world ?? '').trim();
        const uid = Number(entry?.uid);
        const rule = policy.worldInfo.entries
            .find((candidate) => candidate.book === book && candidate.uid === uid);

        return {
            profileId,
            displayName: String(summary?.displayName ?? profileId),
            builtin: profileId === BUILTIN_PROFILE_ID,
            subagentInherits: policy.worldInfo.subagentInherits,
            carried: worldInfoEntryCarriedBy(policy, entry),
            exceptional: rule !== undefined,
        };
    }));
}

/**
 * Pins one entry for one Agent, or drops the pin when it agrees with the switch.
 *
 * @param {{ world: string, uid: number }} entry Named the way the scan names it.
 * @param {string} profileId
 * @param {boolean} carried
 */
export async function setAgentEntryCarrier(entry, profileId, carried) {
    const api = profilesApi();
    if (typeof api?.load !== 'function' || typeof api?.save !== 'function') {
        throw new Error('agent.profile_api_unavailable: TauriTavern Agent profile API is unavailable');
    }

    const normalizedProfileId = String(profileId || '').trim();
    const loaded = await api.load({ profileId: normalizedProfileId });
    const profile = loaded?.profile;
    if (!profile) {
        throw new Error(`agent.profile_not_found: Agent profile not found: ${normalizedProfileId}`);
    }

    // Written whole: the policy is the Profile's context, and the save drops an
    // empty one, so there is nothing to clean up here.
    profile.context = worldInfoPolicyWithEntry(profile.context, entry, carried !== false);
    await api.save({ profile });
}
