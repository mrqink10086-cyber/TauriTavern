import type {
    EmbeddedMachineItem,
    EmbeddedPredicateItem,
    EmbeddedProfileItem,
    EmbeddedSkillItem,
    EmbeddedStateItem,
} from './EmbeddedAssetsContract';
import { translateAgentSystem as tr } from './i18n';
import { readStatePackageValue, toStatePackage, type StatePackage } from './state-package';
import type { StateDeclaration } from './state-config-model';
import type { MachineSpec } from './state-machine-model';
import type { StatePredicateSet } from './state-predicate-model';
import {
    sanitizePortableAgentProfile,
    sanitizePortableAgentProfilePackage,
} from '../../../tauritavern/agent/agent-profile-portable.js';

const EMBEDDED_PROFILES_VERSION = 1;
const EMBEDDED_SKILLS_VERSION = 1;
const EMBEDDED_STATES_VERSION = 1;
const EMBEDDED_MACHINES_VERSION = 1;
const EMBEDDED_PREDICATES_VERSION = 1;
export const EMBEDDED_SKILL_ARCHIVE_FORMAT = 'ttskill-archive-base64-v1';

export type StoredEmbeddedProfile = Record<string, unknown> & { id: string; displayName: string };
export type StoredEmbeddedProfileItem = { profile: StoredEmbeddedProfile };
export type StoredEmbeddedSkillItem = {
    bundleFormat: string;
    skillName: string;
    sourceScope: TauriTavernSkillScope;
    sourceScopeLabel: string;
    fileName: string;
    contentBase64: string;
    sha256: string;
};
export type EmbeddedProfilePackage = { version: number; items: StoredEmbeddedProfileItem[] };
export type EmbeddedSkillPackage = { version: number; items: StoredEmbeddedSkillItem[] };
/** A scene travels as the file it exports to, so a card and a download agree. */
export type StoredEmbeddedStateItem = { scene: StatePackage };
export type EmbeddedStatePackage = { version: number; items: StoredEmbeddedStateItem[] };

/**
 * A machine or a predicate set as a card carries it.
 *
 * There is no file format for these two, and their names are already the keys
 * they are stored under, so the item is the name plus the document — a wrapper
 * around a wrapper would only be a second thing to keep in step.
 */
export type StoredEmbeddedMachineItem = { name: string; machine: MachineSpec };
export type EmbeddedMachinePackage = { version: number; items: StoredEmbeddedMachineItem[] };
export type StoredEmbeddedPredicateItem = { name: string; set: StatePredicateSet };
export type EmbeddedPredicatePackage = { version: number; items: StoredEmbeddedPredicateItem[] };

export function readEmbeddedProfilePackage(existing: unknown): EmbeddedProfilePackage {
    if (existing == null) return { version: EMBEDDED_PROFILES_VERSION, items: [] };
    const payload = sanitizePortableAgentProfilePackage(existing);
    return {
        version: payload.version,
        items: payload.items.map((item, index) => profileItem(item, `agentProfiles.items[${index}]`)),
    };
}

export function readEmbeddedSkillPackage(existing: unknown): EmbeddedSkillPackage {
    if (existing == null) return { version: EMBEDDED_SKILLS_VERSION, items: [] };
    const payload = plainObject(existing, 'skills');
    if (Number(payload.version) !== EMBEDDED_SKILLS_VERSION) {
        throw new Error(tr('embeddedSkillVersionUnsupported', { version: scalarText(payload.version) }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedSkillItemsInvalid'));
    }
    return {
        version: EMBEDDED_SKILLS_VERSION,
        items: payload.items.map((item, index) => skillItem(item, `skills.items[${index}]`)),
    };
}

/**
 * The scenes a card carries.
 *
 * Each item is a whole scene file, so the card needs no second format and no
 * second reader: what arrives here is exactly what the editor's own import
 * accepts, name included — the name is how the scene is stored afterwards.
 */
export function readEmbeddedStatePackage(existing: unknown): EmbeddedStatePackage {
    if (existing == null) return { version: EMBEDDED_STATES_VERSION, items: [] };
    const payload = plainObject(existing, 'stateDeclarations');
    if (Number(payload.version) !== EMBEDDED_STATES_VERSION) {
        throw new Error(tr('embeddedStateVersionUnsupported', { version: scalarText(payload.version) }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedStateItemsInvalid'));
    }
    return {
        version: EMBEDDED_STATES_VERSION,
        items: payload.items.map((item, index) => stateItem(item, `stateDeclarations.items[${index}]`)),
    };
}

export function readEmbeddedMachinePackage(existing: unknown): EmbeddedMachinePackage {
    if (existing == null) return { version: EMBEDDED_MACHINES_VERSION, items: [] };
    const payload = plainObject(existing, 'stateMachines');
    if (Number(payload.version) !== EMBEDDED_MACHINES_VERSION) {
        throw new Error(tr('embeddedMachineVersionUnsupported', { version: scalarText(payload.version) }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedMachineItemsInvalid'));
    }
    return {
        version: EMBEDDED_MACHINES_VERSION,
        items: payload.items.map((item, index) => machineItem(item, `stateMachines.items[${index}]`)),
    };
}

export function readEmbeddedPredicatePackage(existing: unknown): EmbeddedPredicatePackage {
    if (existing == null) return { version: EMBEDDED_PREDICATES_VERSION, items: [] };
    const payload = plainObject(existing, 'statePredicates');
    if (Number(payload.version) !== EMBEDDED_PREDICATES_VERSION) {
        throw new Error(tr('embeddedPredicateVersionUnsupported', { version: scalarText(payload.version) }));
    }
    if (!Array.isArray(payload.items)) {
        throw new Error(tr('embeddedPredicateItemsInvalid'));
    }
    return {
        version: EMBEDDED_PREDICATES_VERSION,
        items: payload.items.map((item, index) => predicateItem(item, `statePredicates.items[${index}]`)),
    };
}

export function portableEmbeddedMachine(name: string, machine: MachineSpec): StoredEmbeddedMachineItem {
    return machineItem({ name: nonEmptyString(name, 'machine.name'), machine }, 'machine');
}

export function portableEmbeddedPredicate(name: string, set: StatePredicateSet): StoredEmbeddedPredicateItem {
    return predicateItem({ name: nonEmptyString(name, 'set.name'), set }, 'set');
}

/**
 * What the panel says a carried machine holds.
 *
 * Read defensively for the same reason a carried scene is: the document came
 * from a foreign card, and a summary that throws would hide the asset.
 */
export function embeddedMachineSummary(item: StoredEmbeddedMachineItem): EmbeddedMachineItem {
    const states = Array.isArray(item.machine?.states) ? item.machine.states : [];
    const transitions = Array.isArray(item.machine?.transitions) ? item.machine.transitions : [];
    return {
        name: item.name,
        stateCount: states.length,
        transitionCount: transitions.length,
        hasHooks: Boolean(item.machine?.hooks),
    };
}

export function embeddedPredicateSummary(item: StoredEmbeddedPredicateItem): EmbeddedPredicateItem {
    const groups = Array.isArray(item.set?.groups) ? item.set.groups : [];
    const groupEntries = groups.reduce(
        (total, group) => total + (Array.isArray(group?.entries) ? group.entries.length : 0),
        0,
    );
    const constants = Array.isArray(item.set?.constants) ? item.set.constants.length : 0;
    return { name: item.name, groupCount: groups.length, entryCount: groupEntries + constants };
}

export function portableEmbeddedProfile(profile: unknown): StoredEmbeddedProfile {
    return profileItem({
        profile: sanitizePortableAgentProfile(plainObject(profile, 'profile')),
    }, 'profile').profile;
}

export function embeddedProfileSummary(item: StoredEmbeddedProfileItem): EmbeddedProfileItem {
    return {
        profile: {
            id: item.profile.id,
            ...(item.profile.displayName ? { displayName: item.profile.displayName } : {}),
        },
    };
}

export function embeddedSkillSummary(item: StoredEmbeddedSkillItem): EmbeddedSkillItem {
    return {
        skillName: item.skillName,
        sourceScopeLabel: item.sourceScopeLabel,
        fileName: item.fileName,
    };
}

/** A scene as a card stores it: the exported file, name and all. */
export function portableEmbeddedState(name: string, declaration: StateDeclaration): StoredEmbeddedStateItem {
    return stateItem({ scene: toStatePackage(name, declaration) }, 'scene');
}

/**
 * What the panel says a carried scene holds.
 *
 * The document arrives from a foreign file, so the counts are read defensively:
 * a summary that throws would hide the very asset the panel exists to list.
 */
export function embeddedStateSummary(item: StoredEmbeddedStateItem): EmbeddedStateItem {
    const { declaration } = item.scene;
    const panels: unknown = declaration.panels?.panels;
    return {
        name: item.scene.name,
        fieldCount: declaration.fields.length,
        panelCount: Array.isArray(panels) ? panels.length : 0,
        hasMachine: Boolean(declaration.machine),
        hasPredicates: Boolean(declaration.predicates),
    };
}

function stateItem(value: unknown, label: string): StoredEmbeddedStateItem {
    const item = plainObject(value, label);
    const read = readStatePackageValue(item.scene);
    if (!read.declaration) {
        throw new Error(tr('embeddedSceneUnreadable', { label, reason: read.failure ?? 'not_a_package' }));
    }
    if (!read.name) {
        throw new Error(tr('embeddedSceneNameRequired', { label }));
    }
    return { scene: toStatePackage(read.name, read.declaration) };
}

function machineItem(value: unknown, label: string): StoredEmbeddedMachineItem {
    const item = plainObject(value, label);
    const name = nonEmptyString(item.name, `${label}.name`);
    const machine = plainObject(item.machine, `${label}.machine`);
    if (!Array.isArray(machine.states) || !Array.isArray(machine.transitions)) {
        throw new Error(tr('embeddedMachineUnreadable', { label }));
    }
    return { name, machine: machine as MachineSpec };
}

function predicateItem(value: unknown, label: string): StoredEmbeddedPredicateItem {
    const item = plainObject(value, label);
    const name = nonEmptyString(item.name, `${label}.name`);
    const set = plainObject(item.set, `${label}.set`);
    if (!Array.isArray(set.groups) && !Array.isArray(set.constants)) {
        throw new Error(tr('embeddedPredicateUnreadable', { label }));
    }
    return { name, set };
}

function profileItem(value: unknown, label: string): StoredEmbeddedProfileItem {
    const item = plainObject(value, label);
    const profile = plainObject(item.profile, `${label}.profile`);
    const id = nonEmptyString(profile.id, `${label}.profile.id`);
    const displayName = profile.displayName == null
        ? ''
        : requireString(profile.displayName, `${label}.profile.displayName`);
    return { profile: { ...profile, id, displayName } };
}

function skillItem(value: unknown, label: string): StoredEmbeddedSkillItem {
    const item = plainObject(value, label);
    const bundleFormat = nonEmptyString(item.bundleFormat, `${label}.bundleFormat`);
    if (bundleFormat !== EMBEDDED_SKILL_ARCHIVE_FORMAT) {
        throw new Error(`Unsupported embedded Agent Skill bundle format: ${bundleFormat}`);
    }
    return {
        bundleFormat,
        skillName: nonEmptyString(item.skillName, `${label}.skillName`),
        sourceScope: skillScope(item.sourceScope, `${label}.sourceScope`),
        sourceScopeLabel: requireString(item.sourceScopeLabel, `${label}.sourceScopeLabel`),
        fileName: nonEmptyString(item.fileName, `${label}.fileName`),
        contentBase64: nonEmptyString(item.contentBase64, `${label}.contentBase64`),
        sha256: requireString(item.sha256, `${label}.sha256`).trim(),
    };
}

function skillScope(value: unknown, label: string): TauriTavernSkillScope {
    const scope = plainObject(value, label);
    const kind = nonEmptyString(scope.kind, `${label}.kind`);
    if (kind === 'global') return { kind };
    if (kind === 'preset') {
        return {
            kind,
            apiId: nonEmptyString(scope.apiId, `${label}.apiId`),
            name: nonEmptyString(scope.name, `${label}.name`),
        };
    }
    if (kind === 'profile') return { kind, profileId: nonEmptyString(scope.profileId, `${label}.profileId`) };
    if (kind === 'character') return { kind, characterId: nonEmptyString(scope.characterId, `${label}.characterId`) };
    throw new Error(`Unsupported Skill scope kind: ${kind}`);
}

function plainObject(value: unknown, label: string): Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return value as Record<string, unknown>;
}

function requireString(value: unknown, label: string): string {
    if (typeof value !== 'string') throw new Error(`${label} must be a string`);
    return value;
}

function nonEmptyString(value: unknown, label: string): string {
    const text = requireString(value, label).trim();
    if (!text) throw new Error(`${label} is required`);
    return text;
}

function scalarText(value: unknown): string {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
        ? String(value)
        : '';
}
