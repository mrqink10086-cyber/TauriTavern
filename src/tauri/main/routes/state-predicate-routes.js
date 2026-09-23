// @ts-check

/**
 * The state predicate set routes.
 *
 * Split out of `resource-routes.js` because that file is at its size ceiling and
 * this is a self-contained family: one store, one read, one check. It follows
 * the same conventions as the machine routes — a save only answers once the
 * bytes are down, a read's failure text is passed through untouched, and a
 * lookup of an unknown name answers `null` rather than a 404 the panel would
 * have to special-case.
 */

/**
 * @param {any} router
 * @param {any} context
 * @param {{ jsonResponse: (payload: any) => Response }} responses
 */
export function registerStatePredicateRoutes(router, context, { jsonResponse }) {
    router.post('/api/state-predicates/save', async ({ body }) => {
        await context.safeInvoke('save_state_predicate_set', { dto: body || {} });
        return jsonResponse({ ok: true });
    });

    router.post('/api/state-predicates/get', async ({ body }) => {
        const set = await context.safeInvoke('get_state_predicate_set', { name: body?.name || '' });
        return jsonResponse(set);
    });

    router.post('/api/state-predicates/list', async () => {
        const names = await context.safeInvoke('list_state_predicate_sets');
        return jsonResponse(Array.isArray(names) ? names : []);
    });

    router.post('/api/state-predicates/delete', async ({ body }) => {
        await context.safeInvoke('delete_state_predicate_set', { name: body?.name || '' });
        return jsonResponse({ ok: true });
    });

    router.post('/api/state-predicates/evaluate', async ({ body }) => {
        const result = await context.safeInvoke('evaluate_state_predicate_set', { dto: body || {} });
        return jsonResponse(result);
    });

    router.post('/api/state-predicates/validate', async ({ body }) => {
        const result = await context.safeInvoke('validate_state_predicate_set', { dto: body || {} });
        return jsonResponse(result);
    });

    router.post('/api/state-predicates/entries', async ({ body }) => {
        const result = await context.safeInvoke('get_state_predicate_entries', { dto: body || {} });
        return jsonResponse(result);
    });
}
