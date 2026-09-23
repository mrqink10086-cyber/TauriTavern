/**
 * The scenes an embedded-asset panel reads and writes.
 *
 * A scene lives in a card's extension or a preset's extension field, which is
 * the only thing the two carriers disagree about — so the carrier is injected
 * and everything a scene actually does stays here: read the payload, keep one
 * scene per name, and let the backend store be the source of what gets embedded.
 */

import type { EmbeddedStateItem } from './EmbeddedAssetsContract';
import type { NamedAssetCarrier } from './embedded-named-assets';
import {
    embeddedStateSummary,
    portableEmbeddedState,
    readEmbeddedStatePackage,
    type EmbeddedStatePackage,
    type StoredEmbeddedStateItem,
} from './embedded-asset-packages';
import { translateAgentSystem as tr } from './i18n';
import { getStateDeclaration } from './state-config-api';

/** Where one target's scenes live; the same pair every carriable document uses. */
export type SceneCarrier = NamedAssetCarrier<EmbeddedStatePackage>;

/** The scenes this carrier holds, as the panel lists them. */
export function readEmbeddedScenes(carrier: SceneCarrier): EmbeddedStateItem[] {
    return readEmbeddedStatePackage(carrier.read()).items.map(embeddedStateSummary);
}

/**
 * One scene per name, like the backend's own store: carrying the same name twice
 * would make the card's meaning depend on import order.
 */
export function upsertScene(packageValue: EmbeddedStatePackage, item: StoredEmbeddedStateItem): EmbeddedStatePackage {
    const name = item.scene.name;
    const index = packageValue.items.findIndex((entry) => entry?.scene?.name === name);
    if (index >= 0) {
        packageValue.items[index] = item;
    } else {
        packageValue.items.push(item);
    }
    return packageValue;
}

function removeScene(packageValue: EmbeddedStatePackage, name: string): EmbeddedStatePackage {
    packageValue.items = packageValue.items.filter((entry) => entry?.scene?.name !== name);
    return packageValue;
}

function requireStateName(stateName: string): string {
    const name = String(stateName ?? '').trim();
    if (!name) {
        throw new Error(tr('sceneNameRequired'));
    }
    return name;
}

/**
 * Carry a saved scene in this target.
 *
 * The backend store is the source: the target gets the same document the editor
 * would export, so a scene moves between a card and a `.scene.json` file with no
 * second notion of what a scene is.
 */
export async function embedScene(carrier: SceneCarrier, stateName: string): Promise<string> {
    const name = requireStateName(stateName);
    const declaration = await getStateDeclaration(name);
    await carrier.write(upsertScene(
        readEmbeddedStatePackage(carrier.read()),
        portableEmbeddedState(name, declaration),
    ));
    return name;
}

export async function removeEmbeddedScene(carrier: SceneCarrier, stateName: string): Promise<void> {
    const name = requireStateName(stateName);
    await carrier.write(removeScene(readEmbeddedStatePackage(carrier.read()), name));
}
