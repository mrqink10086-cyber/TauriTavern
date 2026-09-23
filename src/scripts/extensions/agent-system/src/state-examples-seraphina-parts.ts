/**
 * The non-field, non-panel parts of the Seraphina showcase.
 *
 * A scene document is one thing, but three of its members are authored as code
 * or as rules: the shared script modules a picture set imports, the machine that
 * moves the stages, and the conditional text. Keeping them beside the fields
 * would bury the declaration under them, so they live here and the scene reads
 * as a list of what it declares.
 */

import type { MachineSpec } from './state-machine-model';
import type { StatePredicateSet } from './state-predicate-model';
import type { StateScriptModules } from './state-config-model';

/**
 * The one logic several picture sets share.
 *
 * A condition script is called with `{ state, candidates }` and answers with the
 * index of the picture it chose, so the module reads the mood and picks a
 * candidate — the part that a plain condition cannot express, kept in one place
 * and one line away from the set that uses it.
 */
export const SERAPHINA_SCENE_SCRIPTS: StateScriptModules = {
    'scene.js': [
        'export function pickMood(args) {',
        '    const state = (args && args.state) || {};',
        '    const candidates = (args && Array.isArray(args.candidates)) ? args.candidates : [];',
        '    if (candidates.length === 0) {',
        '        return { index: null };',
        '    }',
        "    const values = state['角色/Seraphina/心情'];",
        "    const mood = Array.isArray(values) && values.length > 0 ? String(values[0]) : '';",
        "    const dark = mood === '担忧' || mood === '恐惧' || mood === '悲伤' || mood === '愤怒';",
        '    return { index: dark && candidates.length > 1 ? 1 : 0 };',
        '}',
    ].join('\n'),
};

/**
 * Day, night, danger, and a presence that is always true.
 *
 * Between them they use what the machine layer offers: several positions, the
 * same source moving two ways, a combined condition (`all` with a `not` inside),
 * an unconditional entry (`from: []`), field writes, an emitted event, a
 * priority, and a hook that only ever says yes.
 */
export const SERAPHINA_MACHINE: MachineSpec = {
    initial: ['白天'],
    states: [
        { id: '白天', label: '白天' },
        { id: '夜晚', label: '夜晚' },
        { id: '危险', label: '危险' },
        { id: '在场', label: '在场' },
    ],
    transitions: [
        {
            id: 'day-to-night',
            from: ['白天'],
            to: ['夜晚'],
            conditions: [{
                source: 'field',
                field: '环境/时间',
                op: 'in',
                values: ['傍晚', '晚上', '深夜'],
            }],
            actions: [
                { kind: 'setField', target: '环境/光源', values: ['烛火'] },
                { kind: 'emit', target: 'scene/入夜' },
            ],
        },
        {
            id: 'night-to-day',
            from: ['夜晚'],
            to: ['白天'],
            conditions: [{
                source: 'field',
                field: '环境/时间',
                op: 'in',
                values: ['清晨', '上午', '午后'],
            }],
            actions: [
                { kind: 'setField', target: '环境/光源', values: ['窗缝的晨光'] },
                { kind: 'emit', target: 'scene/天亮' },
            ],
        },
        {
            id: 'danger-rises',
            from: ['白天', '夜晚'],
            to: ['危险'],
            // A combination carries no comparison of its own: the empty `source`
            // and `op` say so, which is the shape the machine editor stores too.
            conditions: [{
                source: '',
                op: '',
                compose: {
                    all: [
                        { source: 'field', field: '世界/危险度', op: 'in', values: ['高', '极高'] },
                        // `not` keeps the rule idempotent: without it the same
                        // transition would keep firing while danger stayed high.
                        {
                            source: '',
                            op: '',
                            compose: { not: { source: 'active', op: 'active', values: ['危险'] } },
                        },
                    ],
                },
            }],
            actions: [{ kind: 'emit', target: 'scene/险境' }],
            priority: 10,
        },
        {
            id: 'danger-passes',
            from: ['危险'],
            to: ['白天'],
            conditions: [{
                source: 'field',
                field: '世界/危险度',
                op: 'in',
                values: ['低', '中'],
            }],
            actions: [],
        },
        {
            id: 'always-present',
            // No `from`: this one holds whoever else does, which is how a scene
            // keeps a position of its own alongside the story's.
            from: [],
            to: ['在场'],
            conditions: [],
            actions: [],
        },
    ],
    hooks: {
        script: 'export default (args) => ({ allow: true });',
    },
};

/**
 * What the story sounds like right now.
 *
 * Two groups and a standing line: the first group is a contest between two ways
 * of behaving, the second reads the world and acts on the first by label — one
 * entry takes the closeness away while the character is hurt, the other waits
 * for the hurt before it may speak. The standing entry is outside both groups,
 * because a rule that never competes should not be able to lose.
 */
export const SERAPHINA_PREDICATES: StatePredicateSet = {
    groups: [
        {
            id: 'tone',
            label: '相处',
            entries: [
                {
                    id: 'distant',
                    label: '保持距离',
                    content: '彼此还不到交心的距离：说话留三分，不主动靠近。',
                    tags: ['distance'],
                    priority: 1,
                },
                {
                    id: 'close',
                    label: '亲近',
                    content: '距离已经破了：可以直说，可以靠近，也不必解释。',
                    tags: ['closeness'],
                    source: {
                        kind: 'state',
                        condition: {
                            source: 'field',
                            field: '角色/Seraphina/好感度',
                            op: 'gte',
                            value: '40',
                        },
                    },
                    priority: 10,
                },
            ],
        },
        {
            id: 'condition',
            label: '处境',
            entries: [
                {
                    id: 'hurt',
                    label: '重伤在身',
                    content: '她自己也在疼，动作比平时慢，说话间会停一下。',
                    tags: ['hurt'],
                    availability: {
                        source: 'field',
                        field: '主角/伤势',
                        op: 'ne',
                        value: '无',
                    },
                    effects: [{ kind: 'inhibit', tags: ['closeness'] }],
                    priority: 5,
                },
                {
                    id: 'guarded',
                    label: '风声不对',
                    content: '外面有东西在转：她能不动就不动，也让你别出声。',
                    tags: ['guarded'],
                    source: {
                        kind: 'state',
                        condition: {
                            source: 'field',
                            field: '世界/危险度',
                            op: 'in',
                            values: ['高', '极高'],
                        },
                    },
                    effects: [{ kind: 'require', tags: ['hurt'] }],
                },
            ],
        },
    ],
    constants: [
        {
            id: 'standing',
            label: '常驻',
            content: '无论何时，先照顾对方的感受再开口。',
        },
    ],
};
