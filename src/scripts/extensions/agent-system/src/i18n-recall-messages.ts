/**
 * Copy for recall: what an Agent does with the blocks its extensions wrote.
 *
 * Split out of the default messages the way the state feature's are, so a reader
 * looking for recall wording has one place to look. The merge happens in
 * `i18n.ts`.
 */
export const RECALL_MESSAGES = {
    "recall": "Recall",
    "recallHint": "The blocks your recall extension wrote for this turn. They are written before the run starts and frozen with it, so every Agent of the run reads the same text — and nothing here asks the extension to recall again. What is decided here is only who reads what was already recalled.",
    "recallInject": "This Agent carries the recall blocks",
    "recallInjectHint": "On: the blocks stay in this Agent's prompt, where the assembly put them. Off: they are dropped from it before the first model call — the retrieval still happened, this Agent just works without it.",
    "recallSources": "Recall prompt keys",
    "recallSourcesPlaceholder": "3_vectfox*",
    "recallSourcesHint": "Which extension prompts count as recall. A key, or a prefix when it ends in * — one recall extension writes under several keys, and a prefix names all of them. Empty means this Agent claims no recall block: nothing is dropped and nothing is inherited.",
    "recallSubagentInherits": "Sub-agents inherit these blocks",
    "recallSubagentHint": "Off: a delegated invocation starts from its own system prompt and the task, with no recall of the parent's — it can still reach the story through the tools it was given. On: the parent's blocks are added to it as one system message."
} as const;
