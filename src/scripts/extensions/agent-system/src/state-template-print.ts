/**
 * A compiled template, back as the text a user edits.
 *
 * The other half of the round trip the compiler starts: saving turns the text
 * into a tree, reopening prints that tree back. Both halves are tested against
 * each other, so what a user sees after a save is what they wrote.
 */

import type {
    CompiledTemplate,
    CompiledTemplateNode,
    TemplateAttributePart,
    TemplateIfNode,
} from './state-template';

/** Elements with no closing tag; the printer must not invent one. */
const VOID_TAGS = new Set(['br', 'hr', 'img']);

export function printTemplate(markup: CompiledTemplate | null | undefined): string {
    return printNodes(markup?.nodes ?? []);
}

function printNodes(nodes: readonly CompiledTemplateNode[]): string {
    return nodes.map(printNode).join('');
}

function printNode(node: CompiledTemplateNode): string {
    switch (node?.kind) {
        case 'element': {
            const rendered: Record<string, string> = { ...(node.attrs ?? {}) };
            for (const [name, parts] of Object.entries(node.boundAttrs ?? {})) {
                rendered[name] = (parts ?? []).map(printAttributePart).join('');
            }
            const attrs = Object.entries(rendered)
                .map(([name, value]) => ` ${name}="${escapeAttribute(value)}"`)
                .join('');
            if (VOID_TAGS.has(node.tag)) {
                return `<${node.tag}${attrs}>`;
            }
            return `<${node.tag}${attrs}>${printNodes(node.children ?? [])}</${node.tag}>`;
        }
        case 'text':
            return escapeText(node.text);
        case 'value':
            return `{{value ${node.key}}}`;
        case 'if': {
            const otherwise = (node.else?.length ?? 0) > 0
                ? `{{else}}${printNodes(node.else)}`
                : '';
            return `{{#if ${printCondition(node)}}}${printNodes(node.then ?? [])}${otherwise}{{/if}}`;
        }
        case 'each':
            return `{{#each ${node.key}}}${printNodes(node.body ?? [])}{{/each}}`;
        default:
            // An unknown node renders as nothing, so it prints as nothing.
            return '';
    }
}

function printAttributePart(part: TemplateAttributePart): string {
    return part?.kind === 'value' ? `{{value ${part.key}}}` : String(part?.text ?? '');
}

function printCondition(node: TemplateIfNode): string {
    return node.op ? `${node.key} ${node.op}${node.value ? ` ${node.value}` : ''}` : node.key;
}

function escapeText(text: string): string {
    return String(text).replace(/&/gu, '&amp;').replace(/</gu, '&lt;').replace(/>/gu, '&gt;');
}

function escapeAttribute(text: string): string {
    return escapeText(text).replace(/"/gu, '&quot;');
}
