// Similharity compatibility bridge.
//
// The VectFox extension talks to a SillyTavern server plugin at
// `/api/plugins/similharity/*`. TauriTavern serves the same contract from the
// native side, so the extension runs unchanged against a local Qdrant instance.
// Only the endpoints below the prefix are forwarded, and the endpoint string is
// passed through verbatim: the native side owns the allowlist.

import { extractErrorText, resolveHostErrorResponse } from '../kernel/host-error-response.js';

const BRIDGE_PREFIX = '/api/plugins/similharity';

function normalizeEndpoint(wildcard) {
    return String(wildcard || '').replace(/^\/+/, '');
}

function bridgeErrorCause(status) {
    if (status === 401) return 'similharity_auth_failed';
    if (status === 404) return 'similharity_not_found';
    if (status === 429) return 'similharity_rate_limited';
    if (status >= 500) return 'similharity_unavailable';
    return undefined;
}

async function invokeBridge(context, jsonResponse, method, endpoint, body) {
    try {
        const response = await context.safeInvoke('similharity_handle', {
            method,
            endpoint,
            request: body && typeof body === 'object' && !Array.isArray(body) ? body : {},
        });
        const status = Number(response?.status);
        const safeStatus = Number.isInteger(status) && status >= 100 && status <= 599 ? status : 500;
        if (response?.kind === 'empty') {
            return new Response(null, { status: safeStatus });
        }
        if (response?.kind === 'json') {
            return jsonResponse(response.body ?? null, safeStatus);
        }
        return jsonResponse(
            { error: true, cause: 'similharity_unavailable', message: 'Invalid bridge response' },
            500,
        );
    } catch (error) {
        const resolved = resolveHostErrorResponse(extractErrorText(error));
        return jsonResponse(
            { error: true, cause: bridgeErrorCause(resolved.status), message: resolved.body },
            resolved.status,
        );
    }
}

export function registerSimilharityRoutes(router, context, { jsonResponse }) {
    router.all(`${BRIDGE_PREFIX}/*`, async ({ method, body, wildcard }) => {
        const endpoint = normalizeEndpoint(wildcard);
        if (!endpoint) {
            return jsonResponse({ error: 'Missing Similharity endpoint' }, 404);
        }
        return invokeBridge(context, jsonResponse, method, endpoint, body);
    });
}
