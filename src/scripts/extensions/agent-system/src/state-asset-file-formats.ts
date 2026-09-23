/**
 * The on-disk shape of each asset the state editors can export.
 *
 * A machine and a predicate set had no file format of their own — they travelled
 * as character-card extensions before they could travel as files — so these are
 * what make a file say which of them it holds. A scene keeps its own older
 * wrapper, in `state-package.ts`.
 */

import { createDraftFileFormat } from './state-asset-file';
import {
    machineDraftFromSpec,
    normalizeMachineForSave,
    type MachineDraft,
    type MachineSpec,
} from './state-machine-model';
import {
    normalizePredicateSetForSave,
    predicateDraftFromSet,
    type PredicateSetDraft,
    type StatePredicateSet,
} from './state-predicate-model';

export const MACHINE_FILE_FORMAT = createDraftFileFormat<MachineDraft, MachineSpec>({
    kind: 'tauritavern.state-machine',
    documentKey: 'machine',
    fileSuffix: '.machine.json',
    fallbackFileName: 'machine.json',
    isDocument: (value) => Array.isArray(value.states) && Array.isArray(value.transitions),
    toDraft: machineDraftFromSpec,
    fromDraft: normalizeMachineForSave,
});

export const PREDICATE_FILE_FORMAT = createDraftFileFormat<PredicateSetDraft, StatePredicateSet>({
    kind: 'tauritavern.state-predicates',
    documentKey: 'set',
    fileSuffix: '.predicates.json',
    fallbackFileName: 'predicates.json',
    isDocument: (value) => Array.isArray(value.groups) || Array.isArray(value.constants),
    toDraft: predicateDraftFromSet,
    fromDraft: normalizePredicateSetForSave,
});
