// 测试 MCP 的扫描与差异对比（只读）。
//
// 用真实母版和真实配置，只跑 diff 不写盘。
//
// 用法：node scripts/test-mcp-diff.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';
const MASTER = 'C:/Users/leaf/.agents/mcp.toml';

// 备份已有母版
if (existsSync(MASTER)) {
  copyFileSync(MASTER, MASTER + '.bak');
  console.log('已备份现有 mcp.toml');
}

// 用真实存在的 4 个 server 造母版，验证 diff 能否正确识别「已一致」
const master = `[[server]]
id = "context7"
name = "Context7"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ["claude", "codex", "pi", "opencode"]

[[server]]
id = "fetch"
name = "Fetch"
transport = "stdio"
command = "uvx"
args = ["mcp-server-fetch"]
enabled = true
agents = ["claude", "codex", "opencode"]

[[server]]
id = "time"
name = "Time"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@modelcontextprotocol/server-time"]
enabled = true
agents = ["claude", "codex"]

[[server]]
id = "new-test-server"
name = "测试新增"
transport = "stdio"
command = "node"
args = ["test.js"]
enabled = true
agents = ["claude", "cursor"]
`;

mkdirSync('C:/Users/leaf/.agents', { recursive: true });
writeFileSync(MASTER, master, 'utf8');
console.log('已写入测试母版（4 个 server）\n');

// 跑 list
console.log('=== mcp list ===\n');
try {
  console.log(execFileSync(EXE, ['mcp', 'list'], { encoding: 'utf8' }));
} catch (e) {
  console.log(e.stdout || e.message);
}

// 跑 diff
console.log('\n=== mcp diff ===\n');
try {
  console.log(execFileSync(EXE, ['mcp', 'diff'], { encoding: 'utf8' }));
} catch (e) {
  console.log(e.stdout || '');
  console.log('stderr:', e.stderr || '');
}
