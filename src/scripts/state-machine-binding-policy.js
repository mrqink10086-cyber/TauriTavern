/**
 * Resolves which saved state machine applies to the current context.
 *
 * Candidate order is a fixed priority, mirroring the state declaration binding:
 *   1. `chat`      - the machine stored on the open chat header metadata
 *                    (`chat_metadata.state_machine`);
 *   2. `character` - the machine bound to the active character
 *                    (`power_user.state_machine_bindings.characters[avatar]`);
 *   3. `group`     - the machine bound to the active group
 *                    (`power_user.state_machine_bindings.groups[groupId]`).
 *
 * The first candidate whose name exists in `availableNames` (the names returned
 * by `list_state_machines`) wins and is returned. When no candidate resolves —
 * nothing bound, or every bound name was deleted — the function returns `null`
 * so the caller omits the `stateMachine` snapshot key and the run behaves
 * exactly as it does on a chat with no machine at all.
 *
 * The candidate list is produced by `getStateMachineBindingCandidates()` in
 * `src/scripts/power-user.js`, which owns the context state (chat metadata,
 * power-user settings, selected group/character). Like
 * `resolveStateDeclarationBinding`, this policy is a pure function: it only
 * reads its arguments and never touches global state.
 *
 * @param {Array<{scope: 'chat'|'character'|'group', name: string|undefined}>} candidates
 * @param {string[]} availableNames
 * @returns {string|null}
 */
export function resolveStateMachineBinding(candidates, availableNames) {
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
