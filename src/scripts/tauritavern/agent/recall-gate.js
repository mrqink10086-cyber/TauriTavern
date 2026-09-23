/**
 * Recall blocks have to exist before the run input is frozen.
 *
 * The interceptor pass is awaited, so an extension that awaits its own retrieval
 * is finished by the time the freeze happens. This covers the rest: a hook that
 * starts its query in the background resolves before its block exists, and the
 * freeze then captures an empty block or the previous turn's. Nothing
 * downstream can tell the difference between that and a turn with nothing to
 * recall, which is why it is checked rather than assumed.
 *
 * The wait is bounded: a turn is not held hostage by an extension. What is not
 * allowed is freezing without saying why.
 */

/**
 * Recall blocks an extension is expected to have written by freeze time.
 *
 * The prefix covers every key that one recall extension writes — its own tag,
 * one per position group, one per channel — which is what has to be present for
 * the turn to carry what it recalled. Mirrors the default source list a Profile's
 * recall policy carries.
 */
export const DEFAULT_RECALL_SOURCES = ['3_vectfox*'];

export const RECALL_GATE_TIMEOUT_MS = 3000;
const RECALL_GATE_INTERVAL_MS = 50;

/** A source ending in `*` is a prefix; anything else is the whole key. */
export function matchesRecallSource(key, source) {
    const name = String(key ?? '').trim();
    const pattern = String(source ?? '').trim();
    if (!name || !pattern) {
        return false;
    }
    return pattern.endsWith('*') ? name.startsWith(pattern.slice(0, -1)) : name === pattern;
}

/** Which sources have a non-empty block, and which do not. */
export function recallBlocksPresent(prompts, sources) {
    const entries = Object.entries(prompts ?? {});
    const present = [];
    const missing = [];

    for (const source of sources ?? []) {
        const hit = entries.find(([key, prompt]) => matchesRecallSource(key, source)
            && String(prompt?.value ?? '').trim() !== '');
        if (hit) {
            present.push(hit[0]);
        } else {
            missing.push(source);
        }
    }

    return { present, missing };
}

/**
 * Wait for the configured sources to appear, up to a ceiling.
 *
 * @param {string[]} sources
 * @param {{ read?: () => object; timeoutMs?: number; intervalMs?: number; sleep?: (ms: number) => Promise<void> }} [options]
 * @returns {Promise<{ sources: string[]; present: string[]; missing: string[]; waitedMs: number; ready: boolean }>}
 */
export async function waitForRecallBlocks(sources, options = {}) {
    const list = (sources ?? []).map((source) => String(source ?? '').trim()).filter((source) => source !== '');
    if (list.length === 0) {
        return { sources: [], present: [], missing: [], waitedMs: 0, ready: true };
    }

    const read = typeof options.read === 'function' ? options.read : () => ({});
    const timeoutMs = Number.isFinite(options.timeoutMs) ? options.timeoutMs : RECALL_GATE_TIMEOUT_MS;
    const intervalMs = Number.isFinite(options.intervalMs) ? options.intervalMs : RECALL_GATE_INTERVAL_MS;
    const sleep = typeof options.sleep === 'function'
        ? options.sleep
        : (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const startedAt = Date.now();

    let state = recallBlocksPresent(read(), list);
    while (state.missing.length > 0 && Date.now() - startedAt < timeoutMs) {
        await sleep(intervalMs);
        state = recallBlocksPresent(read(), list);
    }

    return {
        sources: list,
        present: state.present,
        missing: state.missing,
        waitedMs: Date.now() - startedAt,
        ready: state.missing.length === 0,
    };
}
