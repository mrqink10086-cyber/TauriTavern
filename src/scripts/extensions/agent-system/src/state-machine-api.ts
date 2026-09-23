/**
 * The state machine endpoints, as the editor uses them.
 *
 * The routes are the app's own HTTP surface (`/api/state-machines/*`), the same
 * one every other settings page uses; the host turns each of them into the
 * matching Tauri command. Nothing here decides anything: the backend validates
 * and refuses, and its message is what the editor shows.
 */

import type { MachineRunDto, MachineSpec } from './state-machine-model';

const STATE_MACHINE_ROUTES = Object.freeze({
    list: '/api/state-machines/list',
    get: '/api/state-machines/get',
    save: '/api/state-machines/save',
    delete: '/api/state-machines/delete',
    validate: '/api/state-machines/validate',
    evaluate: '/api/state-machines/evaluate',
});

async function postMachineRoute<T>(url: string, body: unknown): Promise<T> {
    const response = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body ?? {}),
    });
    if (!response.ok) {
        const details = String(await response.text()).trim();
        throw new Error(details || response.statusText || `HTTP ${response.status}`);
    }
    return await response.json() as T;
}

export async function listStateMachines(): Promise<string[]> {
    const names = await postMachineRoute<unknown>(STATE_MACHINE_ROUTES.list, {});
    return Array.isArray(names) ? names.map((name) => String(name)) : [];
}

export async function getStateMachine(name: string): Promise<MachineSpec> {
    const machine = await postMachineRoute<MachineSpec>(STATE_MACHINE_ROUTES.get, { name });
    if (!machine || typeof machine !== 'object' || !Array.isArray(machine.states) || !Array.isArray(machine.transitions)) {
        throw new Error(`state machine \`${name}\` is not a machine document`);
    }
    return machine;
}

export async function saveStateMachine(name: string, machine: MachineSpec): Promise<void> {
    await postMachineRoute(STATE_MACHINE_ROUTES.save, { name, machine });
}

export async function deleteStateMachine(name: string): Promise<void> {
    await postMachineRoute(STATE_MACHINE_ROUTES.delete, { name });
}

/**
 * Run a machine once against assumed inputs.
 *
 * `active` omitted or empty means "start from the machine's own initial
 * positions" — the same default the backend applies, so an empty preview is
 * still meaningful.
 */
export async function evaluateStateMachine(input: {
    machine: MachineSpec;
    active?: string[];
    fields?: Record<string, string[]>;
}): Promise<MachineRunDto> {
    const run = await postMachineRoute<MachineRunDto>(STATE_MACHINE_ROUTES.evaluate, input);
    if (!run || typeof run !== 'object' || !run.evaluation) {
        throw new Error('state machine evaluation returned no evaluation');
    }
    return run;
}
