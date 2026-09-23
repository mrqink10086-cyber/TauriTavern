/**
 * The scene the default character card ships with, as a worked example.
 *
 * It is meant to be a showcase, not a starting point to copy: every capability
 * the state system has is used once, in a place where a reader can see why it is
 * there. A scene that only declares fields answers "what can this do?" with
 * silence, so this one declares fields with three different access grants, uses
 * all three key spellings, sets ceilings, draws two panels (one from a compiled
 * markup document, one from a scripted picture set), reads a prose block,
 * carries shared script modules, a theme sheet, a machine with combined
 * conditions and a hook, and a predicate set with competing entries and one
 * standing instruction.
 *
 * Three things are worth reading here:
 *
 * - Every literal field starts with a value, so the scene opens with a world in
 *   it instead of asking the model to invent one before the story can start.
 * - The pictures are host paths under `/backgrounds/`, the one form both the
 *   domain and the renderer accept.
 * - The machine, the predicate set and the shared scripts travel inside this
 *   document: one scene, one binding, one file.
 */

import type { StateDeclaration } from './state-config-model';
import {
    SERAPHINA_MACHINE,
    SERAPHINA_PREDICATES,
    SERAPHINA_SCENE_SCRIPTS,
} from './state-examples-seraphina-parts';

export const SERAPHINA_SCENE_NAME = 'seraphina-scene';

/**
 * What the model may do with a field, shown once per grant.
 *
 * - `maintained`: told about it, may look it up, may write it.
 * - `watched`: told about it, may look it up, may not write it — the story can
 *   observe the weather without being able to change it.
 * - `hidden`: knows nothing about it. The value still exists and the machine and
 *   the predicate set can read it; nothing is injected, so the model cannot.
 */
const maintained = {
    inject: true,
    visible: true,
    writable: true,
} as const;

const watched = {
    inject: true,
    visible: true,
    writable: false,
} as const;

const hidden = {
    inject: false,
    visible: false,
    writable: false,
} as const;

export function seraphinaDeclaration(): StateDeclaration {
    return {
        limits: {
            value: 256,
            valuesPerField: 16,
            fieldsPerUpdate: 64,
            unit: 'chars',
        },
        fields: [
            // Literal keys: the only spelling that may carry an initial value.
            { pattern: '环境/日期', label: '日期', initial: ['春天 · 第 3 天'], access: maintained },
            { pattern: '环境/时间', label: '时间', initial: ['清晨'], access: maintained },
            { pattern: '环境/天气', label: '天气', initial: ['薄雾'], access: maintained },
            { pattern: '环境/地点', label: '地点', initial: ['塞拉菲娜的小屋'], access: maintained },
            // Observed, not written: the weather decides the picture, and a model
            // that could rewrite it could rewrite the panel into anything.
            { pattern: '环境/光源', label: '光源', initial: ['窗缝的晨光'], access: watched },
            // Never injected: it exists so the machine and the predicate set have
            // something to read, which is what "injection is admission" means.
            { pattern: '世界/危险度', label: '危险度', initial: ['低'], access: hidden },
            { pattern: '主角/伤势', label: '伤势', initial: ['重伤未愈'], access: maintained },
            { pattern: '主角/体力', label: '体力', initial: ['低'], access: maintained },
            // A picture the panel shows as a picture: the value is a host path.
            { pattern: '角色/Seraphina/立绘', label: '立绘', initial: ['/backgrounds/royal.jpg'], access: watched },
            {
                pattern: '角色/Seraphina/好感度',
                label: '好感度',
                initial: ['25'],
                access: maintained,
            },
            { pattern: '角色/Seraphina/心情', label: '心情', initial: ['担忧'], access: maintained },
            {
                pattern: '角色/Seraphina/照料',
                label: '照料',
                initial: ['刚换过草药绷带'],
                access: maintained,
            },
            // A wildcard: one row covers every character, and no initial value,
            // because a pattern has no key to write one to.
            { pattern: '角色/*/称呼', label: '称呼', access: maintained },
            // A regular expression: keys nobody can enumerate in advance.
            { pattern: '/^线索\\/.+/', label: '线索', access: maintained },
        ],
        panels: {
            panels: [
                {
                    // One panel, not one per half of the scene: where you are and
                    // how she is get read together, and a second window over the
                    // chat is one more thing to move out of the way. `match` is
                    // everything, because the template binds both halves — the
                    // panel's own rows are the pictures, which are hers.
                    title: '此刻',
                    rail: 'left',
                    match: '**',
                    templateSource: [
                        '<div class="scene">',
                        '  <div class="scene-when">{{value 环境/日期}} · {{value 环境/时间}}</div>',
                        '  <div class="scene-where">{{value 环境/地点}}</div>',
                        '  {{#if 环境/天气}}<div class="scene-soft">{{value 环境/天气}}</div>{{/if}}',
                        '  {{#if 环境/光源}}<div class="scene-soft">{{value 环境/光源}}</div>{{/if}}',
                        '  {{#if 主角/伤势}}<div class="scene-soft">{{value 主角/伤势}}</div>{{/if}}',
                        // A value in an attribute, which is what a bar is: the
                        // number the model wrote decides how wide the fill is.
                        '  <div class="scene-bar">',
                        '    <span class="scene-bar__label">体力</span>',
                        '    <span class="scene-bar__track">',
                        '      <span class="scene-bar__fill" style="width: {{value 主角/体力}}%"></span>',
                        '    </span>',
                        '    <span class="scene-bar__value">{{value 主角/体力}}</span>',
                        '  </div>',
                        // A comparison, not just "has a value": 体力 is always
                        // written, so only a threshold says anything.
                        '  {{#if 主角/体力 lt 30}}<div class="scene-warn">撑不住了。</div>{{/if}}',
                        '  <div class="scene-name">{{value 角色/Seraphina/心情}}</div>',
                        '  <div class="scene-bar">',
                        '    <span class="scene-bar__label">好感度</span>',
                        '    <span class="scene-bar__track">',
                        '      <span class="scene-bar__fill" style="width: {{value 角色/Seraphina/好感度}}%"></span>',
                        '    </span>',
                        '    <span class="scene-bar__value">{{value 角色/Seraphina/好感度}}</span>',
                        '  </div>',
                        '  {{#if 角色/Seraphina/好感度 gte 60}}<div class="scene-soft">她不再跟你客气了。</div>{{/if}}',
                        '  {{#if 角色/Seraphina/照料}}<div class="scene-soft">{{value 角色/Seraphina/照料}}</div>{{/if}}',
                        '</div>',
                    ].join('\n'),
                    // Pictures by condition, in order: the first candidate whose
                    // condition holds wins, and the one without a condition is
                    // the fallback — so it has to be last.
                    background: {
                        candidates: [
                            {
                                source: '/backgrounds/_black.jpg',
                                when: {
                                    source: 'field',
                                    field: '环境/时间',
                                    op: 'in',
                                    values: ['傍晚', '晚上', '深夜'],
                                },
                            },
                            { source: '/backgrounds/royal.jpg' },
                        ],
                    },
                    // Her own voice, in her own file: prose is not a field, and a
                    // diary entry does not belong in a 256-character value.
                    prose: { path: 'persist/心声.md', title: '心声' },
                    fields: [
                        { pattern: '角色/Seraphina/立绘', label: '立绘', render: 'image' },
                        {
                            pattern: '角色/Seraphina/心情',
                            label: '心情',
                            images: {
                                candidates: [
                                    { source: '/backgrounds/_white.jpg' },
                                    { source: '/backgrounds/_black.jpg' },
                                ],
                                // This script replaces the candidates' conditions,
                                // and decides from the shared module below.
                                conditionScript: {
                                    script: [
                                        "import { pickMood } from './scene.js';",
                                        'export default pickMood;',
                                    ].join('\n'),
                                },
                            },
                        },
                    ],
                },
            ],
            scripts: SERAPHINA_SCENE_SCRIPTS,
            css: [
                '.scene { display: flex; flex-direction: column; gap: 4px; }',
                '.scene-when { font-weight: 600; }',
                '.scene-where { font-weight: 500; }',
                '.scene-name { font-weight: 600; }',
                '.scene-soft { opacity: 0.75; font-size: 0.9em; }',
                '.scene-warn { color: #e8a0a0; font-size: 0.9em; }',
                '.scene-bar { display: flex; align-items: center; gap: 6px; }',
                '.scene-bar__label { flex: 0 0 auto; opacity: 0.75; font-size: 0.85em; }',
                '.scene-bar__value { flex: 0 0 auto; opacity: 0.6; font-size: 0.8em; }',
                '.scene-bar__track { flex: 1 1 auto; height: 6px; overflow: hidden;',
                '  border-radius: 3px; background-color: rgba(255, 255, 255, 0.16); }',
                '.scene-bar__fill { display: block; height: 100%; background-color: currentColor; }',
                '.tt-state-panel__body { border-radius: 6px; }',
            ].join('\n'),
        },
        // The scene carries its stages and its conditional text too: one store,
        // one binding, one export.
        machine: SERAPHINA_MACHINE,
        predicates: SERAPHINA_PREDICATES,
    };
}
