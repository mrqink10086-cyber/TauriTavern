// 把本地模型当施工方用。
//
// 读一份 prompt 文件（可附带若干上下文文件），POST 到本地 OpenAI 兼容端点，
// 把回复原样写到输出文件。串行调用，一次一份产出，便于逐份抽查。
//
// 用法：
//   node scripts/local-llm-call.mjs --prompt <file> --out <file> \
//        [--context <file> ...] [--model qwen3.8-27b] \
//        [--base http://127.0.0.1:10086/v1] [--temperature 0.2] [--max-tokens 16000]
//
// 上下文文件会被包进 ``` 围栏并带上路径，模型能分清每块的来源。

import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

function arg(name, fallback) {
    const index = process.argv.indexOf(`--${name}`);
    if (index === -1) {
        return fallback;
    }
    return process.argv[index + 1] ?? fallback;
}

function allArgs(name) {
    const values = [];
    for (let index = 0; index < process.argv.length; index += 1) {
        if (process.argv[index] === `--${name}`) {
            values.push(process.argv[index + 1]);
        }
    }
    return values.filter(Boolean);
}

const promptPath = arg('prompt');
const outPath = arg('out');
if (!promptPath || !outPath) {
    console.error('usage: node scripts/local-llm-call.mjs --prompt <file> --out <file> [--context <file> ...]');
    process.exit(1);
}

const base = arg('base', process.env.LOCAL_LLM_BASE ?? 'http://127.0.0.1:10086/v1');
const model = arg('model', process.env.LOCAL_LLM_MODEL ?? 'qwen3.8-27b');
const temperature = Number(arg('temperature', '0.2'));
const maxTokens = Number(arg('max-tokens', '16000'));

const prompt = readFileSync(promptPath, 'utf8');
const contexts = allArgs('context').map((file) => {
    const language = path.extname(file).replace('.', '') || 'text';
    return `### 文件：${file}\n\n\`\`\`${language}\n${readFileSync(file, 'utf8')}\n\`\`\``;
});

const user = [prompt, ...contexts].join('\n\n');

const body = {
    model,
    messages: [
        {
            role: 'system',
            content: '你是一个只交付产出的施工方。严格按用户给的规格写代码，不要寒暄、不要解释思路、不要复述需求。输出里只放被要求交付的内容。',
        },
        { role: 'user', content: user },
    ],
    temperature,
    max_tokens: maxTokens,
};
// Qwen3 系默认开思考，思考会把 max_tokens 吃光导致 content 为空。
// 这一步是纯代码转写，不需要思维链，默认关掉；服务器不认这个字段时用 --think 回退。
if (process.argv.includes('--think')) {
    body.chat_template_kwargs = { enable_thinking: true };
} else {
    body.chat_template_kwargs = { enable_thinking: false };
}

const response = await fetch(`${base}/chat/completions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
});

if (!response.ok) {
    const detail = await response.text();
    throw new Error(`HTTP ${response.status}: ${detail.slice(0, 600)}`);
}

const payload = await response.json();
writeFileSync(`${outPath}.json`, JSON.stringify(payload, null, 2), 'utf8');

const message = payload?.choices?.[0]?.message ?? {};
const finishReason = payload?.choices?.[0]?.finish_reason ?? '?';
// Qwen3 系模型可能把预算烧在思考上：content 为空而 reasoning_content 有内容。
// 这种情况必须说清楚，不能当成"模型没产出"就往下走。
const reasoning = message.reasoning_content ?? message.reasoning ?? '';
let text = message.content ?? '';
if (!text && reasoning) {
    throw new Error(
        `the model produced reasoning but no content (finish_reason=${finishReason}, reasoning ${reasoning.length} chars); ` +
            'raw payload written to ' + `${outPath}.json`,
    );
}
if (!text) {
    throw new Error(`the model returned empty content (finish_reason=${finishReason}); raw payload written to ${outPath}.json`);
}
writeFileSync(outPath, text, 'utf8');

const usage = payload?.usage ?? {};
console.log(`wrote ${outPath} (${text.length} chars; finish ${finishReason}; prompt ${usage.prompt_tokens ?? '?'} / completion ${usage.completion_tokens ?? '?'} tokens)`);
