// 扫描 WorkBuddy / Hermes / Qoder / DSH 是否存在 provider / API key 配置。
//
// 只报结构和字段名，绝不输出密钥值。
//
// 用法：node scripts/scan-extra-agents.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const HOME = 'C:/Users/leaf';

/** 疑似密钥的字段名关键词。 */
const SECRET_HINTS = [
  'key', 'token', 'secret', 'password', 'apikey', 'api_key',
  'credential', 'auth', 'bearer', 'access',
];

/** provider 相关的字段名关键词。 */
const PROVIDER_HINTS = [
  'provider', 'model', 'baseurl', 'base_url', 'endpoint', 'api', 'llm', 'vendor',
];

function isSecret(k) {
  const low = String(k).toLowerCase();
  return SECRET_HINTS.some((h) => low.includes(h));
}

function isProviderish(k) {
  const low = String(k).toLowerCase();
  return PROVIDER_HINTS.some((h) => low.includes(h));
}

/** 把一个值压成结构描述，密钥隐藏。 */
function describe(v, keyPath, depth = 0) {
  if (depth > 2) return '…';
  if (v === null) return 'null';
  if (typeof v === 'string') {
    if (isSecret(keyPath) || v.length > 60) return `<字符串 len=${v.length}>`;
    return JSON.stringify(v);
  }
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    return `[${v.length} 项]`;
  }
  if (typeof v === 'object') {
    const keys = Object.keys(v);
    return `{ ${keys.slice(0, 6).map((k) => k).join(', ')}${keys.length > 6 ? ', …' : ''} }`;
  }
  return typeof v;
}

/**
 * 递归找含 provider/model/key 迹象的键。
 *
 * 返回命中路径列表，用于判断这个 agent 有没有可管理的 provider 结构。
 */
function findProviderish(obj, prefix = '', out = []) {
  if (!obj || typeof obj !== 'object') return out;
  if (Array.isArray(obj)) {
    obj.slice(0, 3).forEach((v, i) => findProviderish(v, `${prefix}[${i}]`, out));
    return out;
  }
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (isProviderish(k) || isSecret(k)) {
      out.push({ path, kind: Array.isArray(v) ? 'array' : typeof v, sample: describe(v, k) });
    }
    if (v && typeof v === 'object') findProviderish(v, path, out);
  }
  return out;
}

/** 列出目录下的文件（一层）。 */
function listFiles(dir) {
  try {
    return readdirSync(dir, { withFileTypes: true })
      .filter((e) => e.isFile())
      .map((e) => e.name);
  } catch {
    return [];
  }
}

const TARGETS = [
  { id: 'workbuddy', dir: join(HOME, '.workbuddy') },
  { id: 'workbuddy-ai', dir: join(HOME, '.workbuddy-ai') },
  { id: 'hermes', dir: join(HOME, '.hermes') },
  { id: 'qoder', dir: join(HOME, '.qoder') },
  { id: 'qoder-cn', dir: join(HOME, '.qoder-cn') },
  { id: 'dsh', dir: join(HOME, '.dsh') },
];

for (const t of TARGETS) {
  console.log(`\n===== ${t.id} =====`);

  if (!existsSync(t.dir)) {
    console.log('  目录不存在，未安装');
    continue;
  }

  const files = listFiles(t.dir);
  console.log(`  顶层文件：${files.join(', ') || '（无）'}`);

  // 只检查 JSON 类配置文件
  const candidates = files.filter((f) => /\.(json|toml|yaml|yml)$/i.test(f));

  for (const f of candidates) {
    const full = join(t.dir, f);
    let size = 0;
    try {
      size = statSync(full).size;
    } catch {
      continue;
    }

    console.log(`\n  --- ${f} (${size} 字节) ---`);

    if (size > 300_000) {
      console.log('    文件过大，跳过');
      continue;
    }

    let text;
    try {
      text = readFileSync(full, 'utf8');
    } catch {
      console.log('    读取失败');
      continue;
    }

    if (!f.endsWith('.json')) {
      // TOML/YAML 只看有没有可疑行
      const hits = text
        .split('\n')
        .filter((l) => isProviderish(l) || isSecret(l))
        .slice(0, 8)
        .map((l) => '    ' + l.split('=')[0].trim() + ' = <��隐藏>');
      console.log(hits.length ? hits.join('\n') : '    无 provider 相关行');
      continue;
    }

    let json;
    try {
      json = JSON.parse(text);
    } catch {
      console.log('    非法 JSON');
      continue;
    }

    const hits = findProviderish(json);
    if (hits.length === 0) {
      console.log('    没有 provider / key 相关字段');
    } else {
      // 只显示前 20 条，避免刷屏
      for (const h of hits.slice(0, 20)) {
        console.log(`    ${h.path}  (${h.kind}) = ${h.sample}`);
      }
      if (hits.length > 20) console.log(`    ……还有 ${hits.length - 20} 条`);
    }
  }
}

// 额外的：检查已知的插件缓存目录里有没有 provider 配置
console.log('\n===== 插件缓存里的 provider（qoder/workbuddy）=====');
for (const t of ['.qoder', '.workbuddy', '.codebuddy']) {
  const plugins = join(HOME, t, 'plugins');
  if (!existsSync(plugins)) continue;
  try {
    const subs = readdirSync(plugins, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name);
    console.log(`${t}/plugins: ${subs.join(', ') || '(空)'}`);
  } catch {
    // 忽略
  }
}
