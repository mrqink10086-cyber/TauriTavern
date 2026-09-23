/** Questions the panel asks about the editor's state. */

import { prettyJson } from './host-api';
import { normalizeStateDeclarationForSave } from './state-declaration-normalize';
import type { StateConfigSnapshot } from './state-config-contract';

export function stateConfigBusy(snapshot: StateConfigSnapshot): boolean {
    return snapshot.loading || snapshot.saving;
}

/**
 * Whether the draft differs from what is stored.
 *
 * The comparison goes through the same normalization the save uses, so a blank
 * row the user has not filled in does not count as a change.
 */
export function stateConfigDraftIsDirty(snapshot: StateConfigSnapshot): boolean {
    if (!snapshot.draft) {
        return false;
    }
    return prettyJson(normalizeStateDeclarationForSave(snapshot.draft)) !== snapshot.savedJson;
}

export function sortedNames(names: readonly string[]): string[] {
    return [...names].map((name) => String(name)).sort((left, right) => left.localeCompare(right));
}
