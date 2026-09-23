/**
 * Copy for the World Info exceptions a Profile's context policy carries.
 *
 * Split out of the default messages the way the recall copy is, so a reader
 * looking for World Info wording has one place to look. The merge happens in
 * `i18n.ts`.
 */
export const WORLD_INFO_MESSAGES = {
    "worldInfoAccess": "World Info injection",
    "worldInfoHint": "Which of the entries this chat's books activated a delegated invocation reads. The scan happens once, before the run starts, and answers a question about the chat — what is decided here is only who is given it, word for word. This is where a fixed style sheet or house rule belongs: a Skill has to be opened on purpose, and anything summarised on the way in is no longer the text that was written.",
    "worldInfoSubagentInherits": "Sub-agents read the activated entries",
    "worldInfoSubagentHint": "Off: a delegated invocation starts from its own system prompt and the task, and reads none of them — what it gets today. On: it carries them all, except the ones ruled out below. A rule can also do the reverse while this is off.",
    "worldInfoEntriesHint": "Entries this chat's scan activated. Untouched ones follow the switch above; a ticked one is an exception, in either direction.",
    "worldInfoNoActivation": "This chat has not scanned its World Info yet. Run one turn first and the entries come up here.",
    "worldInfoActivationUnavailable": "The last activation could not be read. The rules below still apply; they just cannot be listed here.",
    "worldInfoConstant": "constant",
    "worldInfoExceptional": "exception",
    "worldInfoClearRules": "Clear the exceptions"
} as const;
