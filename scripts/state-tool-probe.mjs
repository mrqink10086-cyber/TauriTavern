// 手工验证脚本：用本地 OpenAI 兼容端点跑状态系统的三个假设。
//
//   A. 工具 schema 能否约束住字段 —— 模型是否产出符合 descriptor 的参数。
//   B. 字段级错误回灌能否让模型一轮改对 —— 构造典型错误后看它怎么改。
//   C. 注入即准入 —— 只看得见被注入字段的模型，是否只写回它看得见的键，
//      并保持增量语义（未变化的字段不重复提交）。
//
// 需要本地端点，因此不进自动化测试（`pnpm test` 只跑 `tests/**/*.test.mjs`）：
//   node scripts/state-tool-probe.mjs
//   STATE_PROBE_BASE=http://127.0.0.1:10086/v1 STATE_PROBE_MODEL=qwen3.8-27b node scripts/state-tool-probe.mjs
//
// schema 必须与 src-tauri/crates/tt-application/src/services/agent_tools/state/descriptors.rs 一致，
// 错误信息必须与同目录 update.rs 的 rejected() / tt-domain models/state.rs 的报错一致。
// 模型侧工具名把 `.` 换成 `_`（见 registry.rs 的 model_alias 约定），所以这里是 state_update。

const BASE = process.env.STATE_PROBE_BASE ?? 'http://127.0.0.1:10086/v1';
const MODEL = process.env.STATE_PROBE_MODEL ?? 'qwen3.8-27b';

const TOOL = {
  type: 'function',
  function: {
    name: 'state_update',
    description:
      'Record what changed in the tracked state. Send only the fields that changed: a field you leave out keeps its current value, an empty value array clears it, and `remove` deletes it. Keys must be the ones the state declaration defines.',
    parameters: {
      type: 'object',
      additionalProperties: false,
      properties: {
        fields: {
          type: 'array',
          description: 'Fields to set or clear.',
          items: {
            type: 'object',
            additionalProperties: false,
            properties: {
              key: {
                type: 'string',
                description: "The field's key, spelled the way the state declaration writes it.",
              },
              value: {
                type: 'array',
                items: { type: 'string' },
                description:
                  'One entry per line of the field\'s value. Send an empty array to clear the field without deleting it.',
              },
            },
            required: ['key', 'value'],
          },
        },
        remove: {
          type: 'array',
          items: { type: 'string' },
          description:
            'Keys to delete outright. An empty value only clears; deleting needs this list.',
        },
      },
    },
  },
};

// 示例声明。键空间由用户声明定义，这里的键名只是占位（见 docs/Agent/State.md 的边界一节）。
const DECLARED = ['日期', '时间'];

const SYSTEM = `你是状态记录助手。用户描述场景后，调用 state_update 把状态记录下来。
本次聊天声明的字段只有：${DECLARED.join('、')}。
只记录声明的字段，不要新增字段。不要输出解释文字，直接调用工具。`;

const USER = '记录这次场景：2026 年 9 月 10 日，下午三点左右，在城西的一家旧书店。店里很安静。';

async function complete(messages, tools) {
  const body = { model: MODEL, messages, temperature: 0 };
  if (tools) {
    body.tools = tools;
    body.tool_choice = 'auto';
  }
  const response = await fetch(`${BASE}/chat/completions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${(await response.text()).slice(0, 400)}`);
  }
  const payload = await response.json();
  return payload.choices[0].message;
}

function firstCall(message) {
  const call = message.tool_calls?.[0];
  if (!call) return null;
  return { id: call.id, raw: call.function.arguments };
}

// ---- A. schema 符合性 ----
function checkSchema(raw) {
  const problems = [];
  let args;
  try {
    args = JSON.parse(raw);
  } catch (error) {
    return { args: null, problems: [`arguments 不是合法 JSON: ${error.message}`] };
  }
  if (typeof args !== 'object' || args === null || Array.isArray(args)) {
    return { args, problems: ['arguments 必须是对象'] };
  }
  for (const key of Object.keys(args)) {
    if (!['fields', 'remove'].includes(key)) problems.push(`出现了未定义的参数 \`${key}\``);
  }
  if (args.fields === undefined && args.remove === undefined) {
    problems.push('fields 与 remove 都没有提供');
  }
  if (args.fields !== undefined) {
    if (!Array.isArray(args.fields)) problems.push('fields 必须是数组');
    else {
      args.fields.forEach((item, index) => {
        if (typeof item !== 'object' || item === null || Array.isArray(item)) {
          problems.push(`fields[${index}] 必须是对象`);
          return;
        }
        for (const key of Object.keys(item)) {
          if (!['key', 'value'].includes(key)) {
            problems.push(`fields[${index}] 出现未定义字段 \`${key}\``);
          }
        }
        if (typeof item.key !== 'string') problems.push(`fields[${index}].key 必须是字符串`);
        if (!Array.isArray(item.value)) {
          problems.push(`fields[${index}].value 必须是字符串数组（实际是 ${typeof item.value}）`);
        } else {
          item.value.forEach((line, lineIndex) => {
            if (typeof line !== 'string') {
              problems.push(`fields[${index}].value[${lineIndex}] 必须是字符串`);
            }
          });
        }
      });
    }
  }
  if (args.remove !== undefined) {
    if (!Array.isArray(args.remove)) problems.push('remove 必须是数组');
    else {
      args.remove.forEach((item, index) => {
        if (typeof item !== 'string') problems.push(`remove[${index}] 必须是字符串`);
      });
    }
  }
  return { args, problems };
}

function checkDeclaration(args) {
  const problems = [];
  if (!Array.isArray(args?.fields)) return problems;
  for (const item of args.fields) {
    if (typeof item?.key === 'string' && !DECLARED.includes(item.key)) {
      problems.push(`key \`${item.key}\` 不在声明中`);
    }
  }
  for (const key of args?.remove ?? []) {
    if (typeof key === 'string' && !DECLARED.includes(key)) {
      problems.push(`remove 的 key \`${key}\` 不在声明中`);
    }
  }
  return problems;
}

function report(title, lines) {
  console.log(`\n=== ${title} ===`);
  for (const line of lines) console.log(line);
}

// ---- A. 正常一轮：看 schema 约束 ----
async function probeSchema() {
  const message = await complete(
    [
      { role: 'system', content: SYSTEM },
      { role: 'user', content: USER },
    ],
    [TOOL],
  );
  const call = firstCall(message);
  if (!call) {
    report('A. schema 约束', ['模型没有调用工具', `正文: ${(message.content ?? '').slice(0, 200)}`]);
    return null;
  }
  const { args, problems } = checkSchema(call.raw);
  const declarationProblems = args ? checkDeclaration(args) : [];
  report('A. schema 约束', [
    `原始参数: ${call.raw}`,
    `schema 违规: ${problems.length === 0 ? '无' : ''}`,
    ...problems.map((problem) => `  - ${problem}`),
    `声明违规: ${declarationProblems.length === 0 ? '无' : ''}`,
    ...declarationProblems.map((problem) => `  - ${problem}`),
    `结论: ${problems.length === 0 ? 'schema 被遵守' : 'schema 未被遵守'}`,
  ]);
  return call;
}

// ---- B. 错误回灌：构造两个典型错误，看能否一轮改对 ----
const BAD_ARGUMENTS = JSON.stringify({
  fields: [
    { key: '日期', value: '2026-09-10' },
    { key: '地点', value: ['旧书店'] },
  ],
});

// 与 update.rs 的 rejected() 输出保持一致：一句总述 + 每个问题一行。
const BAD_RESULT = [
  'The state update was rejected and nothing was written. Fix every problem below, then call state.update again with the whole change set:',
  '- fields[0].value must be an array of strings, even for a single value; send [] to clear the field',
  '- key `地点` is not in the state declaration',
].join('\n');

async function probeCorrection() {
  const messages = [
    { role: 'system', content: SYSTEM },
    { role: 'user', content: USER },
    {
      role: 'assistant',
      content: null,
      tool_calls: [
        {
          id: 'call_1',
          type: 'function',
          function: { name: 'state_update', arguments: BAD_ARGUMENTS },
        },
      ],
    },
    { role: 'tool', tool_call_id: 'call_1', content: BAD_RESULT },
  ];
  const message = await complete(messages, [TOOL]);
  const call = firstCall(message);
  if (!call) {
    report('B. 错误回灌一轮改对', [
      '模型没有重新调用工具',
      `正文: ${(message.content ?? '').slice(0, 300)}`,
    ]);
    return;
  }
  const { args, problems } = checkSchema(call.raw);
  const declarationProblems = args ? checkDeclaration(args) : [];
  const all = [...problems, ...declarationProblems];
  report('B. 错误回灌一轮改对', [
    `原始参数: ${call.raw}`,
    `剩余问题: ${all.length === 0 ? '无' : ''}`,
    ...all.map((problem) => `  - ${problem}`),
    `结论: ${all.length === 0 ? '一轮改对' : '仍未通过'}`,
  ]);
}

// ---- C. 注入即准入：模型只看得见被注入的字段，也只能写回它看得见的键 ----
//
// 注入文本由 tt-domain 的 `render_injection` 产出：每个字段一行 `键: 值`，按
// 键序排列，未授权注入的字段完全不出现。本段验证这段文本足以让模型
//   C1. 只写回**被注入**的键（声明里存在但未注入的字段不该被它碰），
//   C2. 键的拼写与注入文本一致（不缩写、不加路径前缀），
//   C3. 只提交**变化**的字段（增量语义）。
// 注入格式若变动，本段随之更新（与 A 段对 descriptor 的耦合同理）。

const INJECT_DECLARED = ['环境/日期', '环境/时间', '环境/地点', '角色/艾拉/好感度'];
// 场景会改变地点与时间，所以它们必须被注入——模型只能写它看得见的字段，
// 未注入的字段它不会去更新（实测会把变化挤进看得见的字段里）。好感度声明
// 但未注入，用来验证"没注入的字段，模型不知道它存在"。
const INJECTED_STATE = [
  '环境/日期: 2026/09/10',
  '环境/时间: 下午三点左右',
  '环境/地点: 城西旧书店',
].join('\n');

const INJECT_SYSTEM = `你是状态记录助手。需要记录状态变化时调用 state_update；键必须是已声明的字段，不要新建字段。
以下是本次聊天已经记录的状态：
${INJECTED_STATE}`;

const INJECT_USER = '场景推进：我们离开书店，回到城西的公寓，天已经黑了，大概晚上八点。';

async function probeInjection() {
  const message = await complete(
    [
      { role: 'system', content: INJECT_SYSTEM },
      { role: 'user', content: INJECT_USER },
    ],
    [TOOL],
  );
  const call = firstCall(message);
  if (!call) {
    report('C. 注入即准入', ['模型没有调用工具', `正文: ${(message.content ?? '').slice(0, 200)}`]);
    return;
  }

  const { args, problems } = checkSchema(call.raw);
  const written = (args?.fields ?? []).map((item) => String(item?.key ?? ''));
  const injectedKeys = INJECTED_STATE.split('\n').map((line) => line.split(':')[0].trim());
  // 未注入的键与拼写不符的键都落在这里：对模型来说两者都"看不见"。
  const outsideInjection = written.filter((key) => !injectedKeys.includes(key));
  const declaredButUnseen = INJECT_DECLARED.filter((key) => !injectedKeys.includes(key));
  const untouched = declaredButUnseen.filter((key) => !written.includes(key));
  const unchanged = injectedKeys.filter((key) => written.includes(key) && !isChangedByUser(key));

  report('C. 注入即准入', [
    `原始参数: ${call.raw}`,
    `思考模式: ${message.reasoning_content ? '端点返回了 reasoning_content，工具调用照常' : '未返回 reasoning_content'}`,
    `schema 违规: ${problems.length === 0 ? '无' : problems.join('; ')}`,
    `C1/C2 写回了注入文本之外的键: ${outsideInjection.length === 0 ? '无' : outsideInjection.join('、')}`,
    `C1 声明但未注入的字段（${declaredButUnseen.join('、') || '无'}）未被触碰: ${
      untouched.length === declaredButUnseen.length ? '是' : `否（写了 ${declaredButUnseen.filter((key) => written.includes(key)).join('、')}）`
    }`,
    `C3 未重复提交未变化的字段（${unchanged.join('、') || '无'}）: ${unchanged.length === 0 ? '是' : '否'}`,
    `结论: ${
      problems.length === 0 && outsideInjection.length === 0 && unchanged.length === 0
        ? '注入文本足以支撑正确的增量写回'
        : '需要复核'
    }`,
  ]);
}

/** 用户这句话只改变了地点与时间，没提日期。 */
function isChangedByUser(key) {
  return key !== '环境/日期';
}

await probeSchema();
await probeCorrection();
await probeInjection();
