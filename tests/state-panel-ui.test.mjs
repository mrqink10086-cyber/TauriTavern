import assert from 'node:assert/strict';
import { test } from 'node:test';

import { Window } from 'happy-dom';

import {
    createStatePanel,
    renderStatePanel,
    renderStatePanelField,
    statePanelBackgroundValue,
} from '../src/scripts/tauritavern/state/state-panel-ui.js';
import { EDGE_MARGIN } from '../src/scripts/tauritavern/state/state-panel-view.js';

function context() {
    const window = new Window({ url: 'https://localhost/' });
    const mount = window.document.createElement('div');
    window.document.body.append(mount);
    return { window, document: window.document, mount };
}

/** A storage double: one value, and a way to read what was written. */
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

/** A pointer event at a point; a mouse event of the same type will do. */
function pointer(window, type, clientX, clientY = 0) {
    const PointerEvent = window.PointerEvent ?? window.MouseEvent;
    return new PointerEvent(type, { clientX, clientY, bubbles: true });
}

test('a panel background is only emitted for a plain host path', () => {
    assert.equal(
        statePanelBackgroundValue('/backgrounds/night.png'),
        'url("/backgrounds/night.png")',
    );
    for (const source of [
        'https://example.test/scene.png',
        '/backgrounds/night.png") , url("https://example.test/x',
        '/backgrounds/../secrets.png',
        '',
    ]) {
        assert.equal(
            statePanelBackgroundValue(source),
            null,
            `\`${source}\` must never reach CSS`,
        );
    }
});

test('a field with values renders its text', () => {
    const { document } = context();

    const field = renderStatePanelField(document, {
        key: '环境/日期',
        label: 'DATE',
        body: { kind: 'text', values: ['2026/09/10'] },
    });

    assert.equal(field.dataset.key, '环境/日期');
    assert.equal(field.querySelector('.tt-state-field__label').textContent, 'DATE');
    assert.equal(field.querySelector('.tt-state-field__value').textContent, '2026/09/10');
});

test('a field with pictures renders one image per source and skips unusable ones', () => {
    const { document } = context();

    const field = renderStatePanelField(document, {
        key: '角色/艾拉/立绘',
        label: 'PORTRAIT',
        body: {
            kind: 'images',
            sources: ['/user/images/aira.png', 'https://example.test/evil.png'],
        },
    });

    const images = [...field.querySelectorAll('.tt-state-field__images img')];
    assert.equal(images.length, 1, 'only the host path may load');
    assert.equal(images[0].getAttribute('src'), '/user/images/aira.png');
});

test('a panel carries its own background and its fields', () => {
    const { document } = context();

    const panel = renderStatePanel(document, {
        title: '环境',
        background: '/backgrounds/cafe.png',
        fields: [{ key: '环境/地点', label: 'PLACE', body: { kind: 'text', values: ['咖啡馆'] } }],
    });

    const body = panel.querySelector('.tt-state-panel__body');
    assert.equal(panel.dataset.panel, '环境');
    assert.match(body.style.backgroundImage, /cafe\.png/);
    assert.equal(body.querySelectorAll('.tt-state-field').length, 1);
});

test('a floor that did not update state marks every panel it shows', () => {
    const { document, mount } = context();
    const panel = createStatePanel({ document, mount });

    panel.render([{ title: '环境', rail: 'left', fields: [] }]);
    assert.equal(
        panel.element.querySelector('.tt-state-panel__stale'),
        null,
        'a floor that updated state carries no mark',
    );

    panel.render([
        { title: '环境', rail: 'left', fields: [] },
        { title: '角色', rail: 'right', fields: [] },
    ], false);

    assert.equal(
        panel.element.querySelectorAll('.tt-state-panel__stale').length,
        2,
        'the mark is about the floor, so every panel showing that floor carries it',
    );
});

test('panels become windows, and an empty render leaves none behind', () => {
    const { document, mount } = context();
    const panel = createStatePanel({ document, mount });

    const shown = panel.render([
        { title: '角色', fields: [] },
        { title: '环境', fields: [] },
    ]);

    assert.equal(shown, 2);
    assert.deepEqual(
        [...panel.element.querySelectorAll('.tt-state-panel')].map((node) => node.dataset.panel),
        ['角色', '环境'],
    );
    // Placed by the host rather than laid out by a rail: a window has a place
    // and a size of its own, and the data does not decide which side it is on.
    const first = panel.element.querySelector('.tt-state-panel');
    assert.notEqual(first.style.left, '');
    assert.notEqual(first.style.width, '');
    assert.equal(first.dataset.panelId, '角色');

    panel.render([]);
    assert.equal(panel.element.querySelectorAll('.tt-state-panel').length, 0);
});

test('rendering replaces the previous slice instead of stacking it', () => {
    const { document, mount } = context();
    const panel = createStatePanel({ document, mount });

    panel.render([{ title: '环境', rail: 'left', fields: [{ key: '环境/日期', label: 'DATE', body: { kind: 'text', values: ['a'] } }] }]);
    panel.render([{ title: '角色', rail: 'right', fields: [] }]);

    assert.equal(panel.element.querySelectorAll('.tt-state-panel').length, 1);
    assert.equal(panel.element.querySelector('.tt-state-panel').dataset.panel, '角色');
});

test('the theme sheet is applied as text and lives only as long as the theme does', () => {
    const { document, mount } = context();
    const panel = createStatePanel({ document, mount });

    const sheet = '.tt-state-root .tt-state-field { color: red }';
    panel.setTheme(sheet);

    const style = document.head.querySelector('.tt-state-theme');
    assert.equal(style?.textContent, sheet);
    assert.equal(document.querySelectorAll('.tt-state-theme').length, 1);

    // A refresh hands the same sheet back: the element is reused, so an idle
    // refresh writes nothing into the document.
    panel.setTheme(sheet);
    assert.equal(document.head.querySelector('.tt-state-theme'), style);

    panel.setTheme('.tt-state-root .tt-state-field { color: blue }');
    assert.equal(document.head.querySelector('.tt-state-theme'), style);
    assert.equal(style.textContent, '.tt-state-root .tt-state-field { color: blue }');

    panel.setTheme('');
    assert.equal(document.head.querySelector('.tt-state-theme'), null);

    panel.setTheme(sheet);
    panel.destroy();
    assert.equal(
        document.head.querySelector('.tt-state-theme'),
        null,
        'a destroyed panel leaves no sheet behind',
    );
});

test('a templated panel draws its own markup inside the panel body', () => {
    const { document } = context();
    const panel = renderStatePanel(document, {
        title: '环境',
        fields: [
            { key: '环境/日期', label: 'DATE', body: { kind: 'text', values: ['2026/09/10'] } },
            { key: '环境/天气', label: 'WEATHER', body: { kind: 'text', values: [] } },
        ],
        markup: {
            nodes: [
                {
                    kind: 'element',
                    tag: 'strong',
                    attrs: { class: 'day' },
                    children: [{ kind: 'value', key: '环境/日期' }],
                },
                {
                    kind: 'if',
                    key: '环境/天气',
                    then: [{ kind: 'text', text: '有天气' }],
                    else: [{ kind: 'text', text: '没天气' }],
                },
            ],
        },
    });

    assert.equal(panel.dataset.templated, 'true');
    assert.equal(panel.querySelector('.tt-state-panel__header').textContent, '环境');
    const body = panel.querySelector('.tt-state-panel__body');
    assert.equal(body.querySelector('strong.day').textContent, '2026/09/10');
    assert.equal(body.textContent, '2026/09/10没天气');
    // The default rows are what the template replaces.
    assert.equal(body.querySelectorAll('.tt-state-field').length, 0);
});

test('a value is inserted as text, never as markup', () => {
    const { document } = context();
    const panel = renderStatePanel(document, {
        title: '环境',
        fields: [{ key: '环境/日期', label: 'DATE', body: { kind: 'text', values: ['<b>x</b>'] } }],
        markup: {
            nodes: [{
                kind: 'element',
                tag: 'div',
                attrs: {},
                children: [{ kind: 'value', key: '环境/日期' }],
            }],
        },
    });

    const body = panel.querySelector('.tt-state-panel__body');
    assert.equal(body.querySelector('b'), null);
    assert.equal(body.textContent, '<b>x</b>');
});

test('a panel folds to its title, and is still folded the next time it is built', () => {
    const { window, document, mount } = context();
    const store = storage();
    const panel = createStatePanel({ document, mount, storage: store });
    panel.render([{
        title: '环境',
        rail: 'left',
        fields: [{ key: '环境/日期', label: 'DATE', body: { kind: 'text', values: ['2026/09/10'] } }],
    }]);

    const node = panel.element.querySelector('.tt-state-panel');
    node.querySelector('.tt-state-panel__header')
        .dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
    assert.equal(node.classList.contains('tt-state-panel--folded'), true);

    const reopened = createStatePanel({ document, mount, storage: store });
    reopened.render([{ title: '环境', rail: 'left', fields: [] }]);
    assert.equal(
        reopened.element.querySelector('.tt-state-panel').classList.contains('tt-state-panel--folded'),
        true,
        'the arrangement is the user\'s, so it survives a reload',
    );
});

test('a drag moves a panel, and the place is committed when the pointer is released', () => {
    const { window, document, mount } = context();
    const store = storage();
    const panel = createStatePanel({ document, mount, storage: store });
    panel.render([{ title: '环境', fields: [] }]);

    const node = panel.element.querySelector('.tt-state-panel');
    const startLeft = Number.parseFloat(node.style.left);
    const startTop = Number.parseFloat(node.style.top);
    node.querySelector('.tt-state-panel__header')
        .dispatchEvent(pointer(window, 'pointerdown', 100, 100));
    document.dispatchEvent(pointer(window, 'pointermove', 160, 130));

    // While the pointer is down only a transform moves: the place in force is
    // the one from before the drag, so nothing re-lays-out mid-drag.
    assert.equal(node.style.left, `${startLeft}px`);
    assert.match(node.style.transform, /translate\(/);

    document.dispatchEvent(pointer(window, 'pointerup', 160, 130));
    assert.equal(node.style.transform, '');
    // A test document reports no room, so the place is clamped back to the
    // margin; what matters here is that the drag wrote one down.
    assert.match(store.read(), /"x":/);
});

test('a drag cannot push a panel out of the room it has', () => {
    const { window, document, mount } = context();
    const panel = createStatePanel({ document, mount, storage: storage() });
    panel.render([{ title: '环境', fields: [] }]);

    const node = panel.element.querySelector('.tt-state-panel');
    node.querySelector('.tt-state-panel__header')
        .dispatchEvent(pointer(window, 'pointerdown', 0, 0));
    document.dispatchEvent(pointer(window, 'pointermove', 5000, 5000));
    document.dispatchEvent(pointer(window, 'pointerup', 5000, 5000));

    // A test document reports no room at all, so the margin is the only place
    // left: the panel sits there rather than five thousand pixels away.
    assert.equal(Number.parseFloat(node.style.left), EDGE_MARGIN);
    assert.equal(Number.parseFloat(node.style.top), EDGE_MARGIN);
});

test('a prose block is drawn once the panel names a file, even while it is empty', () => {
    const { document } = context();

    const empty = renderStatePanel(document, {
        title: '内心',
        prose: { title: '心声', text: '', path: 'persist/heart.md' },
    });
    assert.equal(empty.querySelector('.tt-state-prose').dataset.prose, 'persist/heart.md');
    assert.equal(empty.querySelector('.tt-state-prose__text').textContent, '');

    // A panel that declares no prose block has nothing to draw.
    const without = renderStatePanel(document, { title: '环境', fields: [] });
    assert.equal(without.querySelector('.tt-state-prose'), null);
});

test('the prose pencil writes the file the block came from', async () => {
    const { document, mount } = context();
    const writes = [];
    const panel = createStatePanel({
        document,
        mount,
        onWriteProse: async (request) => {
            writes.push(request);
        },
    });

    panel.render([{
        title: '内心',
        rail: 'left',
        fields: [],
        prose: { title: '心声', text: '昨天的字', path: 'persist/heart.md' },
    }]);

    const node = panel.element.querySelector('.tt-state-prose');
    assert.equal(node.querySelector('.tt-state-prose__text').textContent, '昨天的字');

    node.querySelector('.tt-state-prose__edit').click();
    const input = panel.element.querySelector('.tt-state-prose__editor-input');
    assert.equal(input.value, '昨天的字', 'the editor opens on what is written');

    input.value = '今天的字';
    [...panel.element.querySelectorAll('.tt-state-prose__editor-action')]
        .find((button) => button.textContent === '保存')
        .click();
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.deepEqual(writes, [{ path: 'persist/heart.md', text: '今天的字' }]);
});

test('the refresh button survives a render and asks for a re-read when clicked', () => {
    const { document, mount } = context();
    let refreshes = 0;
    const panel = createStatePanel({ document, mount, onRefresh: () => { refreshes += 1; } });

    const button = panel.element.querySelector('.tt-state-refresh');
    assert.ok(button, 'the panel offers a way to re-read itself');
    assert.equal(button.type, 'button');

    // Drawn twice: a rail replaces its children, the root does not.
    panel.render([{ title: '环境', rail: 'left', fields: [] }]);
    panel.render([{ title: '环境', rail: 'left', fields: [] }, { title: '角色', rail: 'right', fields: [] }]);
    assert.equal(panel.element.querySelectorAll('.tt-state-refresh').length, 1);

    button.click();
    assert.equal(refreshes, 1);
});

test('a panel built without a refresh callback does not throw when the button is clicked', () => {
    const { document, mount } = context();
    const panel = createStatePanel({ document, mount });

    panel.element.querySelector('.tt-state-refresh').click();
});
