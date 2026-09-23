/**
 * Which document a new declaration starts from.
 *
 * Kept out of the controller because it is a lookup table, not an edit: the
 * shipped examples and the sentence that introduces each one live here, so
 * adding a third one is one row rather than another branch in the middle of the
 * create path.
 */

import type { AgentSystemMessageKey } from './i18n';
import type { StateExampleKind } from './state-config-contract';
import type { StateDeclaration } from './state-config-model';
import { exampleDeclaration } from './state-examples';
import { seraphinaDeclaration } from './state-examples-seraphina';

export function draftForExample(example: StateExampleKind): {
    draft: StateDeclaration;
    noticeKey: AgentSystemMessageKey;
} {
    return example === 'seraphina'
        ? { draft: seraphinaDeclaration(), noticeKey: 'stateDeclarationSeraphinaLoaded' }
        : { draft: exampleDeclaration(), noticeKey: 'stateDeclarationExampleLoaded' };
}
