// 扫描本机所有 agent 的 MCP 配置结构。
//
// 只报结构和字段名，不输出任何密钥值（MCP 的 env 里常带 API key）。
//
// 用法：node scripts/scan-mcp.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const HOME = 'C:/Users/leaf';

/** 疑似密钥的字段名。 */
const SECRET_RE = /key|token|secret|password|credential|bearer/i;

function hide(obj, keyPath = '', depth = 0) {
  if (depth > 3) return '…';
  if (obj === null) return 'null';
  if (typeof obj === 'string') {
    if (SECRET_RE.test(keyPath) || obj.length > 80) return `<str len=${obj.length}>`;
    return JSON.stringify(obj);
  }
  if (typeof obj === 'number' || typeof obj === 'boolean') return String(obj);
  if (Array.isArray(obj)) {
    if (obj.length === 0) return '[]';
    return `[${obj.length} 项: ${hide(obj[0], keyPath, depth + 1)}${obj.length > 1 ? ', …' : ''}]`;
  }
  if (typeof obj === 'object') {
    const keys = Object.keys(obj);
    return `{ ${keys.slice(0, 10).map((k) => k).join(', ')}${keys.length > 10 ? ', …' : ''} }`;
  }
  return typeof obj;
}

/** 打印一个 MCP server 条目的完整字段结构。 */
function describeServer(name, s) {
  if (!s || typeof s !== 'object') {
    console.log(`    ${name}: <非对象>`);
    return;
  }
  const fields = Object.entries(s);
  console.log(`    ${name}:`);
  for (const [k, v] of fields) {
    let shown;
    if (k === 'env' && v && typeof v === 'object') {
      // env 只列变量名和值长度
      const names = Object.entries(v).map(([ek, ev]) =>
        `${ek}=${SECRET_RE.test(ek) ? `<len ${String(ev).length}>` : JSON.stringify(ev)}`,
      );
      shown = `{ ${names.join(', ')} }`;
    } else if (k === 'args' && Array.isArray(v)) {
      shown = JSON.stringify(v.map((a) => (String(a).length > 40 ? `<len ${String(a).length}>` : a)));
    } else {
      shown = hide(v, k);
    }
    console.log(`      ${k} = ${shown}`);
  }
}

/** 读 JSON，失败返回 null。 */
function readJson(p) {
  try {
    return JSON.parse(readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

// ---- 一个 agent 的 MCP 配置位置与结构 ----

const TARGETS = [
  {
    agent: 'claude',
    paths: [
      { p: join(HOME, '.claude.json'), pick: (j) => j.mcpServers, note: '顶层 mcpServers' },
      { p: join(HOME, '.claude/settings.json'), pick: (j) => j.mcpServers, note: 'settings.json' },
    ],
  },
  {
    agent: 'zcode',
    paths: [
      { p: join(HOME, '.zcode/cli/config.json'), pick: (j) => j.mcpServers ?? j.mcp, note: 'cli/config.json' },
    ],
  },
  {
    agent: 'cursor',
    paths: [{ p: join(HOME, '.cursor/mcp.json'), pick: (j) => j.mcpServers, note: '顶层 mcpServers' }],
  },
  {
    agent: 'codex',
    paths: [
      { p: join(HOME, '.codex/config.toml'), toml: 'mcp_servers', note: 'mcp_servers 表' },
    ],
  },
  {
    agent: 'pi',
    paths: [{ p: join(HOME, '.pi/agent/mcp.json'), pick: (j) => j.mcpServers, note: 'mcpServers' }],
  },
  {
    agent: 'opencode',
    paths: [
      { p: join(HOME, '.config/opencode/opencode.json'), pick: (j) => j.mcp, note: 'mcp 段' },
      { p: join(HOME, '.config/opencode/opencode.jsonc'), pick: (j) => j.mcp, note: 'opencode.jsonc' },
    ],
  },
  {
    agent: 'workbuddy',
    paths: [
      { p: join(HOME, '.workbuddy/connectors/default/mcp.json'), pick: (j) => j.mcpServers, note: 'mcpServers' },
      { p: join(HOME, '.workbuddy/mcp-tool-list.json'), pick: (j) => j, note: 'mcp-tool-list（缓存）' },
    ],
  },
  {
    agent: 'qoder',
    paths: [{ p: join(HOME, '.qoder/settings.json'), pick: (j) => j.mcpServers, note: 'settings.json' }],
  },
  {
    agent: 'dsh',
    paths: [
      { p: join(HOME, '.dsh/profiles/desktop/cordis.patch.yml'), yaml: 'mcp', note: 'cordis.patch.yml' },
    ],
  },
];

for (const t of TARGETS) {
  console.log(`\n===== ${t.agent} =====`);

  let foundAny = false;
  for (const spec of t.paths) {
    if (!existsSync(spec.p)) continue;
    foundAny = true;

    let size = 0;
    try {
      size = statSync(spec.p).size;
    } catch {
      continue;
    }

    console.log(`\n  --- ${spec.p.replace(HOME, '~')} (${size} 字节) · ${spec.note} ---`);

    if (spec.toml) {
      // Codex 的 TOML：解析 mcp_servers 表
      try {
        const text = readFileSync(spec.p, 'utf8');
        const lines = text.split('\n');
        const hits = lines
          .map((l, i) => ({ l, i }))
          .filter((x) => /^\[mcp_servers\.[^\]]+\]/.test(x.l.trim()));
        if (hits.length === 0) {
          console.log('    没有 mcp_servers 表');
        } else {
          console.log(`    共 ${hits.length} 个 mcp_servers 表：`);
          for (const h of hits) {
            console.log(`      ${h.l.trim()}`);
            // 打印这个表下的字段
            for (let j = h.i + 1; j < lines.length; j++) {
              const ln = lines[j];
              if (ln.trim().startsWith('[')) break;
              if (!ln.trim() || ln.trim().startsWith('#')) continue;
              const [k, ...rest] = ln.trim().split('=');
              const v = rest.join('=').trim();
              console.log(`        ${k.trim()} = ${SECRET_RE.test(k) ? '<隐藏>' : v}`);
            }
          }
        }
      } catch (e) {
        console.log('    解析失败：', e.message);
      }
      continue;
    }

    if (spec.yaml) {
      const text = readFileSync(spec.p, 'utf8');
      const lines = text.split('\n');
      const mcpIdx = lines.findIndex((l) => /^mcp:/.test(l));
      if (mcpIdx < 0) {
        console.log('    没有 mcp 段');
      } else {
        console.log(`    找到 mcp: 段（第 ${mcpIdx + 1} 行），随后几行：`);
        for (let i = mcpIdx; i < Math.min(mcpIdx + 12, lines.length); i++) {
          const l = lines[i];
          console.log(`      ${l.replace(SECRET_RE, '<隐藏字段>')}`);
        }
      }
      continue;
    }

    const json = readJson(spec.p);
    if (!json) {
      console.log('    非法 JSON 或读取失败');
      continue;
    }

    const block = spec.pick(json);
    if (!block || typeof block !== 'object') {
      console.log('    没有这个段');
      continue;
    }

    const names = Object.keys(block);
    if (names.length === 0) {
      console.log('    段存在但为空');
      continue;
    }

    console.log(`    共 ${names.length} 个 server：`);
    for (const n of names) {
      describeServer(n, block[n]);
    }
  }

  if (!foundAny) console.log('  没有找到任何 MCP 配置文件');
}

// ---- 额外：看 claude 是否在 ~/.claude.json 里（常见位置）----
console.log('\n===== 补充检查 =====');
const claudeJson = join(HOME, '.claude.json');
console.log(`~/.claude.json 存在：${existsSync(claudeJson)}`);
if (existsSync(claudeJson)) {
  const j = readJson(claudeJson);
  if (j) {
    const topKeys = Object.keys(j);
    console.log(`  顶层键（前 15 个）：${topKeys.slice(0, 15).join(', ')}`);
    if (j.mcpServers) {
      console.log(`  顶层 mcpServers：${Object.keys(j.mcpServers).join(', ')}`);
    }
    // 项目级 mcpServers
    if (j.projects) {
      const projCount = Object.keys(j.projects).length;
      console.log(`  projects 数：${projCount}`);
      for (const [k, v] of Object.entries(j.projects).slice(0, 3)) {
        if (v?.mcpServers) {
          console.log(`    ${k}: mcpServers=${Object.keys(v.mcpServers).join(',')}`);
        }
      }
    }
  }
}
