import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
    EDGE_MARGIN,
    MAX_PANEL_SHARE,
    MIN_PANEL_HEIGHT,
    MIN_PANEL_WIDTH,
    clampPanelRect,
    defaultPanelRect,
    emptyStatePanelView,
    panelBounds,
    readStatePanelView,
    writeStatePanelView,
} from '../src/scripts/tauritavern/state/state-panel-view.js';

/** A storage double: one value, no keys to care about. */
function storage(initial = null) {
    let value = initial;
    return {
        getItem: () => value,
        setItem: (_key, next) => {
            value = next;
        },
        read: () => value,
    };
}

test('a panel size stays inside the room it has', () => {
    // Nothing measurable to divide up: only the absolute ceiling applies.
    assert.deepEqual(panelBounds(null), {
        minW: MIN_PANEL_WIDTH,
        minH: MIN_PANEL_HEIGHT,
        maxW: 720,
        maxH: 840,
    });

    assert.deepEqual(panelBounds({ width: 1000, height: 800 }), {
        minW: MIN_PANEL_WIDTH,
        minH: MIN_PANEL_HEIGHT,
        maxW: 1000 * MAX_PANEL_SHARE,
        maxH: 800 * MAX_PANEL_SHARE,
    });

    // A room smaller than the floor: the floor becomes the room rather than
    // describing a panel wider than the window it lives in.
    assert.deepEqual(panelBounds({ width: 120, height: 90 }), {
        minW: 120,
        minH: 90,
        maxW: MIN_PANEL_WIDTH,
        maxH: MIN_PANEL_HEIGHT,
    });
});

test('a stored place is pulled back inside the room it is opened in', () => {
    // A place is a memory of another window: one dragged to the far corner of a
    // wide screen has no business being half off a narrow one.
    assert.deepEqual(clampPanelRect({ x: 5000, y: 5000, w: 400, h: 300 }, { width: 800, height: 600 }), {
        x: 800 - 400 - EDGE_MARGIN,
        y: 600 - 300 - EDGE_MARGIN,
        w: 400,
        h: 300,
        folded: false,
    });

    // Nothing usable: it lands at the corner rather than at zero.
    assert.deepEqual(clampPanelRect(null, { width: 800, height: 600 }), {
        x: EDGE_MARGIN,
        y: EDGE_MARGIN,
        w: MIN_PANEL_WIDTH,
        h: MIN_PANEL_HEIGHT,
        folded: false,
    });

    const big = clampPanelRect({ x: 0, y: 0, w: 99999, h: 99999 }, { width: 800, height: 600 });
    assert.equal(big.w, 800 * MAX_PANEL_SHARE);
    assert.equal(big.h, 600 * MAX_PANEL_SHARE);
});

test('panels that were never placed cascade instead of stacking', () => {
    const first = defaultPanelRect(0, { width: 1000, height: 800 });
    const second = defaultPanelRect(1, { width: 1000, height: 800 });
    assert.ok(second.x > first.x && second.y > first.y);

    // And the cascade is clamped rather than allowed to walk off the screen.
    const far = defaultPanelRect(50, { width: 600, height: 400 });
    assert.ok(far.x + far.w <= 600);
    assert.ok(far.y + far.h <= 400);
});

test('a v1 view keeps what still means something once the rails become windows', () => {
    const migrated = readStatePanelView(storage(JSON.stringify({
        widths: { left: 300, right: null },
        tucked: { left: false, right: false },
        folded: ['环境'],
    })));

    assert.equal(migrated.version, 2);
    // A width has nowhere to go — a rail's width is not a window's place — but
    // a fold still means the same thing, under the name a v1 view used for it.
    assert.equal(migrated.panels['环境']?.folded, true);
    assert.equal(migrated.open, true);
});

test('a stored view comes back, and damage is repaired rather than trusted', () => {
    const store = storage();
    const view = emptyStatePanelView();
    view.open = false;
    view.panels['环境'] = { x: 40, y: 60, w: 320, h: 260, folded: true };
    writeStatePanelView(store, view);
    assert.deepEqual(readStatePanelView(store), view);

    assert.deepEqual(readStatePanelView(storage('not json at all')), emptyStatePanelView());
    // A place that is not numbers is dropped, and a panel with no usable place
    // is not remembered at all — it lands where a new panel lands.
    assert.deepEqual(
        readStatePanelView(storage('{"version":2,"panels":{"环境":{"x":"left","y":0,"w":0,"h":0}}}')),
        emptyStatePanelView(),
    );
});
