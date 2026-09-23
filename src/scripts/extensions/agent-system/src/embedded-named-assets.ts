/**
 * Standalone machines and predicate sets, as a target carries them.
 *
 * A scene gets its own module because its item is a whole exported file; these
 * two have no file and share one shape — a name plus the document the store
 * holds — so they share the "one item per name" rule here.
 *
 * The carrier is injected for the same reason it is for scenes: only
 * `embedded-assets.ts` knows how to reach a card's extension or a preset's field.
 */

import { translateAgentSystem as tr } from './i18n';

/** The item shape both kinds share: the name they are stored under, plus the document. */
export type NamedAssetItem = { name: string };

export type NamedAssetPackage<TItem extends NamedAssetItem> = { version: number; items: TItem[] };

/** What differs between the two kinds: the package envelope and the item builder. */
export type NamedAssetKind<TItem extends NamedAssetItem, TDocument> = {
    readPackage: (existing: unknown) => NamedAssetPackage<TItem>;
    itemOf: (name: string, document: TDocument) => TItem;
};

/**
 * Where one target keeps these documents, as a pair of calls.
 *
 * `write` is declared as a method rather than a property so a carrier typed to
 * one package stays assignable to the untyped one the helpers take.
 */
export type NamedAssetCarrier<TPackage = unknown> = {
    read: () => unknown;
    write(packageValue: TPackage): Promise<void>;
};

export function readEmbeddedNamedItems<TItem extends NamedAssetItem, TDocument>(
    carrier: NamedAssetCarrier,
    kind: NamedAssetKind<TItem, TDocument>,
): TItem[] {
    return kind.readPackage(carrier.read()).items;
}

/**
 * One item per name, like the backend's own store: carrying the same name twice
 * would make the target's meaning depend on the order it was read in.
 */
function upsertItem<TItem extends NamedAssetItem>(items: TItem[], item: TItem): TItem[] {
    const index = items.findIndex((entry) => entry?.name === item.name);
    if (index < 0) {
        return [...items, item];
    }
    return items.map((entry, at) => (at === index ? item : entry));
}

function removeItem<TItem extends NamedAssetItem>(items: TItem[], name: string): TItem[] {
    return items.filter((entry) => entry?.name !== name);
}

function requireName(name: string): string {
    const trimmed = String(name ?? '').trim();
    if (!trimmed) {
        throw new Error(tr('embeddedAssetNameRequired'));
    }
    return trimmed;
}

/**
 * Carry one stored document in this target.
 *
 * The stored document is the source rather than the editor's draft, so what
 * lands in a card is what the backend would hand back on a get.
 */
export async function embedNamedItem<TItem extends NamedAssetItem, TDocument>(
    carrier: NamedAssetCarrier,
    kind: NamedAssetKind<TItem, TDocument>,
    name: string,
    loadDocument: (name: string) => Promise<TDocument>,
): Promise<string> {
    const trimmed = requireName(name);
    const packageValue = kind.readPackage(carrier.read());
    const item = kind.itemOf(trimmed, await loadDocument(trimmed));
    await carrier.write({ ...packageValue, items: upsertItem(packageValue.items, item) });
    return trimmed;
}

export async function removeEmbeddedNamedItem<TItem extends NamedAssetItem, TDocument>(
    carrier: NamedAssetCarrier,
    kind: NamedAssetKind<TItem, TDocument>,
    name: string,
): Promise<void> {
    const trimmed = requireName(name);
    const packageValue = kind.readPackage(carrier.read());
    await carrier.write({ ...packageValue, items: removeItem(packageValue.items, trimmed) });
}
