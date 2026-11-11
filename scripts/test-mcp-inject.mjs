// 在副本上验证 MCP 注入器。
//
// 用 AGENTCENTER_HOME 重定向，真实配置不受影响。
//
// 用法：node scripts/test-mcp-inject.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/mcp-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

// 复制真实配置到假 HOME
const files = {
  claudeJson: [join(REAL, '.claude.json'), join(TEST, '.claude.json')],
  codexToml: [join(REAL, '.codex/config.toml'), join(TEST, '.codex/config.toml')],
  cursorMcp: [join(REAL, '.cursor/mcp.json'), join(TEST, '.cursor/mcp.json')],
  piMcp: [join(REAL, '.pi/agent/mcp.json'), join(TEST, '.pi/agent/mcp.json')],
  ocJson: [join(REAL, '.config/opencode/opencode.json'), join(TEST, '.config/opencode/opencode.json')],
  qoderSettings: [join(REAL, '.qoder/settings.json'), join(TEST, '.qoder/settings.json')],
  wbMcp: [
    join(REAL, '.workbuddy/connectors/default/mcp.json'),
    join(TEST, '.workbuddy/connectors/default/mcp.json'),
  ],
};

const staged = {};
for (const [k, [src, dst]] of Object.entries(files)) staged[k] = stage(src, dst);
console.log('已复制：', staged);

// 母版：一个已存在（context7）+ 一个新增 + 一个远程
const master = `[[server]]
id = "context7"
name = "Context7"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ["claude", "codex", "pi", "opencode"]

[[server]]
id = "mcp-inject-test"
name = "注入测试"
transport = "stdio"
command = "node"
args = ["server.js"]
enabled = true
agents = ["claude", "codex", "cursor", "pi", "opencode", "qoder", "workbuddy"]

[[server]]
id = "remote-test"
name = "远程测试"
transport = "http"
url = "https://remote.example.com/mcp"
enabled = false
agents = ["claude", "opencode"]
`;

mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents/mcp.toml'), master, 'utf8');

// 记录同步前
const snap = {};
for (const [k, [, dst]] of Object.entries(files)) {
  snap[k] = existsSync(dst) ? readFileSync(dst, 'utf8') : '';
}

console.log('\n运行同步…\n');
try {
  const out = execFileSync(EXE, ['mcp', 'sync'], {
    env: { ...process.env, AGENTCENTER_HOME: TEST },
    encoding: 'utf8',
  });
  console.log(out);
} catch (e) {
  console.log(e.stdout || '');
  console.log('错误：', e.stderr || e.message);
}

// 验证结果
console.log('\n=== 验证 ===\n');

// Claude: 检查新增的 server 和原有的是否都在
if (existsSync(join(TEST, '.claude.json'))) {
  const j = JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
  const names = Object.keys(j.mcpServers || {});
  console.log(`[claude] mcpServers: ${names.join(', ')}`);
  console.log(`  含 mcp-inject-test: ${names.includes('mcp-inject-test')}`);
  console.log(`  含 remote-test: ${names.includes('remote-test')}`);
  console.log(`  原有 4 个保留: ${['context7','fetch','time','sequential-thinking'].every(n => names.includes(n))}`);
  // 检查远程型的结构
  const remote = j.mcpServers?.['remote-test'];
  if (remote) console.log(`  remote 结构: type=${remote.type} url=${remote.url}`);
  // 检查顶层其他键
  console.log(`  顶层键数: ${Object.keys(j).length}（应 > 10，projects 等保留）`);
}

// Codex: 检查 TOML 注释保留和新表
if (existsSync(join(TEST, '.codex/config.toml'))) {
  const t = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');
  console.log(`\n[codex] 含 mcp-inject-test: ${t.includes('mcp-inject-test')}`);
  console.log(`  含 remote-test: ${t.includes('remote-test')}`);
  console.log(`  mcp_servers 表数: ${(t.match(/^\[mcp_servers\./gm) || []).length}`);
  console.log(`  原有 model/mcp 配置保留: ${t.includes('model =') && t.includes('[mcp_servers.node_repl]')}`);
}

// opencode: 检查 command 数组形式
if (existsSync(join(TEST, '.config/opencode/opencode.json'))) {
  const j = JSON.parse(readFileSync(join(TEST, '.config/opencode/opencode.json'), 'utf8'));
  const entry = j.mcp?.['mcp-inject-test'];
  console.log(`\n[opencode] 含 mcp-inject-test: ${!!entry}`);
  if (entry) {
    console.log(`  type: ${entry.type}`);
    console.log(`  command 是数组: ${Array.isArray(entry.command)}`);
    console.log(`  command: ${JSON.stringify(entry.command)}`);
    console.log(`  enabled: ${entry.enabled}`);
  }
  const remote = j.mcp?.['remote-test'];
  if (remote) console.log(`  remote: type=${remote.type} url=${remote.url} enabled=${remote.enabled}`);
}

// WorkBuddy: 检查 disabled 字段和联系器保留
if (existsSync(join(TEST, '.workbuddy/connectors/default/mcp.json'))) {
  const j = JSON.parse(readFileSync(join(TEST, '.workbuddy/connectors/default/mcp.json'), 'utf8'));
  const count = Object.keys(j.mcpServers || {}).length;
  console.log(`\n[workbuddy] server 总数: ${count}（原 83 + 1）`);
  const entry = j.mcpServers?.['mcp-inject-test'];
  if (entry) {
    console.log(`  新增条目 disabled: ${entry.disabled}（应为 false）`);
  }
  console.log(`  connector: 开头的保留: ${Object.keys(j.mcpServers || {}).filter(k => k.startsWith('connector:')).length}`);
}

// 幂等
console.log('\n第二次同步（幂等验证）…\n');
try {
  const out2 = execFileSync(EXE, ['mcp', 'sync'], {
    env: { ...process.env, AGENTCENTER_HOME: TEST },
    encoding: 'utf8',
  });
  console.log(out2);
} catch (e) {
  console.log(e.stdout || '');
}
