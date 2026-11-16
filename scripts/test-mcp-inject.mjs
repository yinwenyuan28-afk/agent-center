// 在副本上验证 MCP 注入器。
//
// 用 AGENTCENTER_HOME 重定向，真实配置不受影响。
//
// 重点验证这个分支修掉的两条真 bug：
//   1. Codex 不该写 type（codex doctor 报未知字段，本机原来每次启动报 4 条）
//   2. Codex 的远程 server 头字段必须叫 http_headers，不是 headers（原来被静默忽略）
//
// 用法：node scripts/test-mcp-inject.mjs

import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/mcp-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? '  OK ' : '  XX '} ${name}${detail ? ` -- ${detail}` : ''}`);
  if (!ok) failures += 1;
}

function run(args) {
  return execFileSync(EXE, args, {
    env: {
      ...process.env,
      AGENTCENTER_HOME: TEST,
      // Hermes / DSH 读自己的环境变量，AGENTCENTER_HOME 管不到它们。
      // 指向不存在的目录，避免测试碰真实配置
      HERMES_HOME: join(TEST, 'hermes-not-installed'),
      DSH_HOME: join(TEST, 'dsh-not-installed'),
    },
    encoding: 'utf8',
  });
}

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

rmSync(TEST, { recursive: true, force: true });
mkdirSync(TEST, { recursive: true });

const staged = {
  claudeJson: stage(join(REAL, '.claude.json'), join(TEST, '.claude.json')),
  codexToml: stage(join(REAL, '.codex/config.toml'), join(TEST, '.codex/config.toml')),
  cursorMcp: stage(join(REAL, '.cursor/mcp.json'), join(TEST, '.cursor/mcp.json')),
  piMcp: stage(join(REAL, '.pi/agent/mcp.json'), join(TEST, '.pi/agent/mcp.json')),
  ocJson: stage(
    join(REAL, '.config/opencode/opencode.json'),
    join(TEST, '.config/opencode/opencode.json'),
  ),
  ocJsonc: stage(
    join(REAL, '.config/opencode/opencode.jsonc'),
    join(TEST, '.config/opencode/opencode.jsonc'),
  ),
  qoderSettings: stage(join(REAL, '.qoder/settings.json'), join(TEST, '.qoder/settings.json')),
  wbMcp: stage(
    join(REAL, '.workbuddy/connectors/default/mcp.json'),
    join(TEST, '.workbuddy/connectors/default/mcp.json'),
  ),
};
console.log('已复制真实配置：', staged);

const AGENTS = ['claude', 'codex', 'cursor', 'pi', 'opencode', 'qoder', 'workbuddy'];

// 母版：一个已存在（context7）+ 一个新增 stdio + 一个远程带授权头
const master = `[[server]]
id = "context7"
name = "Context7"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ${JSON.stringify(AGENTS)}

[[server]]
id = "brand-new"
name = "新加的"
transport = "stdio"
command = "node"
args = ["server.js"]
enabled = true
agents = ${JSON.stringify(AGENTS)}

[[server]]
id = "remote-auth"
name = "带鉴权的远程"
transport = "http"
url = "https://example.com/mcp"
enabled = true
agents = ${JSON.stringify(AGENTS)}
[server.headers]
Authorization = "Bearer test-token-12345"
`;
mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents/mcp.toml'), master, 'utf8');

console.log('\n=== 1. dry-run 不写盘 ===');
const dry = run(['mcp', 'sync', '--dry-run']);
check('dry-run 有输出', dry.trim().length > 0);
check(
  'dry-run 之后 codex 文件没变',
  readFileSync(join(TEST, '.codex/config.toml'), 'utf8') ===
    readFileSync(join(REAL, '.codex/config.toml'), 'utf8'),
);

console.log('\n=== 2. 真写 ===');
const out = run(['mcp', 'sync']);
console.log(
  out
    .trim()
    .split('\n')
    .slice(0, 8)
    .map((l) => `    ${l}`)
    .join('\n'),
);

console.log('\n=== 3. Codex：不该有 type，远程该用 http_headers ===');
const codexText = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');
// 只检查我们管的那三条。不在母版里的（sequential-thinking 等）保持原样不动 ——
// 「只操作母版里有的 id」这条规则对 Codex 同样成立
const managed = ['context7', 'brand-new', 'remote-auth'];
const managedText = managed
  .map((id) => {
    const m = codexText.match(new RegExp(`\\[mcp_servers\\.${id}\\]([\\s\\S]*?)(?=\\n\\[|$)`));
    return m ? m[1] : '';
  })
  .join('\n');
check('我们管的条目里没有 type =', !/^\s*type = /m.test(managedText), managedText.trim().slice(0, 80));
check('远程 server 用了 http_headers', codexText.includes('http_headers'));
check('没有裸 headers 行', !/^\s*headers = \{/m.test(codexText));
check('新增的 brand-new 已写入', codexText.includes('[mcp_servers.brand-new]'));
check(
  'Codex 不包 cmd /c（它自己 mcp add 也不包）',
  /\[mcp_servers\.brand-new\][\s\S]*?command = "node"/.test(codexText),
  codexText.match(/\[mcp_servers\.brand-new\][\s\S]{0,60}/)?.[0] ?? '',
);
check(
  'codex 自己的 env_vars 还在（node_repl）',
  codexText.includes('env_vars'),
  '这是「只清另一种传输的专属字段」的直接后果',
);
check(
  '不在母版里的 sequential-thinking 原样保留',
  /\[mcp_servers\.sequential-thinking\]\s*\ntype = "stdio"/.test(codexText),
);

console.log('\n=== 4. Claude：npx 要包 cmd /c ===');
const claude = JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
const bn = claude.mcpServers['brand-new'];
check('brand-new 的 command 被包成 cmd', bn?.command === 'cmd', `实际 ${bn?.command}`);
check('args 以 /c 开头', bn?.args?.[0] === '/c', JSON.stringify(bn?.args));
check('type 是 stdio', bn?.type === 'stdio');
check('projects 段没被碰', !!claude.projects);

console.log('\n=== 5. pi：保留 imports，不写 SSE ===');
const pi = JSON.parse(readFileSync(join(TEST, '.pi/agent/mcp.json'), 'utf8'));
check(
  'imports 还在',
  Array.isArray(pi.imports) && pi.imports.includes('claude-code'),
  JSON.stringify(pi.imports),
);
check('brand-new 已写入', !!pi.mcpServers['brand-new']);
check('没有 sse 条目', !Object.values(pi.mcpServers).some((v) => v.type === 'sse'));

console.log('\n=== 6. opencode：mcp 写进原本有 mcp 的那份 ===');
const ocJson = readFileSync(join(TEST, '.config/opencode/opencode.json'), 'utf8');
const ocJsonc = readFileSync(join(TEST, '.config/opencode/opencode.jsonc'), 'utf8');
check('opencode.json 里有 brand-new', ocJson.includes('brand-new'));
check('没有把 mcp 塞进 .jsonc', !ocJsonc.includes('"mcp"'));
check('provider 的 apiKey 还在 jsonc 里', ocJsonc.includes('apiKey'));
check('opencode 的 command 是数组', /"command":\s*\[/.test(ocJson));

console.log('\n=== 7. WorkBuddy：83 个 connector 一个都没少 ===');
const wb = JSON.parse(readFileSync(join(TEST, '.workbuddy/connectors/default/mcp.json'), 'utf8'));
const connectors = Object.keys(wb.mcpServers).filter((k) => k.startsWith('connector:'));
check('connector 数量仍是 83', connectors.length === 83, `实际 ${connectors.length}`);
check('brand-new 已写入', !!wb.mcpServers['brand-new']);
check('brand-new 用 disabled 而不是 enabled', wb.mcpServers['brand-new']?.disabled === false);

console.log('\n=== 8. 幂等：第二次同步零写入 ===');
const second = run(['mcp', 'sync']);
check('第二次没有 [写入] 行', !second.includes('[写入]'));

console.log(`\n${failures === 0 ? '全部通过' : `${failures} 项失败`}`);
process.exit(failures === 0 ? 0 : 1);
