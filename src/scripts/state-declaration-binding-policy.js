/**
 * Resolves which saved state declaration applies to the current context.
 *
 * Candidate order is a fixed priority, mirroring the theme binding flow:
 *   1. `chat`      - the declaration stored on the open chat header metadata
 *                    (`chat_metadata.state_declaration`);
 *   2. `character` - the declaration bound to the active character
 *                    (`power_user.state_declaration_bindings.characters[avatar]`);
 *   3. `group`     - the declaration bound to the active group
 *                    (`power_user.state_declaration_bindings.groups[groupId]`).
 *
 * The first candidate whose name exists in `availableNames` (the names
 * returned by `list_state_declarations`) wins and is returned. When no
 * candidate resolves — nothing bound, or every bound name was deleted — the
 * function returns `null` so the caller omits the `stateDeclaration` snapshot
 * key and the backend falls back to its default declaration.
 *
 * The candidate list is produced by `getStateDeclarationBindingCandidates()`
 * in `src/scripts/power-user.js`, which owns the context state (chat
 * metadata, power-user settings, selected group/character). Like
 * `resolveThemeBinding` in `./theme-binding-policy.js`, this policy is a pure
 * function: it only reads the arguments and never touches global state.
 *
 * @param {Array<{scope: 'chat'|'character'|'group', name: string|undefined}>} candidates
 * @param {string[]} availableNames
 * @returns {string|null}
 */
export function resolveStateDeclarationBinding(candidates, availableNames) {
    const available = new Set(availableNames);

    for (const candidate of candidates) {
        if (!candidate.name) {
            continue;
        }
        if (available.has(candidate.name)) {
            return candidate.name;
        }
    }

    return null;
}
