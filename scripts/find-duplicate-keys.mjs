// 检测同一个 API key 是否散落在多个 agent 里。
//
// 只输出 SHA-256 前 8 位指纹用于分组，**绝不打印密钥原文**。
// 这是"统一管理凭证"这个需求是否真有价值的关键验证：
// 如果每个 agent 用的 key 都不一样，那统一管理的意义就只剩展示；
// 如果大量重复，那"发现重复 + 收敛到一处"就是真需求。
//
// 用法：node scripts/find-duplicate-keys.mjs

import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';

const home = homedir();

/** 记录一条凭证。 */
const found = [];

/** 算指纹。只用于比对，不可逆。 */
function fp(s) {
  return createHash('sha256').update(s).digest('hex').slice(0, 8);
}

function push(agent, provider, kind, value, where) {
  if (typeof value !== 'string' || value.length < 16) return;
  found.push({ agent, provider, kind, fp: fp(value), len: value.length, where });
}

/** 安全读 JSON，失败返回 null。 */
function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
}

// --- Claude Code ---
const claudeSettings = readJson(join(home, '.claude', 'settings.json'));
if (claudeSettings?.env) {
  const e = claudeSettings.env;
  const base = e.ANTHROPIC_BASE_URL ?? '(官方)';
  if (e.ANTHROPIC_AUTH_TOKEN) push('claude', base, 'api_key', e.ANTHROPIC_AUTH_TOKEN, 'settings.json');
  if (e.ANTHROPIC_API_KEY) push('claude', base, 'api_key', e.ANTHROPIC_API_KEY, 'settings.json');
}

// --- Codex（OAuth，单独标） ---
const codexAuth = readJson(join(home, '.codex', 'auth.json'));
if (codexAuth) {
  if (codexAuth.OPENAI_API_KEY) push('codex', 'openai', 'api_key', codexAuth.OPENAI_API_KEY, 'auth.json');
  if (codexAuth.tokens?.access_token) {
    push('codex', 'openai-oauth', 'oauth', codexAuth.tokens.access_token, 'auth.json');
  }
}

// --- ZCode ---
const zcodeCfg = readJson(join(home, '.zcode', 'v2', 'config.json'));
if (zcodeCfg?.provider) {
  for (const [id, p] of Object.entries(zcodeCfg.provider)) {
    const key = p?.options?.apiKey;
    const base = p?.options?.baseURL ?? id;
    if (key) push('zcode', String(base), 'api_key', key, `config.json#${id}`);
  }
}
const zcodeCred = readJson(join(home, '.zcode', 'v2', 'credentials.json'));
if (zcodeCred) {
  for (const [k, v] of Object.entries(zcodeCred)) {
    // credentials.json 的 key 名本身带语义，值才是密钥
    if (typeof v !== 'string') continue;
    if (/access_token|api-key|credential|jwt/i.test(k)) {
      push('zcode', k.split(':').slice(0, 2).join(':'), 'oauth', v, 'credentials.json');
    }
  }
}

// --- pi ---
const piAuth = readJson(join(home, '.pi', 'agent', 'auth.json'));
if (piAuth) {
  for (const [provider, v] of Object.entries(piAuth)) {
    if (v?.type === 'api_key' && v.key) push('pi', provider, 'api_key', v.key, 'auth.json');
    if (v?.access) push('pi', provider, 'oauth', v.access, 'auth.json');
  }
}

// --- 环境变量（只看是否设置 + 指纹） ---
const ENV_KEYS = ['DEEPSEEK_API_KEY', 'COMMAND_API_KEY', 'ZCODE_OFFICIAL_ENDPOINT_TOKEN'];
for (const k of ENV_KEYS) {
  if (process.env[k]) push('环境变量', k, 'api_key', process.env[k], `$${k}`);
}

// --- 分组输出 ---
const byFp = new Map();
for (const f of found) {
  if (!byFp.has(f.fp)) byFp.set(f.fp, []);
  byFp.get(f.fp).push(f);
}

console.log('=== 凭证指纹分组 ===\n');
const sorted = [...byFp.entries()].sort((a, b) => b[1].length - a[1].length);

for (const [fingerprint, items] of sorted) {
  const agents = [...new Set(items.map((i) => i.agent))];
  const mark = agents.length > 1 ? '★ 重复' : '  单独';
  console.log(`${mark}  ${fingerprint}  长度 ${items[0].len}  出现在 ${agents.length} 个 agent`);
  for (const i of items) {
    console.log(`         ${i.agent.padEnd(10)} ${i.kind.padEnd(8)} ${i.provider.padEnd(42)} ${i.where}`);
  }
  console.log('');
}

console.log(`共 ${found.length} 条凭证，${sorted.length} 个不同指纹`);

// 按 provider 看是否有同一 provider 用不同 key 的情况
console.log('\n=== 同一 provider 是否用了不同 key ===\n');
const byProvider = new Map();
for (const f of found) {
  if (!byProvider.has(f.provider)) byProvider.set(f.provider, new Set());
  byProvider.get(f.provider).add(f.fp);
}
let inconsistent = 0;
for (const [provider, fps] of byProvider) {
  if (fps.size > 1) {
    inconsistent++;
    console.log(`⚠ ${provider}  有 ${fps.size} 个不同的 key: ${[...fps].join(', ')}`);
  }
}
if (inconsistent === 0) console.log('（没有发现同一 provider 使用多个不同 key 的情况）');
