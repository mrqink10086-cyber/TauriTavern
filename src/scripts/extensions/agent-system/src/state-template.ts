/**
 * The panel template, as the editor stores it.
 *
 * Level 2 of the panel's style layers: a panel may say what its own markup looks
 * like instead of accepting one row per field. The text is compiled **once, at
 * save time**, into a tree; the renderer only walks that tree — it creates
 * elements, sets attributes and writes text — so no markup is ever parsed while
 * a chat is on screen.
 *
 * The vocabulary, all of it spelled out in an element's text or in one of its
 * attributes:
 *
 * - `{{value 环境/日期}}` — the field's values, inserted as text.
 * - `{{#if 环境/天气}}…{{else}}…{{/if}}` — a branch on whether the field carries
 *   anything, or on a comparison: `{{#if 主角/体力 gt 30}}`.
 * - `{{#each 角色/*}}…{{/each}}` — the body once per field the pattern matches;
 *   a `{{value}}` naming that same pattern means "this one".
 * - `class="bar bar--{{value 角色/心情}}"` — the same value, inside an attribute.
 *
 * There is deliberately no raw-markup binding. A value arrives at render time,
 * so inserting it as markup would mean sanitizing it at render time — exactly
 * the render-time parsing this design forbids. `{{raw …}}` is therefore refused
 * by name instead of being quietly ignored.
 *
 * The tag and attribute lists are mirrored in the domain (`state_template.rs`),
 * which checks the compiled tree again on the way in: the editor is where HTML
 * is parsed, the domain is where the result is trusted.
 *
 * Reading a stored document back needs the other direction: `printTemplate`
 * turns the tree into the text a user edits. Saving and reopening is therefore a
 * round trip, and both halves are tested against each other.
 */

/**
 * The elements a template may not use. Mirrors `FORBIDDEN_TAGS`.
 *
 * Everything else is allowed, the whole SVG vocabulary included: a panel that
 * draws its own icon should not have to wait for this list to grow. What is
 * refused is markup that is a second document or a second program — a nested
 * document (`iframe`, `object`), a second stylesheet (`style`, `link`), a second
 * script (`script`) — because a panel already has a stylesheet layer and a place
 * to put code, and two of either is one too many to reason about.
 */
export const FORBIDDEN_TAGS: readonly string[] = Object.freeze([
    'script', 'style', 'link', 'meta', 'base', 'iframe', 'frame', 'frameset',
    'object', 'embed', 'applet', 'template', 'slot', 'noscript',
]);

/**
 * The comparison a `{{#if}}` may carry. Mirrors `TEMPLATE_OPS`.
 *
 * Without one, the branch asks whether the field carries anything at all — the
 * question most templates are asking.
 */
export const TEMPLATE_OPS: readonly string[] = Object.freeze([
    'eq', 'ne', 'in', 'not_in', 'contains', 'gt', 'gte', 'lt', 'lte', 'exists', 'missing',
]);

/** Ops whose right-hand side is left out: `{{#if 环境/天气 exists}}`. */
const UNARY_OPS = new Set(['exists', 'missing']);

/** `on*` is matched by shape: the set of events belongs to the browser, not to us. */
export function isEventAttribute(name: string): boolean {
    return /^on[a-z]+$/u.test(name);
}

/** One piece of an attribute's value: literal text, or the marker that follows it. */
export type TemplateAttributePart =
    | { kind: 'text'; text: string }
    | { kind: 'value'; key: string };

export type TemplateElementNode = {
    kind: 'element';
    tag: string;
    /** Attributes whose value is literal. A value-carrying one lives in `boundAttrs`. */
    attrs: Record<string, string>;
    boundAttrs: Record<string, TemplateAttributePart[]>;
    children: CompiledTemplateNode[];
};

export type TemplateIfNode = {
    kind: 'if';
    key: string;
    /** How the field is compared; empty means "carries anything". */
    op: string;
    /** The right-hand side, empty for the ops that take none. */
    value: string;
    then: CompiledTemplateNode[];
    else: CompiledTemplateNode[];
};

export type TemplateEachNode = {
    kind: 'each';
    /** A key pattern: the body renders once per field it matches. */
    key: string;
    body: CompiledTemplateNode[];
};

export type CompiledTemplateNode =
    | TemplateElementNode
    | { kind: 'text'; text: string }
    | { kind: 'value'; key: string }
    | TemplateIfNode
    | TemplateEachNode;

export type CompiledTemplate = { nodes: CompiledTemplateNode[] };

export type TemplateIssue = {
    path: string;
    message: string;
};

/** One piece of a text run: literal text, or the marker that follows it. */
type TextPart =
    | { kind: 'text'; text: string }
    | { kind: 'value'; key: string }
    | { kind: 'open'; key: string; op: string; value: string }
    | { kind: 'each'; key: string }
    | { kind: 'else' }
    | { kind: 'close'; which: 'if' | 'each' };

/**
 * Compile a template for storage.
 *
 * An unusable template compiles to `null`: storing half of one is worse than
 * storing none, and the editor refuses to save while an issue is listed.
 */
export function compileTemplate(
    source: string,
    where = 'template',
): { markup: CompiledTemplate | null; issues: TemplateIssue[] } {
    const text = String(source ?? '');
    if (!text.trim()) {
        return { markup: null, issues: [] };
    }

    const issues: TemplateIssue[] = [];
    const doc = new DOMParser().parseFromString(text, 'text/html');

    // Anything the parser moved into the head (`<style>`, `<title>`, `<link>`)
    // is content the user wrote and this pipeline would otherwise drop without
    // a word, so it is refused by name.
    for (const stray of Array.from(doc.head?.childNodes ?? [])) {
        if (stray.nodeType === 1) {
            const tag = (stray as Element).tagName.toLowerCase();
            issues.push({ path: where, message: `\`<${tag}>\` cannot be used in a panel template` });
        }
    }

    const nodes = buildNodes(tokenize(doc.body), issues, false);
    if (issues.length > 0) {
        return { markup: null, issues: issues.map((issue) => ({ ...issue, path: where })) };
    }
    return { markup: { nodes }, issues: [] };
}

/** The markup as the template text a user edits. */
export { printTemplate } from './state-template-print';

/** The children of one element, as text runs and elements. */
function tokenize(parent: Node | null): Array<{ kind: 'text'; text: string } | { kind: 'element'; node: Element }> {
    const tokens: Array<{ kind: 'text'; text: string } | { kind: 'element'; node: Element }> = [];
    for (const child of Array.from(parent?.childNodes ?? [])) {
        if (child.nodeType === 3) {
            tokens.push({ kind: 'text', text: child.textContent ?? '' });
        } else if (child.nodeType === 1) {
            tokens.push({ kind: 'element', node: child as Element });
        }
        // Comments and anything else render as nothing, so they are dropped.
    }
    return tokens;
}

/** A branch being filled: `{{#if}}` has two, `{{#each}}` one. */
type BlockFrame =
    | { node: TemplateIfNode; branch: 'then' | 'else' }
    | { node: TemplateEachNode };

function buildNodes(
    tokens: ReadonlyArray<{ kind: 'text'; text: string } | { kind: 'element'; node: Element }>,
    issues: TemplateIssue[],
    inEach: boolean,
): CompiledTemplateNode[] {
    const root: CompiledTemplateNode[] = [];
    const stack: BlockFrame[] = [];
    const target = (): CompiledTemplateNode[] => {
        const frame = stack[stack.length - 1];
        if (!frame) {
            return root;
        }
        return 'branch' in frame
            ? (frame.branch === 'then' ? frame.node.then : frame.node.else)
            : frame.node.body;
    };

    // Inside a loop's body a binding may name the loop's pattern — that is how a
    // row says "this one" — so the flag follows the stack, not just the call.
    const insideEach = (): boolean => inEach || stack.some((frame) => !('branch' in frame));

    for (const token of tokens) {
        if (token.kind === 'element') {
            target().push(compileElement(token.node, issues, insideEach()));
            continue;
        }
        for (const part of splitBindings(token.text, issues, insideEach())) {
            if (part.kind === 'text') {
                target().push({ kind: 'text', text: part.text });
                continue;
            }
            if (part.kind === 'value') {
                target().push({ kind: 'value', key: part.key });
                continue;
            }
            if (part.kind === 'open') {
                const node: TemplateIfNode = {
                    kind: 'if',
                    key: part.key,
                    op: part.op,
                    value: part.value,
                    then: [],
                    else: [],
                };
                target().push(node);
                stack.push({ node, branch: 'then' });
                continue;
            }
            if (part.kind === 'each') {
                const node: TemplateEachNode = { kind: 'each', key: part.key, body: [] };
                target().push(node);
                stack.push({ node });
                continue;
            }
            if (part.kind === 'else') {
                const frame = stack[stack.length - 1];
                if (!frame || !('branch' in frame)) {
                    issues.push({ path: 'template', message: '`{{else}}` has no `{{#if}}` to belong to' });
                    continue;
                }
                if (frame.branch === 'else') {
                    issues.push({ path: 'template', message: 'an `{{#if}}` takes one `{{else}}`' });
                    continue;
                }
                frame.branch = 'else';
                continue;
            }
            const frame = stack.pop();
            if (!frame) {
                issues.push({ path: 'template', message: `\`{{/${part.which}}}\` closes nothing` });
                continue;
            }
            const opened = 'branch' in frame ? 'if' : 'each';
            if (opened !== part.which) {
                issues.push({
                    path: 'template',
                    message: `\`{{/${part.which}}}\` closes a \`{{#${opened}}}\``,
                });
            }
        }
    }

    for (const frame of stack) {
        const opened = 'branch' in frame ? `if ${frame.node.key}` : `each ${frame.node.key}`;
        issues.push({ path: 'template', message: `\`{{#${opened}}}\` is never closed` });
    }
    return root;
}

function compileElement(el: Element, issues: TemplateIssue[], inEach: boolean): CompiledTemplateNode {
    const tag = el.tagName.toLowerCase();
    if (FORBIDDEN_TAGS.includes(tag)) {
        issues.push({ path: 'template', message: `\`<${tag}>\` cannot be used in a panel template` });
    }

    const attrs: Record<string, string> = {};
    const boundAttrs: Record<string, TemplateAttributePart[]> = {};
    for (const attribute of Array.from(el.attributes)) {
        const name = attribute.name.toLowerCase();
        // A handler is code, so a binding inside one would be building code out
        // of state; the attribute takes what the template literally says.
        if (isEventAttribute(name)) {
            if (attribute.value.includes('{{')) {
                issues.push({
                    path: 'template',
                    message: `a binding cannot be used inside \`${name}\`: a handler is code`,
                });
                continue;
            }
            attrs[name] = attribute.value;
            continue;
        }
        if (!attribute.value.includes('{{')) {
            attrs[name] = attribute.value;
            continue;
        }
        const parts = compileAttributeParts(attribute.value, name, issues, inEach);
        if (parts) {
            boundAttrs[name] = parts;
        }
    }

    return { kind: 'element', tag, attrs, boundAttrs, children: buildNodes(tokenize(el), issues, inEach) };
}

/** Split one attribute's value into literal text and value markers. */
function compileAttributeParts(
    value: string,
    name: string,
    issues: TemplateIssue[],
    inEach: boolean,
): TemplateAttributePart[] | null {
    const parts: TemplateAttributePart[] = [];
    const pattern = /\{\{([^{}]*)\}\}/gu;
    let cursor = 0;
    let match: RegExpExecArray | null = null;

    while ((match = pattern.exec(value)) !== null) {
        if (match.index > cursor) {
            parts.push({ kind: 'text', text: value.slice(cursor, match.index) });
        }
        cursor = match.index + match[0].length;
        const inner = String(match[1] ?? '').trim();
        const [head, ...rest] = inner.split(/\s+/u);
        if (head !== 'value') {
            issues.push({
                path: 'template',
                message: `\`{{${inner}}}\` cannot be used inside \`${name}\`: only a value fits an attribute`,
            });
            return null;
        }
        const key = rest.join(' ').trim();
        const problem = bindingKeyProblem(key, inEach);
        if (problem) {
            issues.push({ path: 'template', message: problem });
            return null;
        }
        parts.push({ kind: 'value', key });
    }

    const tail = value.slice(cursor);
    if (tail.includes('{{')) {
        issues.push({ path: 'template', message: '`{{` is never closed' });
        return null;
    }
    if (tail.length > 0) {
        parts.push({ kind: 'text', text: tail });
    }
    return parts;
}

/** Split one text run into literal text and binding markers. */
function splitBindings(text: string, issues: TemplateIssue[], inEach: boolean): TextPart[] {
    const parts: TextPart[] = [];
    const pattern = /\{\{([^{}]*)\}\}/gu;
    let cursor = 0;
    let match: RegExpExecArray | null = null;

    while ((match = pattern.exec(text)) !== null) {
        if (match.index > cursor) {
            parts.push({ kind: 'text', text: text.slice(cursor, match.index) });
        }
        cursor = match.index + match[0].length;
        const inner = String(match[1] ?? '').trim();
        const [head, ...rest] = inner.split(/\s+/u);

        if (head === 'value') {
            const key = rest.join(' ').trim();
            const problem = bindingKeyProblem(key, inEach);
            if (problem) {
                issues.push({ path: 'template', message: problem });
                continue;
            }
            parts.push({ kind: 'value', key });
            continue;
        }
        if (head === '#if') {
            const condition = parseCondition(rest);
            if (!condition) {
                issues.push({
                    path: 'template',
                    message: `\`{{${inner}}}\` is not a condition: \`{{#if 键}}\` or \`{{#if 键 gt 30}}\``,
                });
                continue;
            }
            parts.push({ kind: 'open', ...condition });
            continue;
        }
        if (head === '#each') {
            const key = rest.join(' ').trim();
            const problem = bindingKeyProblem(key, true);
            if (problem) {
                issues.push({ path: 'template', message: problem });
                continue;
            }
            if (!key.includes('*') && !key.startsWith('/')) {
                issues.push({
                    path: 'template',
                    message: `\`{{#each ${key}}}\` names one field: a loop needs a pattern, such as 角色/*/好感度`,
                });
                continue;
            }
            parts.push({ kind: 'each', key });
            continue;
        }
        if (head === 'else') {
            parts.push({ kind: 'else' });
            continue;
        }
        if (head === '/if' || head === '/each') {
            parts.push({ kind: 'close', which: head === '/if' ? 'if' : 'each' });
            continue;
        }
        if (head === 'raw') {
            issues.push({
                path: 'template',
                message: '`{{raw …}}` is not a binding: a value is always inserted as text',
            });
            continue;
        }
        issues.push({
            path: 'template',
            message: `\`{{${inner}}}\` is not a binding: the vocabulary is \`value\`, \`#if\` and \`#each\``,
        });
    }

    const tail = text.slice(cursor);
    if (tail.includes('{{')) {
        issues.push({ path: 'template', message: '`{{` is never closed' });
    } else if (tail.length > 0) {
        parts.push({ kind: 'text', text: tail });
    }
    return parts;
}

/** `键`, `键 op`, or `键 op 值`. */
function parseCondition(rest: readonly string[]): { key: string; op: string; value: string } | null {
    const words = rest.filter(Boolean);
    if (words.length === 1) {
        const [key] = words;
        return key ? { key, op: '', value: '' } : null;
    }
    if (words.length === 2) {
        const [key, op] = words;
        if (!key || !op || !UNARY_OPS.has(op)) {
            return null;
        }
        return { key, op, value: '' };
    }
    if (words.length === 3) {
        const [key, op, value] = words;
        if (!key || !op || !value || !TEMPLATE_OPS.includes(op) || UNARY_OPS.has(op)) {
            return null;
        }
        return { key, op, value };
    }
    return null;
}

/**
 * What a binding's key may not be, in the editor's own terms.
 *
 * Whether the key is *declared* is the domain's answer, because that needs the
 * pattern matcher; what can be said here is only whether a key or a pattern is
 * allowed where the binding sits.
 */
function bindingKeyProblem(key: string, allowPattern: boolean): string | null {
    if (!key) {
        return 'a binding names no field';
    }
    if (!allowPattern && (key.includes('*') || key.startsWith('/'))) {
        return `\`${key}\` is a pattern: a binding names one field, such as 环境/日期`;
    }
    return null;
}

