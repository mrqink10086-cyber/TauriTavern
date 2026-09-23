import { describe, expect, test } from '@rstest/core';

import { compileTemplate, printTemplate } from './state-template';

function compile(source: string) {
    const { markup, issues } = compileTemplate(source, 'panel `环境` template');
    return { markup, issues };
}

describe('the panel template', () => {
    test('markup and the bindings compile into a tree', () => {
        const { markup, issues } = compile(
            '<div class="env">{{value 环境/日期}}{{#if 环境/天气}} · {{value 环境/天气}}{{else}} · 未知{{/if}}</div>',
        );

        expect(issues).toEqual([]);
        expect(markup).toEqual({
            nodes: [{
                kind: 'element',
                tag: 'div',
                attrs: { class: 'env' },
                boundAttrs: {},
                children: [
                    { kind: 'value', key: '环境/日期' },
                    {
                        kind: 'if',
                        key: '环境/天气',
                        op: '',
                        value: '',
                        then: [
                            { kind: 'text', text: ' · ' },
                            { kind: 'value', key: '环境/天气' },
                        ],
                        else: [{ kind: 'text', text: ' · 未知' }],
                    },
                ],
            }],
        });
    });

    test('a second document or stylesheet is refused, and by name', () => {
        for (const [source, expected] of [
            ['<script>alert(1)</script>', '<script>'],
            ['<style>.a{}</style>', '<style>'],
            ['<iframe src="/x"></iframe>', '<iframe>'],
        ] as const) {
            const { markup, issues } = compile(source);

            expect(markup).toBeNull();
            expect(issues.length).toBeGreaterThan(0);
            expect(issues.map((issue) => issue.message).join(' ')).toContain(expected);
            expect(issues[0]?.path).toBe('panel `环境` template');
        }
    });

    test('an icon can be drawn in place, with the attributes a real one needs', () => {
        const { markup, issues } = compile(
            '<svg viewBox="0 0 24 24" class="mood"><path d="M4 4h16v16H4z" fill="currentColor"></path></svg>'
            + '<div id="mood-card" style="display:flex" onclick="this.classList.toggle(\'open\')"></div>',
        );

        expect(issues).toEqual([]);
        expect(markup?.nodes[0]).toMatchObject({ kind: 'element', tag: 'svg' });
        expect(markup?.nodes[1]).toMatchObject({
            kind: 'element',
            tag: 'div',
            attrs: {
                id: 'mood-card',
                style: 'display:flex',
                onclick: "this.classList.toggle('open')",
            },
        });
    });

    test('a value can be part of an attribute', () => {
        const { markup, issues } = compile(
            '<div class="bar bar--{{value 角色/心情}}" style="width: {{value 角色/好感度}}%"></div>',
        );

        expect(issues).toEqual([]);
        expect(markup?.nodes[0]).toMatchObject({
            kind: 'element',
            attrs: {},
            boundAttrs: {
                class: [
                    { kind: 'text', text: 'bar bar--' },
                    { kind: 'value', key: '角色/心情' },
                ],
                style: [
                    { kind: 'text', text: 'width: ' },
                    { kind: 'value', key: '角色/好感度' },
                    { kind: 'text', text: '%' },
                ],
            },
        });
    });

    test('a handler takes code, not a binding', () => {
        const { markup, issues } = compile('<div onclick="{{value 环境/日期}}"></div>');

        expect(markup).toBeNull();
        expect(issues[0]?.message).toContain('a handler is code');
    });

    test('a raw binding is refused rather than ignored', () => {
        const { markup, issues } = compile('{{raw 环境/日期}}');

        expect(markup).toBeNull();
        expect(issues[0]?.message).toContain('always inserted as text');
    });

    test('a binding that is not one of the vocabulary is named', () => {
        const { issues } = compile('{{date 环境/日期}}');

        expect(issues[0]?.message).toContain('the vocabulary is');
    });

    test('a binding names one field, not a pattern', () => {
        for (const key of ['角色/*/好感度', '/环境/日期/', '']) {
            const { markup, issues } = compile(`{{value ${key}}}`);

            expect(markup).toBeNull();
            expect(issues.length).toBeGreaterThan(0);
        }
    });

    test('a condition can compare, and says so when it cannot', () => {
        const { markup, issues } = compile('{{#if 主角/体力 gt 30}}跑{{else}}走{{/if}}');

        expect(issues).toEqual([]);
        expect(markup?.nodes[0]).toMatchObject({ kind: 'if', key: '主角/体力', op: 'gt', value: '30' });

        // An op the renderer does not know, one that takes no value, and one that
        // needs it.
        for (const source of [
            '{{#if 主角/体力 like 30}}x{{/if}}',
            '{{#if 主角/体力 exists 30}}x{{/if}}',
            '{{#if 主角/体力 gt}}x{{/if}}',
        ]) {
            expect(compile(source).markup).toBeNull();
        }
        // `exists` on its own is a comparison the renderer can make.
        expect(compile('{{#if 主角/体力 exists}}x{{/if}}').issues).toEqual([]);
    });

    test('a loop takes a pattern, and its body may name that pattern', () => {
        const { markup, issues } = compile(
            '{{#each 角色/*}}<div>{{value 角色/*}}</div>{{/each}}',
        );

        expect(issues).toEqual([]);
        expect(markup?.nodes[0]).toMatchObject({
            kind: 'each',
            key: '角色/*',
            body: [{
                kind: 'element',
                tag: 'div',
                children: [{ kind: 'value', key: '角色/*' }],
            }],
        });

        // One field is not a loop.
        expect(compile('{{#each 角色/爱丽丝}}x{{/each}}').markup).toBeNull();
    });

    test('markers that do not pair up are refused', () => {
        expect(compile('{{#if 环境/天气}}').issues[0]?.message).toContain('never closed');
        expect(compile('{{#each 角色/*}}').issues[0]?.message).toContain('never closed');
        expect(compile('{{/if}}').issues[0]?.message).toContain('closes nothing');
        expect(compile('{{/each}}').issues[0]?.message).toContain('closes nothing');
        expect(compile('{{#each 角色/*}}x{{/if}}').issues[0]?.message).toContain('closes a `{{#each}}`');
        expect(compile('{{else}}').issues[0]?.message).toContain('no `{{#if}}` to belong to');
        expect(compile('{{#if 环境/天气}}a{{else}}b{{else}}c{{/if}}').issues[0]?.message)
            .toContain('one `{{else}}`');
        expect(compile('{{value 环境/日期').issues[0]?.message).toContain('never closed');
    });

    test('saving and reopening is a round trip', () => {
        const source = '<div class="env"><strong>{{value 环境/日期}}</strong>'
            + '{{#if 环境/天气}}<small>{{value 环境/天气}}</small>{{/if}}<br>'
            + '<img src="/backgrounds/a.png" alt="x" style="width: {{value 主角/体力}}%">'
            + '{{#each 角色/*}}<span class="{{value 角色/*}}">{{value 角色/*}}</span>{{/each}}</div>';
        const first = compile(source).markup;

        const printed = printTemplate(first);
        const second = compile(printed).markup;

        expect(second).toEqual(first);
        // And the printed text is still the template the user wrote, not a
        // different one that happens to compile.
        expect(printTemplate(second)).toBe(printed);
    });

    test('an empty template is no template', () => {
        expect(compile('   ')).toEqual({ markup: null, issues: [] });
        expect(printTemplate(null)).toBe('');
    });
});
