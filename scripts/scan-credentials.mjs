// 扫描各 agent 的凭证存储位置与结构。
//
// 安全约定：**绝不输出任何密钥值**。所有疑似密钥的字段统一打印为
// <已隐藏，长度 N>，只保留字段名和文件路径用于设计参考。
//
// 用法：node scripts/scan-credentials.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { homedir } from 'node:os';

const home = homedir();

/** 疑似存放密钥的文件名（大小写不敏感匹配）。 */
const SUSPECT_NAMES = [
  'auth.json',
  'credentials.json',
  '.credentials.json',
  'config.json',
  'config.toml',
  'settings.json',
  'settings.local.json',
  'mcp.json',
  '.env',
  'model.env',
];

/** 字段名里出现这些词就当作密钥，不打印值。 */
const SECRET_HINTS = [
  'key',
  'token',
  'secret',
  'password',
  'passwd',
  'auth',
  'credential',
  'session',
  'cookie',
];

/** 要扫的 agent 配置目录。 */
const TARGETS = [
  '.claude',
  '.codex',
  '.zcode',
  '.cursor',
  '.qoder',
  '.qoder-cn',
  '.qoderworkcn',
  '.config/opencode',
  '.pi/agent',
  '.dsh',
  '.workbuddy',
  '.codebuddy',
  '.trae-cn',
  '.gemini',
];

function isSecretKey(k) {
  const low = String(k).toLowerCase();
  return SECRET_HINTS.some((h) => low.includes(h));
}

/** 把任意值压成"结构描述"，绝不泄露内容。 */
function describe(v, keyPath = '') {
  if (v === null) return 'null';
  if (typeof v === 'string') {
    // 结构性的短值（如 type: "oauth"、路径）照常显示；
    // 长串或字段名像密钥的一律隐藏
    const looksSecret =
      isSecretKey(keyPath) || v.length > 40 || /^[A-Za-z0-9_\-.]{24,}$/.test(v);
    if (looksSecret) return `<已隐藏，长度 ${v.length}>`;
    return JSON.stringify(v);
  }
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    return `[${v.length} 项: ${describe(v[0], keyPath)}${v.length > 1 ? ', …' : ''}]`;
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    const parts = keys.slice(0, 8).map((k) => `${k}: ${describe(v[k], k)}`);
    return `{ ${parts.join(', ')}${keys.length > 8 ? ', …' : ''} }`;
  }
  return typeof v;
}

/** 递归列出目录下所有匹配的文件，限制深度避免扫进 node_modules。 */
function findFiles(dir, depth = 0, out = []) {
  if (depth > 2 || !existsSync(dir)) return out;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const full = join(dir, e.name);
    if (e.isDirectory()) {
      // 跳过体积大且与凭证无关的目录
      if (['node_modules', 'cache', 'projects', 'sessions', 'history', 'logs', 'plugins'].includes(e.name)) {
        continue;
      }
      findFiles(full, depth + 1, out);
    } else if (SUSPECT_NAMES.includes(e.name.toLowerCase())) {
      out.push(full);
    }
  }
  return out;
}

console.log('=== 各 agent 的凭证文件 ===\n');

for (const t of TARGETS) {
  const dir = join(home, t.replace(/\//g, '\\'));
  if (!existsSync(dir)) continue;

  const files = findFiles(dir);
  if (files.length === 0) continue;

  console.log(`## ${t}`);
  for (const f of files) {
    const st = statSync(f);
    console.log(`  ${relative(home, f)}  (${st.size} 字节)`);

    // 只解析小的 json/toml 文本文件
    if (st.size > 200_000) {
      console.log('    文件过大，跳过解析');
      continue;
    }
    const name = f.toLowerCase();
    if (!name.endsWith('.json') && !name.endsWith('.toml') && !name.endsWith('.env')) continue;

    let text;
    try {
      text = readFileSync(f, 'utf8');
    } catch {
      continue;
    }

    if (name.endsWith('.json')) {
      try {
        const obj = JSON.parse(text);
        console.log('    结构: ' + describe(obj));
      } catch {
        console.log('    不是合法 JSON');
      }
    } else if (name.endsWith('.env')) {
      // .env 只列变量名
      const names = text
        .split('\n')
        .map((l) => l.trim())
        .filter((l) => l && !l.startsWith('#'))
        .map((l) => l.split('=')[0]);
      console.log('    变量名: ' + (names.join(', ') || '（无）'));
    } else {
      // TOML 只找含 key/token 的行，值隐藏
      const hits = text
        .split('\n')
        .filter((l) => /api_key|token|secret|password/i.test(l))
        .map((l) => l.split('=')[0].trim() + ' = <已隐藏>');
      if (hits.length) console.log('    疑似密钥字段: ' + hits.join('; '));
    }
  }
  console.log('');
}

console.log('=== 相关环境变量（只看名字，不看值）===\n');
const envHits = Object.keys(process.env)
  .filter((k) => /API_KEY|APIKEY|TOKEN|SECRET|BASE_URL|ANTHROPIC|OPENAI|DEEPSEEK|GEMINI|GLM|QWEN|MOONSHOT/i.test(k))
  .sort();
console.log(envHits.length ? envHits.map((k) => '  ' + k).join('\n') : '  （无）');
