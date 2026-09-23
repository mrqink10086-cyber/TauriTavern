/**
 * Importing a picture the user picked into a host the state panel can serve.
 *
 * A candidate's source is a host path, not a file the panel keeps, so the
 * import goes through the same upload the background gallery uses and ends at
 * `/backgrounds/<name>` — the one root the panel already reads images from.
 */

import { requireSillyTavernContext } from './host-api';
import { isHostImageSource } from './state-config-model';
import type { Tr } from './AgentSystemPanelContract';

const BACKGROUND_UPLOAD_URL = '/api/backgrounds/upload';

type UploadContext = {
    getRequestHeaders?: (options?: { omitContentType?: boolean }) => HeadersInit;
};

/** The source a background file answers to once the host has it. */
export function backgroundSourceFor(fileName: string): string {
    return `/backgrounds/${encodeURIComponent(fileName)}`;
}

/**
 * Upload a picked file and return the host source for it.
 *
 * The file itself is what crosses the boundary — the host names it, and the
 * returned name is the only trustworthy half of the path. Nothing is written
 * to the draft here; the caller applies the source it gets back.
 */
export async function importStateImage(file: File, tr: Tr): Promise<string> {
    const context = requireSillyTavernContext() as Partial<UploadContext>;
    if (typeof context.getRequestHeaders !== 'function') {
        throw new Error(tr('stateDeclarationImageImportUnavailable'));
    }

    const body = new FormData();
    body.append('avatar', file, file.name);
    const response = await fetch(BACKGROUND_UPLOAD_URL, {
        method: 'POST',
        headers: context.getRequestHeaders({ omitContentType: true }),
        body,
        cache: 'no-cache',
    });
    if (!response.ok) {
        throw new Error(`${response.status} ${response.statusText}`);
    }

    const fileName = String(await response.text()).trim();
    const source = backgroundSourceFor(fileName);
    // The host keeps characters a URL attribute may not carry, and the domain
    // refuses those sources rather than escaping them, so say so now instead of
    // letting the save blocker name it later.
    if (!fileName || !isHostImageSource(source)) {
        throw new Error(tr('stateDeclarationImageNameUnsupported', { name: fileName }));
    }

    return source;
}
