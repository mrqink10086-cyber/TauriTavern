// @ts-check

/**
 * How the panels are arranged, as the user left them.
 *
 * A view is a local preference, not state: it says where each floating panel
 * sits, how big it is, whether it is folded, and whether the layer is open at
 * all. Losing it costs nothing, which is why it lives in the browser's storage
 * rather than in the declaration — the same declaration is opened on a phone and
 * on a wide screen, and the room for a panel is different on each.
 *
 * The DOM module owns the elements; this module owns the arithmetic and the one
 * storage read, so "is this a usable place for a panel" is answerable without a
 * document.
 */

export const STATE_PANEL_VIEW_KEY = 'tauritavern.state-panel.view';

/** The format this build writes. A v1 view (two rails) is migrated on read. */
const STATE_PANEL_VIEW_VERSION = 2;

/** A panel never gets smaller than this: below it there is nothing to read. */
export const MIN_PANEL_WIDTH = 220;
export const MIN_PANEL_HEIGHT = 120;

/** Nor bigger than this share of the room, so the chat is never fully covered. */
export const MAX_PANEL_SHARE = 0.92;

/** How much room is kept between a panel and the edge it is pushed against. */
export const EDGE_MARGIN = 8;

/** How far from the corner a panel lands when it has no stored place. */
const DEFAULT_INSET = 24;

/** How far each further panel is offset, so a stack stays readable. */
const CASCADE_STEP = 28;

/** The size a panel opens at before it has been resized. */
const DEFAULT_PANEL_WIDTH = 360;
const DEFAULT_PANEL_HEIGHT = 420;

/**
 * @typedef {{ x: number; y: number; w: number; h: number; folded: boolean }} PanelRect
 *
 * @typedef {{
 *   version: number;
 *   open: boolean;
 *   panels: Record<string, PanelRect>;
 * }} StatePanelView
 */

/** @returns {StatePanelView} */
export function emptyStatePanelView() {
    // Open by default: a panel the user has to find before seeing anything is
    // not the "there is a UI from the start" this is for.
    return { version: STATE_PANEL_VIEW_VERSION, open: true, panels: {} };
}

/**
 * The sizes a panel may take in the room it has.
 *
 * The ceiling is a share of the room, so a panel can be made large without ever
 * covering everything. An unmeasurable room — a test double, or a hidden layer —
 * has no share to take, so the absolute ceiling applies instead.
 *
 * @param {{ width?: number; height?: number } | null | undefined} room
 * @returns {{ minW: number; minH: number; maxW: number; maxH: number }}
 */
export function panelBounds(room) {
    const width = Number(room?.width);
    const height = Number(room?.height);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
        return {
            minW: MIN_PANEL_WIDTH,
            minH: MIN_PANEL_HEIGHT,
            maxW: DEFAULT_PANEL_WIDTH * 2,
            maxH: DEFAULT_PANEL_HEIGHT * 2,
        };
    }
    return {
        minW: Math.min(MIN_PANEL_WIDTH, width),
        minH: Math.min(MIN_PANEL_HEIGHT, height),
        maxW: Math.max(MIN_PANEL_WIDTH, width * MAX_PANEL_SHARE),
        maxH: Math.max(MIN_PANEL_HEIGHT, height * MAX_PANEL_SHARE),
    };
}

/**
 * Bring a panel's place and size inside the room it has to live in.
 *
 * A stored rect is a memory of another window: a panel dragged to the far right
 * of a wide screen has no business being half off a narrow one, so every read of
 * it comes through here rather than being trusted.
 *
 * @param {Partial<PanelRect> | null | undefined} rect
 * @param {{ width?: number; height?: number } | null | undefined} room
 * @returns {PanelRect}
 */
export function clampPanelRect(rect, room) {
    const bounds = panelBounds(room);
    const w = Math.min(bounds.maxW, Math.max(bounds.minW, Number(rect?.w) || bounds.minW));
    const h = Math.min(bounds.maxH, Math.max(bounds.minH, Number(rect?.h) || bounds.minH));
    const roomWidth = Number(room?.width);
    const roomHeight = Number(room?.height);
    const maxX = Number.isFinite(roomWidth)
        ? Math.max(EDGE_MARGIN, roomWidth - w - EDGE_MARGIN)
        : Number.MAX_SAFE_INTEGER;
    const maxY = Number.isFinite(roomHeight)
        ? Math.max(EDGE_MARGIN, roomHeight - h - EDGE_MARGIN)
        : Number.MAX_SAFE_INTEGER;
    return {
        x: Math.min(maxX, Math.max(EDGE_MARGIN, Number(rect?.x) || 0)),
        y: Math.min(maxY, Math.max(EDGE_MARGIN, Number(rect?.y) || 0)),
        w,
        h,
        folded: rect?.folded === true,
    };
}

/**
 * The column beside the chat, when there is room for the panel there.
 *
 * The conversation is a centred column, so a wide window has space left over on
 * either side and a panel belongs in it: state is read next to the transcript,
 * not on top of it. A narrow window leaves none, and the caller falls back to
 * the corner. The hint is optional — a room that never measured a gutter is
 * simply a room with no gutter.
 *
 * @param {{ width?: number; gutterLeft?: number; gutterRight?: number } | null | undefined} room
 * @param {number} width
 * @returns {number | null}
 */
function gutterColumn(room, width) {
    const left = Number(room?.gutterLeft);
    const right = Number(room?.gutterRight);
    const need = width + (2 * EDGE_MARGIN);
    if (Number.isFinite(left) && left >= need) {
        return EDGE_MARGIN;
    }
    if (Number.isFinite(right) && right >= need) {
        return Math.max(EDGE_MARGIN, Number(room?.width) - width - EDGE_MARGIN);
    }
    return null;
}

/**
 * Where a panel goes when the user has not placed it.
 *
 * Beside the chat when the window has a column to spare, and otherwise
 * cascading from the corner so several panels stay readable; the cascade is
 * clamped rather than allowed to walk off the screen.
 *
 * @param {number} index
 * @param {{ width?: number; height?: number } | null | undefined} room
 * @returns {PanelRect}
 */
export function defaultPanelRect(index, room) {
    const bounds = panelBounds(room);
    const w = Math.min(DEFAULT_PANEL_WIDTH, bounds.maxW);
    const h = Math.min(DEFAULT_PANEL_HEIGHT, bounds.maxH);
    const step = Math.max(0, Number(index) || 0);
    const gutter = gutterColumn(room, w);
    if (gutter !== null) {
        // Down the column rather than across it: the room there is the width of
        // one panel, so a further panel stacks below the one before it.
        const roomHeight = Number(room?.height);
        const stacked = DEFAULT_INSET + (step * CASCADE_STEP);
        const y = Number.isFinite(roomHeight)
            ? Math.min(stacked, Math.max(EDGE_MARGIN, roomHeight - h - EDGE_MARGIN))
            : stacked;
        return clampPanelRect({ x: gutter, y, w, h }, room);
    }

    const offset = DEFAULT_INSET + (step * CASCADE_STEP);
    return clampPanelRect({ x: offset, y: offset, w, h }, room);
}

/**
 * No place at all, which is what a panel that was only ever folded carries.
 *
 * @param {boolean} folded
 * @returns {PanelRect}
 */
function unfoldedPlace(folded) {
    return { x: 0, y: 0, w: 0, h: 0, folded };
}

/**
 * @param {any} value
 * @returns {PanelRect | null}
 */
function readRect(value) {
    if (!value || typeof value !== 'object') {
        return null;
    }
    const folded = /** @type {any} */ (value).folded === true;
    const x = Number(/** @type {any} */ (value).x);
    const y = Number(/** @type {any} */ (value).y);
    const w = Number(/** @type {any} */ (value).w);
    const h = Number(/** @type {any} */ (value).h);
    if (![x, y, w, h].every(Number.isFinite) || w < 0 || h < 0) {
        // A place that is not numbers is not a place — but a fold still is, and
        // throwing the fold away with the numbers would forget a real choice.
        return folded ? unfoldedPlace(true) : null;
    }
    // `w: 0` means "folded, never placed": the fold is remembered, the place is
    // left to the cascade.
    return w > 0 && h > 0 ? { x, y, w, h, folded } : unfoldedPlace(folded);
}

/**
 * A v1 view described two rails; the panels float now.
 *
 * The widths have nowhere to go — a rail's width is not a window's place — so
 * only what still means something survives: which panels were folded, and
 * whether the layer was tucked away. A v1 view named panels by title, and a
 * panel with no id of its own is still named by its title, so those names carry
 * over as they are.
 *
 * @param {any} parsed
 * @param {StatePanelView} view
 */
function migrateRailsToFloats(parsed, view) {
    const folded = Array.isArray(parsed.folded) ? parsed.folded : [];
    for (const title of folded) {
        const id = String(title);
        if (id) {
            // No geometry: the panel lands at its default place on first sight.
            view.panels[id] = { x: 0, y: 0, w: 0, h: 0, folded: true };
        }
    }
    const tucked = parsed.tucked;
    if (tucked && typeof tucked === 'object') {
        view.open = tucked.left !== true || tucked.right !== true;
    }
    return view;
}

/**
 * Read the stored view, or start over.
 *
 * A preference is not state, so damage here is repaired rather than reported:
 * anything unreadable, or of the wrong shape, is dropped field by field and the
 * panels open where they land. What is never done is trusting a stored value
 * that could break the layout — a place is only kept when it is usable numbers.
 *
 * @param {Storage | null | undefined} storage
 * @returns {StatePanelView}
 */
export function readStatePanelView(storage) {
    const view = emptyStatePanelView();
    let raw = null;
    try {
        raw = storage?.getItem(STATE_PANEL_VIEW_KEY) ?? null;
    } catch (error) {
        // Private modes and sandboxed documents can refuse storage outright.
        console.warn('[state-panel] the stored view is unreadable; using defaults', error);
        return view;
    }
    if (!raw) {
        return view;
    }

    let parsed = null;
    try {
        parsed = JSON.parse(raw);
    } catch (error) {
        console.warn('[state-panel] the stored view is not JSON; using defaults', error);
        return view;
    }
    if (!parsed || typeof parsed !== 'object') {
        return view;
    }
    // A v1 view carries no `version` at all — it described two rails by their
    // widths — so what marks it is the absence of the key that replaced them.
    if (parsed.panels === undefined && (parsed.widths !== undefined || parsed.folded !== undefined)) {
        return migrateRailsToFloats(parsed, view);
    }

    view.open = parsed.open !== false;
    const panels = parsed.panels;
    if (panels && typeof panels === 'object') {
        for (const [id, rect] of Object.entries(panels)) {
            const placed = readRect(rect);
            if (id && placed) {
                view.panels[id] = placed;
            }
        }
    }
    return view;
}

/**
 * Store the view.
 *
 * Best effort on purpose: a browser that refuses storage costs the user their
 * arrangement, not the panels, so a failure is said out loud and then dropped.
 *
 * @param {Storage | null | undefined} storage
 * @param {StatePanelView} view
 */
export function writeStatePanelView(storage, view) {
    try {
        storage?.setItem(STATE_PANEL_VIEW_KEY, JSON.stringify(view));
    } catch (error) {
        console.warn('[state-panel] the view could not be stored; it will not survive a reload', error);
    }
}
