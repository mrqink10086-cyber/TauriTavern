import { translateAgentSystem as tr } from './i18n';
import { skillScopeKey, skillScopeLabel } from './skill-scope';

export type EmbeddedAssetTargetInput =
    | { kind: 'preset'; apiId?: string; name?: string }
    | { kind: 'character' };

export type EmbeddedAssetTargetSummary = {
    kind: 'preset' | 'character';
    apiId?: string;
    characterId?: string;
    name: string;
    subtitle?: string;
};

export type EmbeddedProfileItem = {
    profile: {
        id: string;
        displayName?: string;
    };
};

export type EmbeddedSkillItem = {
    skillName: string;
    sourceScopeLabel: string;
    fileName: string;
};

/**
 * A scene carried by a card, as the panel lists it.
 *
 * The counts are here rather than the document: the panel answers "what does
 * this card bring", and reading four numbers beats rendering a whole scene.
 */
export type EmbeddedStateItem = {
    name: string;
    fieldCount: number;
    panelCount: number;
    hasMachine: boolean;
    hasPredicates: boolean;
};

/** A standalone machine a card carries, as the panel lists it. */
export type EmbeddedMachineItem = {
    name: string;
    stateCount: number;
    transitionCount: number;
    hasHooks: boolean;
};

/** A standalone predicate set a card carries, as the panel lists it. */
export type EmbeddedPredicateItem = {
    name: string;
    groupCount: number;
    entryCount: number;
};

export type EmbeddedAssetsRead = {
    target: EmbeddedAssetTargetSummary;
    profiles: EmbeddedProfileItem[];
    skills: EmbeddedSkillItem[];
    states: EmbeddedStateItem[];
    machines: EmbeddedMachineItem[];
    predicates: EmbeddedPredicateItem[];
};

export type SkillOption = TauriTavernSkillIndexEntry & {
    key: string;
    scopeLabel: string;
};

export type EmbeddedAssetsInitial = {
    targetInfo: EmbeddedAssetTargetSummary;
    profiles: TauriTavernAgentProfileSummary[];
    skills: SkillOption[];
    /** Saved scenes, so one of them can be carried by this card. */
    states: string[];
    machines: string[];
    predicates: string[];
    embeddedProfiles: EmbeddedProfileItem[];
    embeddedSkills: EmbeddedSkillItem[];
    embeddedStates: EmbeddedStateItem[];
    embeddedMachines: EmbeddedMachineItem[];
    embeddedPredicates: EmbeddedPredicateItem[];
};

export type EmbeddedAssetsActions = {
    // Resolves the embedded profile id so the panel can toast it.
    embedProfile: (profileId: string) => Promise<string>;
    embedSkill: (skill: SkillOption) => Promise<void>;
    /** Carry a saved scene in this card, resolved by name. */
    embedState: (stateName: string) => Promise<string>;
    embedMachine: (machineName: string) => Promise<string>;
    embedPredicateSet: (setName: string) => Promise<string>;
    removeProfile: (profileId: string) => Promise<void>;
    removeSkill: (skillName: string) => Promise<void>;
    removeState: (stateName: string) => Promise<void>;
    removeMachine: (machineName: string) => Promise<void>;
    removePredicateSet: (setName: string) => Promise<void>;
    // Synchronous re-read of the persisted embedded facts after a mutation.
    readEmbedded: () => EmbeddedAssetsRead;
    toastSuccess: (message: string) => void;
    // console + toastr; returns the message for inline display.
    reportError: (error: unknown) => string;
};

function skillSelectionKey(skill: { scope?: TauriTavernSkillScope | null; name?: string | null }): string {
    const scopeKey = skillScopeKey(skill?.scope);
    const name = String(skill?.name || '').trim();
    if (!scopeKey || !name) {
        throw new Error(tr('skillScopeNotFound', { id: name || scopeKey || '' }));
    }
    return JSON.stringify({ scopeKey, name });
}

export function buildSkillOptions(skills: TauriTavernSkillIndexEntry[]): SkillOption[] {
    if (!Array.isArray(skills)) {
        throw new Error(tr('skillListMustBeArray'));
    }

    return skills
        .map((skill) => ({
            ...skill,
            key: skillSelectionKey(skill),
            scopeLabel: skillScopeLabel(skill.scope),
        }))
        .sort((left, right) => {
            const leftName = String(left.displayName || left.name || '');
            const rightName = String(right.displayName || right.name || '');
            return leftName.localeCompare(rightName, undefined, { sensitivity: 'base' })
                || left.scopeLabel.localeCompare(right.scopeLabel, undefined, { sensitivity: 'base' });
        });
}

export function profileDisplayName(item: EmbeddedProfileItem): string {
    return item.profile.displayName || item.profile.id;
}

export function skillOptionLabel(skill: SkillOption): string {
    return `${skill.displayName || skill.name} (${skill.scopeLabel})`;
}

export function embeddedSkillSubtitle(item: EmbeddedSkillItem): string {
    const sourceScopeLabel = String(item.sourceScopeLabel || '').trim();
    const fileName = String(item.fileName || '').trim();
    return sourceScopeLabel ? `${sourceScopeLabel} - ${fileName}` : fileName;
}

/**
 * What a carried scene holds, in one line.
 *
 * A scene's worth is not its name but how much of the state system it uses, so
 * the line counts what it brings — and names the two parts that can be carried
 * inside a declaration but are authored in their own tabs.
 */
export function embeddedStateSubtitle(item: EmbeddedStateItem): string {
    const parts = [
        tr('embeddedStateFields', { count: item.fieldCount }),
        tr('embeddedStatePanels', { count: item.panelCount }),
    ];
    if (item.hasMachine) parts.push(tr('embeddedStateMachine'));
    if (item.hasPredicates) parts.push(tr('embeddedStatePredicates'));
    return parts.join(' · ');
}

export function embeddedMachineSubtitle(item: EmbeddedMachineItem): string {
    const parts = [
        tr('embeddedMachineStates', { count: item.stateCount }),
        tr('embeddedMachineTransitions', { count: item.transitionCount }),
    ];
    if (item.hasHooks) parts.push(tr('embeddedMachineHooks'));
    return parts.join(' · ');
}

export function embeddedPredicateSubtitle(item: EmbeddedPredicateItem): string {
    return [
        tr('embeddedPredicateGroups', { count: item.groupCount }),
        tr('embeddedPredicateEntries', { count: item.entryCount }),
    ].join(' · ');
}
