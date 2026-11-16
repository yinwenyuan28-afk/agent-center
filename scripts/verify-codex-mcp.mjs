// 用 Codex 自己的 `codex doctor` 验证注入器写出的配置没有未知字段。
//
// 这是这个分支最重要的验证：原来的注入器写 `type` 和 `headers`，
// Codex 每次启动都报「忽略 N 个未知配置项」。修完之后应该一条都不报。
//
// 用法：node scripts/verify-codex-mcp.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';
const TEST = 'C:/temp/provider-test/codex-doctor';

let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? '  OK ' : '  XX '} ${name}${detail ? ` -- ${detail}` : ''}`);
  if (!ok) failures += 1;
}

function runCenter(args) {
  return execFileSync(EXE, args, {
    env: {
      ...process.env,
      AGENTCENTER_HOME: TEST,
      HERMES_HOME: join(TEST, 'none'),
      DSH_HOME: join(TEST, 'none'),
    },
    encoding: 'utf8',
  });
}

function codexConfigPath() {
  return join(TEST, '.codex/config.toml');
}

/** 跑 codex doctor --json，取「有没有忽略的 mcp 字段」。
 *
 * doctor 有 warning 时退出码非零（本机网络本来就警告），所以不能用
 * execFileSync 的返回值，要捕获 stdout。Windows 上 codex 是 .cmd，
 * 必须显式走 cmd /c 才找得到。
 */
function codexWarnings() {
  let out;
  try {
    out = execFileSync('cmd', ['/c', 'codex', 'doctor', '--json'], {
      env: { ...process.env, CODEX_HOME: join(TEST, '.codex') },
      encoding: 'utf8',
    });
  } catch (e) {
    // 退出码非零但 stdout 是完整的 JSON
    out = e.stdout;
    if (!out) throw e;
  }
  const ignored = [...out.matchAll(/`(mcp_servers\.[^`]+)` is ignored/g)].map((m) => m[1]);
  return ignored;
}

rmSync(TEST, { recursive: true, force: true });
mkdirSync(join(TEST, '.codex'), { recursive: true });
mkdirSync(join(TEST, '.agents'), { recursive: true });

// 先放一份「旧注入器会写出来的样子」：带 type + 带 headers
writeFileSync(
  codexConfigPath(),
  `[mcp_servers.old-style]
type = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]

[mcp_servers.old-remote]
type = "http"
url = "https://example.com/mcp"
headers = { Authorization = "Bearer x" }
`,
  'utf8',
);

console.log('=== 1. 先确认 codex doctor 能抓到旧写法的两个问题 ===');
const before = codexWarnings();
check(
  'type 被报为未知字段',
  before.some((p) => p.endsWith('.type')),
  before.join(', '),
);
check(
  'headers 被报为未知字段',
  before.some((p) => p.endsWith('.headers')),
  before.join(', '),
);

console.log('\n=== 2. 用注入器覆盖这两条 ===');
writeFileSync(
  join(TEST, '.agents/mcp.toml'),
  `[[server]]
id = "old-style"
name = "old-style"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ["codex"]

[[server]]
id = "old-remote"
name = "old-remote"
transport = "http"
url = "https://example.com/mcp"
enabled = true
agents = ["codex"]
[server.headers]
Authorization = "Bearer x"
`,
  'utf8',
);

runCenter(['mcp', 'sync']);

console.log('\n=== 3. codex doctor 应该一条都不报了 ===');
const after = codexWarnings();
check('没有 mcp_servers 未知字段', after.length === 0, after.join(', '));

console.log('\n=== 4. 写出来的实际内容 ===');
const written = readFileSync(codexConfigPath(), 'utf8');
console.log(
  written
    .trim()
    .split('\n')
    .map((l) => `    ${l}`)
    .join('\n'),
);
check('没有 type', !/^\s*type\s*=/m.test(written));
check('远程用 http_headers', written.includes('http_headers'));

console.log(`\n${failures === 0 ? '全部通过' : `${failures} 项失败`}`);
rmSync(TEST, { recursive: true, force: true });
process.exit(failures === 0 ? 0 : 1);
