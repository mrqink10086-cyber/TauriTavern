// @ts-check

/**
 * The state panel: floating windows over the chat, filled from the host's data.
 *
 * The host resolves what a panel shows (which fields, which picture the state
 * selected); this module only builds the DOM. That keeps one place that decides
 * anything and one place that draws it, and it keeps the panel free of state
 * parsing — there is nothing here to parse, only values to place.
 *
 * The class names are a contract: user CSS and HTML templates target them, so
 * they change only with a compatibility story. The user's theme sheet arrives
 * already compiled — every selector carries the root class — so applying it is
 * an assignment, never a parse.
 *
 * A panel is a window: dragged by its header, sized by its corner, and left
 * wherever the user put it (`state-panel-view.js`). The root is a full-window
 * layer that passes pointer events through everywhere except onto a panel, so
 * the chat underneath stays usable with panels open.
 */

import {
    clampPanelRect,
    defaultPanelRect,
    panelBounds,
    readStatePanelView,
    writeStatePanelView,
} from './state-panel-view.js';

const ROOT_CLASS = 'tt-state-root';
const ROOT_OPEN_CLASS = 'tt-state-root--open';
const THEME_STYLE_CLASS = 'tt-state-theme';
const LAUNCHER_CLASS = 'tt-state-launcher';
const PANEL_CLASS = 'tt-state-panel';
const PANEL_HEADER_CLASS = 'tt-state-panel__header';
const PANEL_RESIZE_CLASS = 'tt-state-panel__resize';
const PANEL_FOLDED_CLASS = 'tt-state-panel--folded';
const PANEL_BODY_CLASS = 'tt-state-panel__body';
const STALE_MARK_CLASS = 'tt-state-panel__stale';
const FIELD_CLASS = 'tt-state-field';
/** The prose block: a section of its own, after the panel body. */
const PROSE_CLASS = 'tt-state-prose';
const PROSE_TITLE_CLASS = 'tt-state-prose__title';
const PROSE_TEXT_CLASS = 'tt-state-prose__text';
const PROSE_EDIT_CLASS = 'tt-state-prose__edit';
const PROSE_EDITOR_CLASS = 'tt-state-prose__editor';
const PROSE_EDITOR_INPUT_CLASS = 'tt-state-prose__editor-input';
const PROSE_EDITOR_ACTIONS_CLASS = 'tt-state-prose__editor-actions';
const PROSE_EDITOR_ACTION_CLASS = 'tt-state-prose__editor-action';
const PROSE_EDITOR_ERROR_CLASS = 'tt-state-prose__editor-error';
const FIELD_LABEL_CLASS = 'tt-state-field__label';
const FIELD_VALUE_CLASS = 'tt-state-field__value';
const FIELD_IMAGES_CLASS = 'tt-state-field__images';
const FIELD_EDIT_CLASS = 'tt-state-field__edit';
const FIELD_EDITOR_CLASS = 'tt-state-field__editor';
const FIELD_EDITOR_INPUT_CLASS = 'tt-state-field__editor-input';
const FIELD_EDITOR_ACTIONS_CLASS = 'tt-state-field__editor-actions';
const FIELD_EDITOR_ACTION_CLASS = 'tt-state-field__editor-action';
const FIELD_EDITOR_ERROR_CLASS = 'tt-state-field__editor-error';
const REFRESH_CLASS = 'tt-state-refresh';

/**
 * Local storage, or nothing.
 *
 * Private modes and sandboxed documents can refuse the property outright, and a
 * panel that cannot remember its arrangement is still a panel.
 *
 * @returns {Storage | null}
 */
function safeStorage() {
    try {
        return globalThis.localStorage ?? null;
    } catch {
        return null;
    }
}

/**
 * The CSS value for a panel background, or null when the source cannot be used
 * as one.
 *
 * The host refuses anything but a plain path, and this repeats the check at the
 * point of use: a value that reaches CSS would otherwise be able to close the
 * `url(` and add declarations of its own.
 *
 * @param {string | null | undefined} source
 * @returns {string|null}
 */
export function statePanelBackgroundValue(source) {
    const value = String(source ?? '');
    if (!value.startsWith('/') || /[\s"'()\\:]|\.\./u.test(value)) {
        return null;
    }
    return `url("${value}")`;
}

/**
 * @param {Document} doc
 * @param {string} tag
 * @param {string} className
 * @param {string} [text]
 */
function element(doc, tag, className, text) {
    const node = doc.createElement(tag);
    node.className = className;
    if (text !== undefined) {
        node.textContent = text;
    }
    return node;
}

/**
 * One field: a label plus either its values or its pictures.
 *
 * @param {Document} doc
 * @param {StatePanelFieldDto} field
 * @returns {HTMLElement}
 */
export function renderStatePanelField(doc, field) {
    const node = element(doc, 'div', FIELD_CLASS);
    node.dataset.key = String(field?.key ?? '');
    node.append(element(doc, 'span', FIELD_LABEL_CLASS, String(field?.label ?? '')));

    const body = field?.body ?? { kind: 'text' };
    if (body.kind === 'images') {
        const holder = element(doc, 'div', FIELD_IMAGES_CLASS);
        // How the picture fills its box is the set's decision, not the
        // renderer's: a portrait that the panel's aspect crops is not the
        // person it is a picture of.
        holder.dataset.fit = body.fit === 'contain' ? 'contain' : 'cover';
        for (const source of Array.isArray(body.sources) ? body.sources : []) {
            const usable = statePanelBackgroundValue(source);
            if (!usable) {
                continue;
            }
            const image = doc.createElement('img');
            image.loading = 'lazy';
            image.src = String(source);
            image.alt = String(field?.label ?? '');
            holder.append(image);
        }
        node.append(holder);
        return node;
    }

    const values = Array.isArray(body.values) ? body.values : [];
    node.append(element(doc, 'span', FIELD_VALUE_CLASS, values.join(', ')));
    return node;
}

/**
 * The mark a panel carries when the newest floor did not update state.
 *
 * The panel always shows the newest committed version, so a floor that wrote no
 * state leaves it showing the previous one. The mark is what keeps that from
 * reading as "this is current"; it carries its meaning in a title because the
 * panel has no other user-facing copy to translate.
 *
 * @param {Document} doc
 * @returns {HTMLElement}
 */
function staleMark(doc) {
    const mark = element(doc, 'i', `${STALE_MARK_CLASS} fa-solid fa-clock-rotate-left`);
    mark.title = 'This floor did not update state; the panel shows the previous version.';
    return mark;
}

/**
 * One field as the host's resolved panel carries it.
 *
 * `fit` is how the box is filled, and it travels with the resolved body rather
 * than being decided here: the declaration owns it, the renderer only honours
 * it.
 *
 * @typedef {{ key: string; label: string; body: { kind: string; values?: string[]; sources?: string[]; fit?: string } }} StatePanelFieldDto
 */

/**
 * One panel: title, its state-selected background, its fields, and its prose.
 *
 * @param {Document} doc
 * @param {{ title?: string; background?: string | null; backgroundFit?: string | null; fields?: StatePanelFieldDto[]; markup?: { nodes?: unknown[] } | null; prose?: { title?: string; text?: string } | null }} panel
 * @param {StatePanelFieldDto[]} [allFields] the chat's fields, for the template
 * @returns {HTMLElement}
 */
export function renderStatePanel(doc, panel, allFields) {
    const node = element(doc, 'section', PANEL_CLASS);
    node.dataset.panel = String(panel?.title ?? '');
    node.append(element(doc, 'header', PANEL_HEADER_CLASS, String(panel?.title ?? '')));

    const body = element(doc, 'div', PANEL_BODY_CLASS);
    const fields = Array.isArray(panel?.fields) ? panel.fields : [];
    const background = statePanelBackgroundValue(panel?.background);
    if (background) {
        body.style.backgroundImage = background;
        body.dataset.hasBackground = 'true';
        body.dataset.fit = panel?.backgroundFit === 'contain' ? 'contain' : 'cover';
    }

    const markup = panel?.markup;
    if (markup && Array.isArray(markup.nodes) && markup.nodes.length > 0) {
        // A panel with markup says what its body looks like; the header and the
        // body stay, so a theme written against them keeps working.
        node.dataset.templated = 'true';
        renderTemplateNodes(doc, body, markup.nodes, templateFieldValues(panel, allFields));
        // A template places written state and has no way to place a picture:
        // its bindings all end as text. Picture fields are therefore drawn
        // after it — otherwise a declaration that names her portrait resolves
        // it, sends it, and never shows it.
        for (const field of fields) {
            if (String(field?.body?.kind ?? '') !== 'images') {
                continue;
            }
            body.append(renderStatePanelField(doc, field));
        }
    } else {
        for (const field of fields) {
            body.append(renderStatePanelField(doc, field));
        }
    }

    node.append(body);

    // The prose block comes after the body either way: a panel may be drawn from
    // a template or from rows, and what somebody wrote down is a section of its
    // own, not a field. It is kept once the panel declares one even while it is
    // empty — an empty diary is a place to write the first entry, and leaving it
    // out would leave nowhere to start.
    const prose = renderProse(doc, panel?.prose);
    if (prose) {
        node.append(prose);
    }
    return node;
}

/**
 * A panel's prose block: a heading and the text as written.
 *
 * The text is set as text, never as markup: it is written into a file, and a
 * file is not a template. `white-space: pre-wrap` in the stylesheet is what
 * keeps the author's line breaks without any parsing here.
 *
 * The block carries its own file path so the pencil knows what it edits: prose
 * has no key to address it by, and the reader that filled the text is the only
 * one that knows which file it came from.
 *
 * @param {Document} doc
 * @param {{ title?: string; text?: string; path?: string } | null | undefined} prose
 * @returns {HTMLElement | null}
 */
function renderProse(doc, prose) {
    const path = String(prose?.path ?? '').trim();
    if (!path) {
        return null;
    }
    const text = String(prose?.text ?? '');
    const node = element(doc, 'section', PROSE_CLASS);
    node.dataset.prose = path;
    const title = String(prose?.title ?? '').trim();
    if (title) {
        node.append(element(doc, 'header', PROSE_TITLE_CLASS, title));
    }
    node.append(element(doc, 'div', PROSE_TEXT_CLASS, text));
    return node;
}

/**
 * What a template can bind to.
 *
 * A panel's own rows are the floor; the rest of the chat's state is reachable
 * too, because a panel that can only see what its own `match` covers cannot show
 * a summary of the whole scene. Both come from the host already resolved, so a
 * binding reads the same data the default rows would have shown — including a
 * picture field, whose sources are what that field carries.
 *
 * @param {{ fields?: StatePanelFieldDto[] }} panel
 * @param {StatePanelFieldDto[]} [allFields]
 * @returns {Map<string, string[]>}
 */
function templateFieldValues(panel, allFields) {
    const byKey = new Map();
    const sources = [
        ...(Array.isArray(allFields) ? allFields : []),
        ...(Array.isArray(panel?.fields) ? panel.fields : []),
    ];
    for (const field of sources) {
        const body = field?.body ?? {};
        /** @type {unknown[]} */
        const values = Array.isArray(body.values)
            ? body.values
            : (Array.isArray(body.sources) ? body.sources : []);
        byKey.set(String(field?.key ?? ''), values.map((value) => String(value)));
    }
    return byKey;
}

/** `on*` is matched by shape: the set of events belongs to the browser. */
const EVENT_ATTRIBUTE = /^on[a-z]+$/u;

/**
 * Whether one field's value satisfies a template's condition.
 *
 * A comparison with no operator asks the question most templates are asking:
 * does this field carry anything at all.
 */
/**
 * @param {{ key?: string; op?: string; value?: string }} node
 * @param {Map<string, string[]>} values
 * @returns {boolean}
 */
function conditionHolds(node, values) {
    const held = (values.get(String(node.key)) ?? []).filter((value) => value.length > 0);
    const op = String(node.op ?? '');
    if (!op) {
        return held.length > 0;
    }
    if (op === 'exists') {
        return held.length > 0;
    }
    if (op === 'missing') {
        return held.length === 0;
    }
    const right = String(node.value ?? '');
    return held.some((value) => compareValue(value, op, right));
}

/**
 * @param {string} value
 * @param {string} op
 * @param {string} right
 * @returns {boolean}
 */
function compareValue(value, op, right) {
    switch (op) {
        case 'eq':
            return value === right;
        case 'ne':
            return value !== right;
        case 'in':
            return splitValues(right).includes(value);
        case 'not_in':
            return !splitValues(right).includes(value);
        case 'contains':
            return value.includes(right);
        case 'gt':
        case 'gte':
        case 'lt':
        case 'lte': {
            const left = Number(value);
            const bound = Number(right);
            if (!Number.isFinite(left) || !Number.isFinite(bound)) {
                return false;
            }
            if (op === 'gt') return left > bound;
            if (op === 'gte') return left >= bound;
            return op === 'lt' ? left < bound : left <= bound;
        }
        default:
            // An operator this build does not know is not a match; the save
            // refuses those, so this only guards an older stored document.
            return false;
    }
}

/**
 * @param {string} text
 * @returns {string[]}
 */
function splitValues(text) {
    return String(text).split(',').map((value) => value.trim()).filter(Boolean);
}

/**
 * The fields a loop's pattern covers.
 *
 * The rule mirrors the declaration's own matcher — `*` is one segment, `**` is a
 * run of them — and it only filters what the host already resolved, because a
 * template cannot search the key space while a chat is on screen.
 */
/**
 * @param {string} pattern
 * @param {Map<string, string[]>} values
 * @returns {Array<[string, string[]]>}
 */
function matchingEntries(pattern, values) {
    const source = String(pattern).split('/').map((segment) => {
        if (segment === '*') return '[^/]+';
        if (segment === '**') return '.+';
        return segment.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
    }).join('/');
    let matcher;
    try {
        matcher = new RegExp(`^${source}$`, 'u');
    } catch {
        return [];
    }
    return [...values.entries()].filter(([key]) => matcher.test(key));
}

/** An attribute's value, with the parts a field supplies filled in. */
/**
 * @param {any[]} parts
 * @param {Map<string, string[]>} values
 * @returns {string}
 */
function attributeText(parts, values) {
    return (Array.isArray(parts) ? parts : []).map((part) => (
        part?.kind === 'value'
            ? (values.get(String(part.key)) ?? []).join(', ')
            : String(part?.text ?? '')
    )).join('');
}

/**
 * @param {HTMLElement} target
 * @param {string} name
 * @param {string} value
 */
function setTemplateAttribute(target, name, value) {
    const attribute = String(name).toLowerCase();
    if (!EVENT_ATTRIBUTE.test(attribute)) {
        target.setAttribute(attribute, String(value));
        return;
    }
    // A handler is the one piece of code that comes from the document rather
    // than from the application. It is bound rather than assigned so the element
    // is `this` and `closest(...)` works the way plain markup taught everyone.
    try {
        const handler = new Function('event', String(value));
        // Bound to the element explicitly rather than through `this`, so the
        // handler reads the same whether it was written as a function or an arrow.
        const owner = target;
        target.addEventListener(attribute.slice(2), (event) => handler.call(owner, event));
    } catch (error) {
        console.warn('A panel handler could not be compiled:', error);
    }
}

/**
 * Draw one compiled template into a container.
 *
 * The tree was checked before it was stored — no forbidden tags, every binding
 * names a field the declaration defines — and this walk is why that check is
 * enough for everything but handlers: elements are created, attributes are set,
 * and a value is written as text or as part of an attribute. A value never
 * becomes markup, so what the model writes can only ever show up as itself.
 *
 * @param {Document} doc
 * @param {HTMLElement} container
 * @param {any[]} nodes
 * @param {Map<string, string[]>} values
 */
export function renderTemplateNodes(doc, container, nodes, values) {
    for (const node of Array.isArray(nodes) ? nodes : []) {
        if (!node || typeof node !== 'object') {
            continue;
        }
        if (node.kind === 'element') {
            const created = doc.createElement(String(node.tag));
            for (const [name, value] of Object.entries(node.attrs ?? {})) {
                setTemplateAttribute(created, name, value);
            }
            for (const [name, parts] of Object.entries(node.boundAttrs ?? {})) {
                setTemplateAttribute(created, name, attributeText(parts, values));
            }
            renderTemplateNodes(doc, created, node.children, values);
            container.append(created);
            continue;
        }
        if (node.kind === 'text') {
            container.append(doc.createTextNode(String(node.text ?? '')));
            continue;
        }
        if (node.kind === 'value') {
            container.append(doc.createTextNode((values.get(String(node.key)) ?? []).join(', ')));
            continue;
        }
        if (node.kind === 'if') {
            const branch = conditionHolds(node, values) ? node.then : node.else;
            renderTemplateNodes(doc, container, branch, values);
            continue;
        }
        if (node.kind === 'each') {
            // The loop's pattern doubles as the binding a row uses for "this
            // one", so each pass sees it pointing at its own field.
            for (const [key, held] of matchingEntries(node.key, values)) {
                const scoped = new Map(values);
                scoped.set(String(node.key), held);
                scoped.set(`each:${key}`, [key]);
                renderTemplateNodes(doc, container, node.body, scoped);
            }
        }
    }
}

/**
 * The panel host: one root element, two rails, replaced wholesale on refresh.
 *
 * Rendering replaces the children rather than diffing them: the panel is a
 * handful of nodes, and a whole replace cannot leave a stale field behind.
 *
 * The arrangement — how wide a rail is, whether it is tucked away, which panels
 * are folded — is a local preference (`state-panel-view.js`), so it survives a
 * reload and is never written into the declaration.
 *
 * When the host hands it a write path, each text field also carries an editor:
 * a person deciding a value is the same act the model performs with a tool, and
 * it is how a derived value follows — the host runs the scene's recalculation
 * hook after the edit.
 *
 * @param {{
 *   document?: Document;
 *   mount?: HTMLElement | null;
 *   storage?: Storage | null;
 *   onWriteState?: ((request: { fields?: Array<{ key: string; value: string[] }>; remove?: string[] }) => Promise<any>) | null;
 *   onWriteProse?: ((request: { path: string; text: string }) => Promise<any>) | null;
 *   onRefresh?: (() => void) | null;
 * }} [options]
 */
export function createStatePanel(options = {}) {
    const doc = options.document ?? globalThis.document;
    const onWriteState = typeof options.onWriteState === 'function' ? options.onWriteState : null;
    // Prose is written by hand as well as by a model: it is where the text that
    // is not state lives, so the panel offers it the same way it offers a field.
    const onWriteProse = typeof options.onWriteProse === 'function' ? options.onWriteProse : null;
    const onRefresh = typeof options.onRefresh === 'function' ? options.onRefresh : null;
    const root = doc.createElement('div');
    root.className = ROOT_CLASS;
    // The layer is decoration around the chat, never content inside it: screen
    // readers and the chat's own keyboard order must not walk into it. `render`
    // lifts that while an editor is open, because a form is not decoration.
    root.setAttribute('aria-hidden', 'true');

    const launcher = doc.createElement('button');
    launcher.className = `${LAUNCHER_CLASS} fa-solid fa-table-columns`;
    launcher.type = 'button';
    launcher.addEventListener('click', () => setOpen(!view.open));

    // Re-read on demand: the panel otherwise redraws only when the chat or the
    // binding moves, so an edited declaration would wait for the next turn.
    const refreshButton = doc.createElement('button');
    refreshButton.className = `${REFRESH_CLASS} fa-solid fa-rotate`;
    refreshButton.type = 'button';
    refreshButton.title = '重新读取状态并重绘';
    refreshButton.setAttribute('aria-label', '重新读取状态并重绘');
    refreshButton.addEventListener('click', () => onRefresh?.());

    root.append(launcher, refreshButton);

    const mount = options.mount ?? doc.body ?? null;
    mount?.append(root);

    const storage = options.storage ?? safeStorage();
    const view = readStatePanelView(storage);

    const themeStyle = doc.createElement('style');
    themeStyle.className = THEME_STYLE_CLASS;
    /** The sheet currently applied, so an unchanged theme is not re-placed. */
    let appliedTheme = '';

    /**
     * Apply the chat's theme sheet.
     *
     * It goes into the document head rather than the rail: the levels are
     * additive (default style, then the theme), and a sheet that lands after the
     * built-in one is what makes "express the difference" work. An empty theme
     * removes the element instead of leaving an empty one behind, so "not
     * themed" and "themed with nothing" are the same thing to the DOM.
     *
     * @param {string | null | undefined} css
     */
    function setTheme(css) {
        const next = typeof css === 'string' ? css : '';
        if (next === appliedTheme) {
            return;
        }
        appliedTheme = next;
        if (!next) {
            themeStyle.remove();
            return;
        }
        themeStyle.textContent = next;
        (doc.head ?? doc.documentElement)?.append(themeStyle);
    }

    /**
     * The panel's own last data, so an editor can be redrawn on its own.
     *
     * @type {{ panels: Array<any>; stateUpdated: boolean }}
     */
    let shown = { panels: [], stateUpdated: true };

    /**
     * The field whose editor is open, one at a time, and what the user typed.
     *
     * Held here rather than in the DOM because rendering replaces the children:
     * a refresh must not throw away what someone is in the middle of typing.
     *
     * @type {{ key: string; text: string } | null}
     */
    let editing = null;

    /**
     * The open prose editor, if any.
     *
     * Keyed by the file it edits, for the same reason the field editor is keyed
     * by a key: a refresh redraws the panels and must not throw away what
     * somebody is in the middle of writing.
     *
     * @type {{ path: string; text: string } | null}
     */
    let editingProse = null;

    /** The chat's fields: what a template may bind beyond its own panel's rows. @type {StatePanelFieldDto[]} */
    let panelFields = [];

    /**
     * @param {Array<any>} panels
     * @param {boolean} [stateUpdated]
     * @param {StatePanelFieldDto[]} [fields] the chat's fields; kept across redraws
     * @returns {number} how many panels are shown
     */
    function render(panels, stateUpdated = true, fields) {
        const list = Array.isArray(panels) ? panels : [];
        if (Array.isArray(fields)) {
            panelFields = fields;
        }
        shown = { panels: list, stateUpdated };
        for (const node of Array.from(root.querySelectorAll(`.${PANEL_CLASS}`))) {
            node.remove();
        }

        for (const [index, panel] of list.entries()) {
            const node = placeable(renderStatePanel(doc, panel, panelFields), panel, index);
            // Every panel carries the mark, because the mark is about the floor
            // and not about one panel's data: whichever panel the user reads, the
            // values in it are the previous floor's.
            if (stateUpdated === false) {
                node.querySelector(`.${PANEL_HEADER_CLASS}`)?.append(staleMark(doc));
            }
            attachEditors(node, panel);
            root.append(node);
        }

        // An open editor is a form, so the panel stops being decoration: it has
        // to be reachable by assistive technology while someone is typing in it.
        root.setAttribute('aria-hidden', editing || editingProse ? 'false' : 'true');
        applyOpenState();
        return list.length;
    }

    /**
     * Whether a queried node is an element of this panel's document.
     *
     * The constructor is taken from the document rather than a global: the panel
     * is built for whatever document it was handed, and a global constructor is
     * not guaranteed to be the one that made these nodes.
     *
     * @param {unknown} node
     * @returns {node is HTMLElement}
     */
    function isElement(node) {
        const ctor = doc.defaultView?.HTMLElement ?? globalThis.HTMLElement;
        return typeof ctor === 'function' && node instanceof ctor;
    }

    /**
     * Give every editable field of one panel its pencil and, when it is the open
     * one, its editor.
     *
     * A field whose values are pictures is left alone: the pictures are what the
     * state selected, and rewriting the selection through a text box would be a
     * worse way to say the same thing than the state machine.
     *
     * @param {HTMLElement} panelNode
     * @param {any} panel
     */
    function attachEditors(panelNode, panel) {
        /** @type {StatePanelFieldDto[]} */
        const fields = Array.isArray(panel?.fields) ? panel.fields : [];
        if (onWriteState) {
            for (const fieldNode of panelNode.querySelectorAll(`.${FIELD_CLASS}`)) {
                if (!isElement(fieldNode)) {
                    continue;
                }
                const key = String(fieldNode.dataset.key ?? '');
                if (!key) {
                    continue;
                }
                const field = fields.find((candidate) => String(candidate?.key ?? '') === key);
                if ((field?.body?.kind ?? 'text') === 'images') {
                    continue;
                }
                const current = Array.isArray(field?.body?.values)
                    ? field.body.values.map((value) => String(value))
                    : [];
                fieldNode.append(editButton(key, current));
                if (editing?.key === key) {
                    fieldNode.append(fieldEditor(key));
                }
            }
        }

        // The prose block is offered a writer of its own: it is the text that is
        // not state, so what a person writes there is theirs to write, and the
        // panel is where they see it.
        const prosePath = String(panel?.prose?.path ?? '').trim();
        if (!onWriteProse || !prosePath) {
            return;
        }
        const proseNode = panelNode.querySelector(`.${PROSE_CLASS}`);
        if (!isElement(proseNode)) {
            return;
        }
        proseNode.append(proseEditButton(prosePath, String(panel?.prose?.text ?? '')));
        if (editingProse?.path === prosePath) {
            proseNode.append(proseEditor(prosePath));
        }
    }

    /** @param {string} key @param {string[]} values @returns {HTMLElement} */
    function editButton(key, values) {
        const button = doc.createElement('button');
        button.className = `${FIELD_EDIT_CLASS} fa-solid fa-pen`;
        button.type = 'button';
        button.title = '编辑这个字段';
        button.setAttribute('aria-label', `编辑 ${key}`);
        button.addEventListener('click', () => {
            editing = { key, text: values.join('\n') };
            render(shown.panels, shown.stateUpdated);
        });
        return button;
    }

    /** @param {string} key @returns {HTMLElement} */
    function fieldEditor(key) {
        const editor = element(doc, 'div', FIELD_EDITOR_CLASS);
        const input = doc.createElement('textarea');
        input.className = FIELD_EDITOR_INPUT_CLASS;
        input.rows = 3;
        input.value = editing?.text ?? '';
        input.setAttribute('aria-label', `${key} 的值，一行一条`);
        input.addEventListener('input', () => {
            if (editing && editing.key === key) {
                editing.text = input.value;
            }
        });

        const actions = element(doc, 'div', FIELD_EDITOR_ACTIONS_CLASS);
        actions.append(
            editorAction('保存', () => submit({ fields: [{ key, value: editorValues(key) }] })),
            editorAction('清空', () => submit({ fields: [{ key, value: [] }] })),
            editorAction('删除', () => submit({ remove: [key] })),
            editorAction('取消', () => {
                editing = null;
                render(shown.panels, shown.stateUpdated);
            }),
        );

        editor.append(input, actions);
        return editor;
    }

    /** @param {string} label @param {() => void} onClick @returns {HTMLElement} */
    function editorAction(label, onClick) {
        const button = doc.createElement('button');
        button.className = FIELD_EDITOR_ACTION_CLASS;
        button.textContent = label;
        button.type = 'button';
        button.addEventListener('click', onClick);
        return button;
    }

    /**
     * The pencil on a prose block.
     *
     * Prose is not a field, so it has no key and no three states: there is the
     * file, and there is what it says. Clearing writes an empty file, which is
     * the same thing to everyone who reads it.
     *
     * @param {string} path
     * @param {string} text
     * @returns {HTMLElement}
     */
    function proseEditButton(path, text) {
        const button = doc.createElement('button');
        button.className = `${PROSE_EDIT_CLASS} fa-solid fa-pen`;
        button.type = 'button';
        button.title = '编辑这段文字';
        button.setAttribute('aria-label', `编辑 ${path}`);
        button.addEventListener('click', () => {
            editingProse = { path, text };
            render(shown.panels, shown.stateUpdated);
        });
        return button;
    }

    /**
     * @param {string} path
     * @returns {HTMLElement}
     */
    function proseEditor(path) {
        const editor = element(doc, 'div', PROSE_EDITOR_CLASS);
        const input = doc.createElement('textarea');
        input.className = PROSE_EDITOR_INPUT_CLASS;
        input.rows = 6;
        input.value = editingProse?.text ?? '';
        input.setAttribute('aria-label', `${path} 的内容`);
        input.addEventListener('input', () => {
            if (editingProse && editingProse.path === path) {
                editingProse.text = input.value;
            }
        });

        const actions = element(doc, 'div', PROSE_EDITOR_ACTIONS_CLASS);
        actions.append(
            proseAction('保存', () => void submitProse({ path, text: input.value })),
            proseAction('清空', () => void submitProse({ path, text: '' })),
            proseAction('取消', () => {
                editingProse = null;
                render(shown.panels, shown.stateUpdated);
            }),
        );

        editor.append(input, actions);
        return editor;
    }

    /** @param {string} label @param {() => void} onClick @returns {HTMLElement} */
    function proseAction(label, onClick) {
        const button = doc.createElement('button');
        button.className = PROSE_EDITOR_ACTION_CLASS;
        button.textContent = label;
        button.type = 'button';
        button.addEventListener('click', onClick);
        return button;
    }

    /**
     * Send one prose edit.
     *
     * A refusal keeps the editor open and says why beside it, exactly as a
     * refused field edit does: the host reports what was wrong, and closing the
     * editor would leave the user with a panel that looks like it ignored them.
     *
     * @param {{ path: string; text: string }} request
     */
    async function submitProse(request) {
        if (!onWriteProse) {
            return;
        }
        try {
            await onWriteProse(request);
            // The host refreshes the panel after a successful write, so the
            // editor is closed before that render rather than after it.
            editingProse = null;
        } catch (error) {
            const editor = root.querySelector(`.${PROSE_EDITOR_CLASS}`);
            editor?.querySelector(`.${PROSE_EDITOR_ERROR_CLASS}`)?.remove();
            const message = element(
                doc,
                'div',
                PROSE_EDITOR_ERROR_CLASS,
                error instanceof Error ? error.message : String(error),
            );
            editor?.append(message);
        }
    }

    /**
     * What the editor currently holds, one value per non-empty line.
     *
     * A blank line is dropped rather than stored: a value is a line of text, and
     * an empty one would be indistinguishable from a clear.
     *
     * @param {string} key
     * @returns {string[]}
     */
    function editorValues(key) {
        const text = editing && editing.key === key ? editing.text : '';
        return text
            .split('\n')
            .map((line) => line.trim())
            .filter((line) => line.length > 0);
    }

    /**
     * Hand one edit to the host.
     *
     * A refused edit keeps the editor open and says why beside it: the host
     * reports every problem at once, and closing the editor would leave the user
     * with a panel that looks like it ignored them.
     *
     * @param {{ fields?: Array<{ key: string; value: string[] }>; remove?: string[] }} request
     */
    async function submit(request) {
        if (!onWriteState) {
            return;
        }
        try {
            await onWriteState(request);
            // The host refreshes the panel after a successful write, so the
            // editor is closed before that render rather than after it.
            editing = null;
        } catch (error) {
            const editor = root.querySelector(`.${FIELD_EDITOR_CLASS}`);
            editor?.querySelector(`.${FIELD_EDITOR_ERROR_CLASS}`)?.remove();
            const message = element(
                doc,
                'div',
                FIELD_EDITOR_ERROR_CLASS,
                error instanceof Error ? error.message : String(error),
            );
            editor?.append(message);
        }
    }

    function clear() {
        for (const node of Array.from(root.querySelectorAll(`.${PANEL_CLASS}`))) {
            node.remove();
        }
        editing = null;
        shown = { panels: [], stateUpdated: true };
    }

    /**
     * What a panel is called in the stored view.
     *
     * A declaration may name its panels; one that does not is known by its
     * title, which is also what a v1 view recorded, so those names carry over.
     *
     * @param {any} panel
     * @param {number} index
     * @returns {string}
     */
    function panelId(panel, index) {
        const named = String(panel?.id ?? '').trim();
        return named || String(panel?.title ?? '').trim() || `panel-${index}`;
    }

    /**
     * The room a panel may be placed in, and the columns beside the chat.
     *
     * A fresh panel lands beside the conversation rather than on it, which needs
     * to know how much room the conversation leaves — measured here, where the
     * document is, because the arithmetic in `state-panel-view.js` is answerable
     * without one.
     *
     * @returns {{ width: number; height: number; gutterLeft?: number; gutterRight?: number }}
     */
    function roomOf() {
        const rect = root.getBoundingClientRect();
        const room = { width: rect.width, height: rect.height };
        const chatRect = doc.getElementById('sheld')?.getBoundingClientRect?.() ?? null;
        if (!chatRect || !chatRect.width || !rect.width) {
            return room;
        }
        return {
            ...room,
            gutterLeft: chatRect.left - rect.left,
            gutterRight: rect.right - chatRect.right,
        };
    }

    /**
     * Where a panel goes: what was stored, or the next free cascade slot.
     *
     * A remembered fold carries no geometry (`w: 0`), and a v1 fold carries
     * nothing else either, so those panels land at their default place.
     */
    /**
 * @param {string} id
 * @param {number} index
 * @returns {any}
 */
function rectOf(id, index) {
        const stored = view.panels[id];
        return stored && stored.w > 0
            ? clampPanelRect(stored, roomOf())
            : defaultPanelRect(index, roomOf());
    }

    /** @param {HTMLElement} node @param {any} rect */
    function applyRect(node, rect) {
        node.style.left = `${rect.x}px`;
        node.style.top = `${rect.y}px`;
        node.style.width = `${rect.w}px`;
        node.style.height = `${rect.h}px`;
    }

    /**
 * @param {string} id
 * @param {any} rect
 */
function rememberRect(id, rect) {
        view.panels[id] = rect;
        writeStatePanelView(storage, view);
    }

    /** @param {string} id @returns {boolean} */
    function isFolded(id) {
        return view.panels[id]?.folded === true;
    }

    /** @param {string} id @param {boolean} folded */
    function setFolded(id, folded) {
        const stored = view.panels[id] ?? { x: 0, y: 0, w: 0, h: 0 };
        view.panels[id] = { ...stored, folded };
        writeStatePanelView(storage, view);
    }

    /**
     * A panel that folds to its header when that header is clicked.
     *
     * @param {HTMLElement} node
     * @param {string} id
     * @returns {HTMLElement}
     */
    function foldable(node, id) {
        node.classList.toggle(PANEL_FOLDED_CLASS, isFolded(id));
        node.querySelector(`.${PANEL_HEADER_CLASS}`)?.addEventListener('click', () => {
            setFolded(id, !isFolded(id));
            node.classList.toggle(PANEL_FOLDED_CLASS, isFolded(id));
        });
        return node;
    }

    /**
     * Move a panel by its header.
     *
     * Everything geometric is measured once, when the drag starts: the move
     * itself only shifts the node by a transform, and the place is committed on
     * release. Re-measuring on every pointer move is the one thing this must not
     * do — the chat underneath is still laying itself out.
     *
     * @param {HTMLElement} node
     * @param {string} id
     * @param {Element | null} handle
     */
    function startDrag(node, id, handle) {
        if (!handle) {
            return;
        }
        handle.addEventListener('pointerdown', (event) => {
            const down = /** @type {PointerEvent} */ (event);
            if (typeof down.button === 'number' && down.button !== 0) {
                return;
            }
            down.preventDefault();
            const room = roomOf();
            const start = rectOf(id, 0);
            const startX = Number(down.clientX) || 0;
            const startY = Number(down.clientY) || 0;
            let latest = start;
            let moved = false;

            /** @param {PointerEvent} moveEvent */
            const onMove = (moveEvent) => {
                const dx = (Number(moveEvent.clientX) || 0) - startX;
                const dy = (Number(moveEvent.clientY) || 0) - startY;
                if (!moved && Math.abs(dx) + Math.abs(dy) < 3) {
                    // A click that wobbles is still a click: folding needs it.
                    return;
                }
                moved = true;
                latest = clampPanelRect({ ...start, x: start.x + dx, y: start.y + dy }, room);
                node.style.transform = `translate(${latest.x - start.x}px, ${latest.y - start.y}px)`;
            };
            const onEnd = () => {
                doc.removeEventListener('pointermove', onMove);
                doc.removeEventListener('pointerup', onEnd);
                doc.removeEventListener('pointercancel', onEnd);
                node.style.transform = '';
                if (moved) {
                    applyRect(node, latest);
                    rememberRect(id, latest);
                }
            };
            doc.addEventListener('pointermove', onMove);
            doc.addEventListener('pointerup', onEnd);
            doc.addEventListener('pointercancel', onEnd);
        });
    }

    /**
     * Size a panel by its corner.
     *
     * @param {HTMLElement} node
     * @param {string} id
     * @param {Element} handle
     */
    function startResize(node, id, handle) {
        handle.addEventListener('pointerdown', (event) => {
            const down = /** @type {PointerEvent} */ (event);
            if (typeof down.button === 'number' && down.button !== 0) {
                return;
            }
            // The corner sits inside the header, so a resize must not also drag.
            down.preventDefault();
            down.stopPropagation();
            const room = roomOf();
            const start = rectOf(id, 0);
            const startX = Number(down.clientX) || 0;
            const startY = Number(down.clientY) || 0;
            let latest = start;

            /** @param {PointerEvent} moveEvent */
            const onMove = (moveEvent) => {
                latest = clampPanelRect({
                    ...start,
                    w: start.w + ((Number(moveEvent.clientX) || 0) - startX),
                    h: start.h + ((Number(moveEvent.clientY) || 0) - startY),
                }, room);
                node.style.width = `${latest.w}px`;
                node.style.height = `${latest.h}px`;
            };
            const onEnd = () => {
                doc.removeEventListener('pointermove', onMove);
                doc.removeEventListener('pointerup', onEnd);
                doc.removeEventListener('pointercancel', onEnd);
                rememberRect(id, latest);
            };
            doc.addEventListener('pointermove', onMove);
            doc.addEventListener('pointerup', onEnd);
            doc.addEventListener('pointercancel', onEnd);
        });
    }

    /**
     * A panel as a window: placed, draggable, resizable, foldable.
     *
     * @param {HTMLElement} node
     * @param {any} panel
     * @param {number} index
     * @returns {HTMLElement}
     */
    function placeable(node, panel, index) {
        const id = panelId(panel, index);
        node.dataset.panelId = id;
        applyRect(node, rectOf(id, index));

        const resize = element(doc, 'div', PANEL_RESIZE_CLASS);
        node.append(resize);
        startDrag(node, id, node.querySelector(`.${PANEL_HEADER_CLASS}`));
        startResize(node, id, resize);
        return foldable(node, id);
    }

    /** @param {boolean} open */
    function setOpen(open) {
        view.open = open === true;
        writeStatePanelView(storage, view);
        applyOpenState();
    }

    function applyOpenState() {
        root.classList.toggle(ROOT_OPEN_CLASS, view.open);
        const label = view.open ? '收起状态面板' : '展开状态面板';
        launcher.title = label;
        launcher.setAttribute('aria-label', label);
        launcher.classList.toggle(`${LAUNCHER_CLASS}--on`, view.open);
    }

    applyOpenState();

    /**
     * A window resize changes the room, so every panel has to be pulled back
     * inside it: a place that is now off-screen would otherwise stay there.
     */
    const onWindowResize = () => {
        for (const node of Array.from(root.querySelectorAll(`.${PANEL_CLASS}`))) {
            const panelNode = /** @type {HTMLElement} */ (node);
            const id = String(panelNode.dataset.panelId ?? '');
            const stored = view.panels[id];
            if (id && stored && stored.w > 0) {
                applyRect(panelNode, clampPanelRect(stored, roomOf()));
            }
        }
    };
    doc.defaultView?.addEventListener('resize', onWindowResize);

    function destroy() {
        clear();
        themeStyle.remove();
        doc.defaultView?.removeEventListener('resize', onWindowResize);
        root.remove();
    }

    return {
        element: root,
        render,
        clear,
        setTheme,
        destroy,
        /** @param {HTMLElement | null | undefined} nextMount */
        setMount(nextMount) {
            nextMount?.append(root);
        },
    };
}
