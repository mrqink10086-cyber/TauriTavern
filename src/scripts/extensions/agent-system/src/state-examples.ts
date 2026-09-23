/**
 * Starter documents, shown the moment something new is created.
 *
 * A blank editor answers "what am I supposed to type here?" with nothing. The
 * examples are small but complete — everyday scene fields, two panels matching
 * them, a day/night machine — so a new declaration or machine starts as
 * something to edit, not a void. They are ordinary documents: every row can be
 * changed or deleted, and both are stored exactly as shown.
 *
 * The panels cover every branch the example declares. A configured panel list
 * replaces the derived one, so a branch left without a panel would be the one
 * thing the example silently hid.
 */

import type { StateDeclaration } from './state-config-model';
import type { MachineSpec } from './state-machine-model';
import type { StatePredicateSet } from './state-predicate-model';

/**
 * The name a new document is offered under.
 *
 * It is filled in rather than merely suggested, so a new declaration or machine
 * is one click away from showing its example instead of an empty form.
 */
export const DEFAULT_DECLARATION_NAME = 'scene-state';
export const DEFAULT_MACHINE_NAME = 'scene-flow';
export const DEFAULT_PREDICATE_NAME = 'scene-tones';

export function exampleDeclaration(): StateDeclaration {
    return {
        fields: [
            { pattern: '环境/日期', label: '日期' },
            { pattern: '环境/时间', label: '时间' },
            { pattern: '环境/天气', label: '天气' },
            { pattern: '环境/地点', label: '地点' },
            { pattern: '角色/*/好感度', label: '好感度' },
            { pattern: '角色/*/着装', label: '着装' },
        ],
        panels: {
            panels: [
                {
                    title: '环境',
                    rail: 'left',
                    match: '环境/**',
                    // A template, not one row per field: the two that read
                    // together share a line, and the missing one simply does not
                    // appear. It is also the shortest way to show that a panel
                    // can be written by hand rather than derived.
                    templateSource: [
                        '<div class="scene">',
                        '  <div class="scene-when">{{value 环境/日期}} {{value 环境/时间}}</div>',
                        '  <div class="scene-where">{{value 环境/地点}}</div>',
                        '  {{#if 环境/天气}}<div class="scene-weather">{{value 环境/天气}}</div>{{/if}}',
                        '</div>',
                    ].join('\n'),
                },
                { title: '角色', rail: 'right', match: '角色/**' },
            ],
            // Three levels, and this is the middle one: the built-in default
            // style, then this sheet, then the template's own markup. A level
            // only says what it changes.
            css: [
                '.scene { display: flex; flex-direction: column; gap: 2px; }',
                '.scene-when { font-weight: 600; }',
                '.scene-where, .scene-weather { opacity: 0.75; font-size: 0.9em; }',
            ].join('\n'),
        },
    };
}

export function exampleMachine(): MachineSpec {
    return {
        initial: ['白天'],
        states: [
            { id: '白天', label: '白天' },
            { id: '夜晚', label: '夜晚' },
        ],
        transitions: [{
            from: ['白天'],
            to: ['夜晚'],
            conditions: [{
                source: 'field',
                field: '环境/时间',
                op: 'in',
                values: ['傍晚', '晚上', '深夜'],
            }],
            actions: [{ kind: 'emit', target: 'scene/夜晚' }],
        }],
        hooks: null,
    };
}

/**
 * A set that reads the same fields the example declaration declares.
 *
 * It shows both shapes at once: a group where two tones compete and only the
 * higher priority survives, and a standing entry that never competes. Both
 * entries read `环境/时间`, so the preview has something to answer as soon as
 * one assumed value is typed.
 */
export function examplePredicateSet(): StatePredicateSet {
    return {
        groups: [{
            id: 'tone',
            label: '相处',
            entries: [
                {
                    id: 'distant',
                    label: '保持距离',
                    content: '彼此还不到交心的距离：说话留三分，不主动靠近。',
                    tags: ['distance'],
                },
                {
                    id: 'close',
                    label: '亲近',
                    content: '距离已经破了：可以直说，可以靠近，也不必解释。',
                    tags: ['closeness'],
                    source: {
                        kind: 'state',
                        condition: { source: 'field', field: '环境/时间', op: 'in', values: ['晚上', '深夜'] },
                    },
                    priority: 10,
                },
            ],
        }],
        constants: [{
            id: 'standing',
            label: '常驻',
            content: '无论何时，先照顾对方的感受再开口。',
        }],
    };
}
