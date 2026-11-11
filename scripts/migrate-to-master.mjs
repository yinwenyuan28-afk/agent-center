// 从现有各 agent 配置里提取 provider，生成真实母版。
//
// 提取规则（按用户确认的三条）：
// 1. 只收有明文 key、能真正注入的 provider
// 2. deepseek 用环境变量 DEEPSEEK_API_KEY 那把
// 3. 不收 ZCode 内置模板（builtin:* 开头的）
//
// 安全：生成的母版含明文 key，脚本自己负责把它写进 .gitignore。
//
// 用法：node scripts/migrate-to-master.mjs

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const HOME = 'C:/Users/leaf';
const OUT = join(HOME, '.agents/providers.toml');

/** 算指纹，用于报告和去重，不打印原文。 */
const fp = (s) => createHash('sha256').update(s).digest('hex').slice(0, 8);

/** 安全读 JSON。 */
function readJson(p) {
  try {
    return JSON.parse(readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

// ---- 收集 ----

/** 收集结果：id -> { name, base_url, kind, api_key, models, agents, source } */
const collected = new Map();

/** 归一化 base_url 用于去重比较。 */
const normUrl = (u) =>
  String(u || '').trim().replace(/\/+$/, '').replace(/^https?:\/\//, '').toLowerCase();

/** 添加或合并一条。同一个 base_url 出现多次时，合并 agents 列表。 */
function add(entry) {
  const key = normUrl(entry.base_url) || entry.id;
  const existing = collected.get(key);

  if (existing) {
    // 合并 agents
    for (const a of entry.agents) {
      if (!existing.agents.includes(a)) existing.agents.push(a);
    }
    // 合并 models
    for (const m of entry.models) {
      if (!existing.models.includes(m)) existing.models.push(m);
    }
    // key 不一致时保留第一个并警告
    if (existing.api_key !== entry.api_key) {
      console.log(
        `  ! ${entry.id} 的 key 与已有条目不一致（${fp(existing.api_key)} vs ${fp(entry.api_key)}），保留前者`,
      );
    }
  } else {
    collected.set(key, { ...entry });
  }
}

// ---- 1. Claude ----
const claudeSettings = readJson(join(HOME, '.claude/settings.json'));
if (claudeSettings?.env) {
  const e = claudeSettings.env;
  const key = e.ANTHROPIC_AUTH_TOKEN || e.ANTHROPIC_API_KEY;
  const base = e.ANTHROPIC_BASE_URL;
  if (key && base) {
    // 从 base_url 猜一个 id
    const id = base.includes('commandcode') ? 'command' : 'claude-custom';
    const models = [
      e.ANTHROPIC_DEFAULT_OPUS_MODEL,
      e.ANTHROPIC_DEFAULT_SONNET_MODEL,
      e.ANTHROPIC_DEFAULT_HAIKU_MODEL,
    ].filter(Boolean);

    add({
      id,
      name: id === 'command' ? 'CommandCode' : 'Claude 自定义端点',
      base_url: base,
      kind: 'anthropic',
      api_key: key,
      models: [...new Set(models)],
      agents: ['claude'],
      source: 'claude/settings.json',
    });
  }
}

// ---- 2. Pi ----
const piAuth = readJson(join(HOME, '.pi/agent/auth.json'));
const piModels = readJson(join(HOME, '.pi/agent/models.json'));
// models-store.json 是 Pi 从远端目录拉的缓存，里面有 baseUrl。
// 有些 provider 只在 auth.json 里有 key、在 models.json 里没定义，
// baseUrl 得从这里补。
const piStore = readJson(join(HOME, '.pi/agent/models-store.json'));

/**
 * 从三个来源查一个 provider 的 baseUrl：
 * 1. models.json（用户手写，最可信）
 * 2. models-store.json（远端目录缓存）
 * 3. 都没有就返回空串
 */
function piBaseUrl(id) {
  const direct = piModels?.providers?.[id]?.baseUrl;
  if (direct) return direct;

  const cached = piStore?.[id]?.models;
  if (Array.isArray(cached)) {
    const withUrl = cached.find((m) => m?.baseUrl);
    if (withUrl) return withUrl.baseUrl;
  }
  return '';
}

/** 从 models-store 缓存里拿模型 id 列表。 */
function piModelsFromStore(id) {
  const cached = piStore?.[id]?.models;
  if (!Array.isArray(cached)) return [];
  return cached.map((m) => m?.id).filter(Boolean);
}

if (piAuth) {
  for (const [id, v] of Object.entries(piAuth)) {
    const key = v?.key;
    if (!key || typeof key !== 'string' || key.length < 16) continue;

    // deepseek 特殊处理：用户选择用环境变量那把，所以跳过 pi 里的
    if (id === 'deepseek') {
      continue;
    }

    const def = piModels?.providers?.[id];
    const base = piBaseUrl(id);
    if (!base) {
      console.log(`  - 跳过 pi/${id}：三个来源都查不到 baseUrl`);
      continue;
    }

    // 模型列表：优先 models.json，没有就从缓存拿
    let models = (def?.models || []).map((m) => m.id).filter(Boolean);
    if (models.length === 0) models = piModelsFromStore(id);

    const api = def?.api || '';
    const kind = api.includes('anthropic') ? 'anthropic' : 'openai-compatible';

    add({
      id,
      name: def?.name || id,
      base_url: base,
      kind,
      api_key: key,
      models,
      agents: ['pi'],
      source: `pi/auth.json (${id})`,
    });
  }
}

// ---- 3. ZCode ----
const zcodeCfg = readJson(join(HOME, '.zcode/v2/config.json'));

/**
 * 把 ZCode 的键换成可读 id。
 *
 * ZCode 用户自建的 provider 键是 UUID，直接拿来当母版 id 会很难读。
 * 用 provider 的 name 字段转成 kebab-case；转不出来就保留原键。
 */
function readableId(rawKey, name) {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-/i.test(rawKey)) return rawKey;
  const slug = String(name || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return slug || rawKey.slice(0, 8);
}

if (zcodeCfg?.provider) {
  for (const [id, p] of Object.entries(zcodeCfg.provider)) {
    // 跳过内置模板
    if (id.startsWith('builtin:')) continue;

    const key = p?.options?.apiKey;
    const base = p?.options?.baseURL;
    if (!key || key.length < 16 || !base) continue;

    add({
      id: readableId(id, p.name),
      name: p.name || id,
      base_url: base,
      kind: p.kind === 'anthropic' ? 'anthropic' : 'openai-compatible',
      api_key: key,
      models: Object.keys(p.models || {}),
      agents: ['zcode'],
      source: `zcode/config.json`,
    });
  }
}

// ---- 4. opencode ----
const ocCfg = readJson(join(HOME, '.config/opencode/opencode.json'));
const ocAuth = readJson(join(HOME, '.local/share/opencode/auth.json'));

if (ocAuth) {
  for (const [id, v] of Object.entries(ocAuth)) {
    const key = v?.key;
    if (!key || typeof key !== 'string' || key.length < 16) continue;

    const def = ocCfg?.providers?.[id] || ocCfg?.provider?.[id];
    const base =
      def?.settings?.baseURL || def?.options?.baseURL || '';
    if (!base) {
      console.log(`  - 跳过 opencode/${id}：配置里没有 baseURL`);
      continue;
    }

    const models = Object.keys(def?.models || {});
    const pkg = def?.package || '';
    const kind = pkg.includes('anthropic') ? 'anthropic' : 'openai-compatible';

    add({
      id,
      name: def?.name || id,
      base_url: base,
      kind,
      api_key: key,
      models,
      agents: ['opencode'],
      source: `opencode auth.json (${id})`,
    });
  }
}

// ---- 5. deepseek：用环境变量那把（用户选择）----
const dsKey = process.env.DEEPSEEK_API_KEY;
if (dsKey && dsKey.length >= 16) {
  add({
    id: 'deepseek',
    name: 'DeepSeek',
    base_url: 'https://api.deepseek.com',
    kind: 'openai-compatible',
    api_key: dsKey,
    models: ['deepseek-chat'],
    agents: ['pi'],
    source: '环境变量 DEEPSEEK_API_KEY',
  });
  console.log(`  + deepseek 用环境变量那把（${fp(dsKey)}）`);
} else {
  console.log('  ! DEEPSEEK_API_KEY 未设置，deepseek 无法收录');
}

// ---- 生成 TOML ----
const list = [...collected.values()];

/** TOML 字符串转义。 */
function q(s) {
  return String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

let toml = `# provider 统一管理母版
#
# 这个文件含有明文 API key，已加入 .gitignore，不要提交到 git。
#
# 由 agentcenter 于 ${new Date().toISOString().slice(0, 10)} 从现有配置提取生成。
# 提取规则：只收有明文 key 且能注入的 provider；跳过 ZCode 内置模板与 OAuth 令牌。
#
# 修改后执行 "agentcenter providers diff" 预览，再 "agentcenter providers sync" 注入。

`;

for (const p of list) {
  toml += `[[provider]]\n`;
  toml += `id = "${q(p.id)}"\n`;
  toml += `name = "${q(p.name)}"\n`;
  toml += `base_url = "${q(p.base_url)}"\n`;
  toml += `kind = "${p.kind}"\n`;
  toml += `api_key = "${q(p.api_key)}"\n`;
  if (p.models.length) {
    toml += `models = [${p.models.map((m) => `"${q(m)}"`).join(', ')}]\n`;
  } else {
    toml += `models = []\n`;
  }
  toml += `agents = [${p.agents.map((a) => `"${a}"`).join(', ')}]\n`;
  toml += `note = "来自 ${q(p.source)}"\n`;
  toml += `\n`;
}

mkdirSync(join(HOME, '.agents'), { recursive: true });
writeFileSync(OUT, toml, 'utf8');

// 确保 .gitignore 有这一行
const gi = join(HOME, '.agents/.gitignore');
let giText = existsSync(gi) ? readFileSync(gi, 'utf8') : '';
if (!/^\s*\/?providers\.toml\s*$/m.test(giText)) {
  giText += '\n# provider 母版含明文 key，禁止提交\nproviders.toml\n';
  writeFileSync(gi, giText, 'utf8');
  console.log('\n已把 providers.toml 加入 .gitignore');
}

// ---- 报告 ----
console.log(`\n=== 已生成 ${OUT} ===\n`);
console.log(`${list.length} 个 provider：\n`);
for (const p of list) {
  console.log(`  ${p.id.padEnd(14)} ${p.name}`);
  console.log(`  ${''.padEnd(14)} ${p.base_url}`);
  console.log(
    `  ${''.padEnd(14)} key ${String(p.api_key.length).padStart(3)} 字符 ${fp(p.api_key)}  →  ${p.agents.join(', ')}`,
  );
  if (p.models.length) {
    console.log(`  ${''.padEnd(14)} models: ${p.models.slice(0, 3).join(', ')}${p.models.length > 3 ? ' …' : ''}`);
  }
  console.log('');
}
