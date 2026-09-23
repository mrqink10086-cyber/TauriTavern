/**
 * The first screen of both editors.
 *
 * What these lock in is the thing a blank form could not say: what the tab is
 * for, and that a new document is one click away from a worked example rather
 * than an empty grid.
 */

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useSyncExternalStore } from 'react';
import { afterEach, expect, test } from '@rstest/core';

import { machineConfigFor, predicateConfigFor, stateConfigFor, tr } from './PanelTestWorld';
import { StateConfigPanel } from './StateConfigPanel';
import { StateMachinePanel } from './StateMachinePanel';
import { StatePredicatePanel } from './StatePredicatePanel';
import type { StateConfigController } from './state-config-controller';
import { DEFAULT_DECLARATION_NAME, DEFAULT_MACHINE_NAME, DEFAULT_PREDICATE_NAME } from './state-examples';
import type { MachineConfigController } from './state-machine-controller';
import type { PredicateConfigController } from './state-predicate-controller';

const disposables: Array<{ dispose: () => void }> = [];

afterEach(() => {
    cleanup();
    disposables.splice(0).forEach((disposable) => disposable.dispose());
});

/** The tab's own wiring: the panels render a snapshot, the app subscribes. */
function DeclarationTab({ controller }: { controller: StateConfigController }) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    return <StateConfigPanel snapshot={snapshot} controller={controller} tr={tr} />;
}

function MachineTab({ controller }: { controller: MachineConfigController }) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    return <StateMachinePanel snapshot={snapshot} controller={controller} tr={tr} />;
}

function PredicateTab({ controller }: { controller: PredicateConfigController }) {
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    return <StatePredicatePanel snapshot={snapshot} controller={controller} tr={tr} />;
}

test('the declaration tab explains itself and offers the example under a ready name', async () => {
    const controller = stateConfigFor(new Map());
    disposables.push(controller);
    const user = userEvent.setup();
    render(<DeclarationTab controller={controller} />);
    await controller.init();

    expect(screen.getByText('stateDeclarationGuideTitle')).toBeTruthy();
    // The guide carries what the tab can do, not just a headline.
    expect(screen.getByText('stateDeclarationGuidePanel')).toBeTruthy();
    const name = screen.getByRole<HTMLInputElement>('textbox', { name: 'stateDeclarationName' });
    expect(name.value).toBe(DEFAULT_DECLARATION_NAME);

    await user.click(screen.getByRole('button', { name: 'stateDeclarationCreate' }));

    expect(screen.getByDisplayValue('环境/日期')).toBeTruthy();
    expect(screen.getByText('stateDeclarationExampleLoaded')).toBeTruthy();
});

test('the theme box writes the sheet the declaration carries', async () => {
    const controller = stateConfigFor(new Map());
    disposables.push(controller);
    const user = userEvent.setup();
    render(<DeclarationTab controller={controller} />);
    await controller.init();
    await user.click(screen.getByRole('button', { name: 'stateDeclarationCreate' }));

    const theme = screen.getByLabelText<HTMLTextAreaElement>('stateDeclarationTheme');
    // A new declaration opens with a sheet already in it: the example carries the
    // middle style level, not just fields.
    expect(theme.value).toContain('.scene');

    await user.clear(theme);
    // `{{` is how user-event types a literal opening brace.
    await user.type(theme, '.tt-state-field {{ color: red }');

    expect(controller.getSnapshot().draft?.panels?.css).toBe('.tt-state-field { color: red }');
});

test('the machine tab explains itself and offers the example under a ready name', async () => {
    const controller = machineConfigFor();
    disposables.push(controller);
    const user = userEvent.setup();
    render(<MachineTab controller={controller} />);
    await controller.init();

    expect(screen.getByText('machineGuideTitle')).toBeTruthy();
    expect(screen.getByText('machineGuidePreview')).toBeTruthy();
    const name = screen.getByRole<HTMLInputElement>('textbox', { name: 'machineName' });
    expect(name.value).toBe(DEFAULT_MACHINE_NAME);

    await user.click(screen.getByRole('button', { name: 'machineCreate' }));

    const stateIds = screen.getAllByRole<HTMLInputElement>('textbox', { name: 'machineStateId' });
    expect(stateIds.map((input) => input.value)).toEqual(['白天', '夜晚']);
});

test('the predicate tab explains itself and offers the example under a ready name', async () => {
    const controller = predicateConfigFor();
    disposables.push(controller);
    const user = userEvent.setup();
    render(<PredicateTab controller={controller} />);
    await controller.init();

    expect(screen.getByText('predicateGuideTitle')).toBeTruthy();
    // A group and a standing entry are the two shapes, said in the guide.
    expect(screen.getByText('predicateGuideGroups')).toBeTruthy();
    expect(screen.getByText('predicateGuideConstants')).toBeTruthy();
    const name = screen.getByRole<HTMLInputElement>('textbox', { name: 'predicateName' });
    expect(name.value).toBe(DEFAULT_PREDICATE_NAME);

    await user.click(screen.getByRole('button', { name: 'predicateCreate' }));

    expect(screen.getByDisplayValue('tone')).toBeTruthy();
    const ids = screen.getAllByRole<HTMLInputElement>('textbox', { name: 'predicateEntryId' });
    expect(ids.map((input) => input.value)).toEqual(['distant', 'close', 'standing']);
});

test('the declaration tab shows where the selection can be bound', async () => {
    const controller = stateConfigFor(new Map());
    disposables.push(controller);
    render(<DeclarationTab controller={controller} />);
    await controller.init();

    expect(screen.getByText('stateBindingLabel')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'stateBindingChat' })).toBeTruthy();
});
