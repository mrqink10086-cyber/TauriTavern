import { expect, test } from '@rstest/core';

import { readEmbeddedMachinePackage, portableEmbeddedMachine } from './embedded-asset-packages';
import {
    embedNamedItem,
    readEmbeddedNamedItems,
    removeEmbeddedNamedItem,
    type NamedAssetCarrier,
    type NamedAssetKind,
} from './embedded-named-assets';
import type { MachineSpec } from './state-machine-model';
import type { StoredEmbeddedMachineItem } from './embedded-asset-packages';

const MACHINES: NamedAssetKind<StoredEmbeddedMachineItem, MachineSpec> = {
    readPackage: readEmbeddedMachinePackage,
    itemOf: portableEmbeddedMachine,
};

function machine(states: string[]): MachineSpec {
    return {
        initial: states.slice(0, 1),
        states: states.map((id) => ({ id })),
        transitions: [],
    };
}

function createCarrier(existing: unknown = undefined) {
    const writes: { version: number; items: StoredEmbeddedMachineItem[] }[] = [];
    const carrier: NamedAssetCarrier = {
        read: () => existing,
        write: (packageValue) => {
            writes.push(packageValue as { version: number; items: StoredEmbeddedMachineItem[] });
            return Promise.resolve();
        },
    };
    return { carrier, writes };
}

const loaded = (document: MachineSpec) => () => Promise.resolve(document);

test('a target with no carrier reads as empty', () => {
    const { carrier } = createCarrier();
    expect(readEmbeddedNamedItems(carrier, MACHINES)).toEqual([]);
});

test('carrying one document writes it under its name', async () => {
    const { carrier, writes } = createCarrier();

    expect(await embedNamedItem(carrier, MACHINES, ' flow ', loaded(machine(['day'])))).toBe('flow');
    expect(writes[0]?.items.map((item) => item.name)).toEqual(['flow']);
    expect(writes[0]?.items[0]?.machine.states).toEqual([{ id: 'day' }]);
});

test('carrying the same name again replaces that item and keeps the others', async () => {
    const { carrier, writes } = createCarrier({
        version: 1,
        items: [portableEmbeddedMachine('other', machine(['x'])), portableEmbeddedMachine('flow', machine(['old']))],
    });

    await embedNamedItem(carrier, MACHINES, 'flow', loaded(machine(['new'])));
    expect(writes[0]?.items.map((item) => item.name)).toEqual(['other', 'flow']);
    expect(writes[0]?.items[1]?.machine.states).toEqual([{ id: 'new' }]);
});

test('an empty name is refused before anything is read or written', async () => {
    const { carrier, writes } = createCarrier();

    await expect(embedNamedItem(carrier, MACHINES, '   ', loaded(machine(['day'])))).rejects.toThrow();
    await expect(removeEmbeddedNamedItem(carrier, MACHINES, '')).rejects.toThrow();
    expect(writes).toEqual([]);
});

test('taking one back off leaves the rest of the carrier alone', async () => {
    const { carrier, writes } = createCarrier({
        version: 1,
        items: [portableEmbeddedMachine('other', machine(['x'])), portableEmbeddedMachine('flow', machine(['y']))],
    });

    await removeEmbeddedNamedItem(carrier, MACHINES, 'flow');
    expect(writes[0]?.items.map((item) => item.name)).toEqual(['other']);
});
