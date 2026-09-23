/**
 * The state predicate endpoints, as the editor uses them.
 *
 * The routes are the app's own HTTP surface (`/api/state-predicates/*`), the
 * same one every other settings page uses; the host turns each of them into the
 * matching Tauri command. Nothing here decides anything: the backend validates
 * and refuses, and its message is what the editor shows.
 */

import type {
    PredicateEvaluationDto,
    StatePredicateSet,
} from './state-predicate-model';

const STATE_PREDICATE_ROUTES = Object.freeze({
    list: '/api/state-predicates/list',
    get: '/api/state-predicates/get',
    save: '/api/state-predicates/save',
    delete: '/api/state-predicates/delete',
    evaluate: '/api/state-predicates/evaluate',
});

async function postPredicateRoute<T>(url: string, body: unknown): Promise<T> {
    const response = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body ?? {}),
    });
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(details || response.statusText || `HTTP ${response.status}`);
    }
    return await response.json() as T;
}

export async function listStatePredicateSets(): Promise<string[]> {
    const names = await postPredicateRoute<unknown>(STATE_PREDICATE_ROUTES.list, {});
    return Array.isArray(names) ? names.map((name) => String(name)) : [];
}

export async function getStatePredicateSet(name: string): Promise<StatePredicateSet> {
    const set = await postPredicateRoute<StatePredicateSet>(STATE_PREDICATE_ROUTES.get, { name });
    if (!set || typeof set !== 'object' || Array.isArray(set)) {
        throw new Error(`state predicate set \`${name}\` is not a predicate document`);
    }
    return set;
}

export async function saveStatePredicateSet(name: string, set: StatePredicateSet): Promise<void> {
    await postPredicateRoute(STATE_PREDICATE_ROUTES.save, { name, set });
}

export async function deleteStatePredicateSet(name: string): Promise<void> {
    await postPredicateRoute(STATE_PREDICATE_ROUTES.delete, { name });
}

/**
 * Run one set once against assumed field values.
 *
 * The preview has no chat to read, so it sends the values it wants judged. An
 * empty field list is still meaningful: every entry that reads state is simply
 * unavailable, and the answer says so.
 */
export async function evaluateStatePredicateSet(input: {
    set: StatePredicateSet;
    fields?: Record<string, string[]>;
}): Promise<PredicateEvaluationDto> {
    const evaluation = await postPredicateRoute<PredicateEvaluationDto>(
        STATE_PREDICATE_ROUTES.evaluate,
        input,
    );
    if (!evaluation || typeof evaluation !== 'object' || !Array.isArray(evaluation.selected)) {
        throw new Error('state predicate evaluation returned no result');
    }
    return evaluation;
}
