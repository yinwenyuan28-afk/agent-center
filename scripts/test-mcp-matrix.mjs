// 测试 MCP 分配矩阵：导入 → 勾选 → 应用 → 取消勾选 → 应用。
//
// 用 AGENTCENTER_HOME 隔离，真实配置不受影响。
//
// 用法：node scripts/test-mcp-matrix.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/matrix-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

function run(args) {
  return execFileSync(EXE, args, {
    env: {
      ...process.env,
      AGENTCENTER_HOME: TEST,
      // Hermes 读 HERMES_HOME，AGENTCENTER_HOME 管不到它。
      // 指向一个不存在的目录，避免测试碰真实配置。
      HERMES_HOME: join(TEST, "hermes-not-installed"),
    },
    encoding: 'utf8',
  });
}

function stage(src, dst) {
  if (!existsSync(src)) return;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
}

// 只复制 Claude 和 Codex（最小验证集）
stage(join(REAL, '.claude.json'), join(TEST, '.claude.json'));
stage(join(REAL, '.codex/config.toml'), join(TEST, '.codex/config.toml'));

// 清空母版（模拟首次使用）
mkdirSync(join(TEST, '.agents'), { recursive: true });

console.log('=== 1. 首次：母版为空 ===');
const m1 = JSON.parse(execFileSync(EXE, ['--version'], { encoding: 'utf8' }) ? '{}' : '{}');
try {
  console.log(run(['mcp', 'list']));
} catch (e) {
  console.log(e.stdout || '');
}

console.log('\n=== 2. 从现有配置导入 ===');
// CLI 没有 import 子命令，直接调用逻辑：用 node 模拟不行，
// 所以这里验证矩阵 API 的行为——先手工写一个母版
import('node:fs').then((fs) => {
  const master = `[[server]]
id = "context7"
name = "Context7"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ["claude"]

[[server]]
id = "fetch"
name = "Fetch"
transport = "stdio"
command = "uvx"
args = ["mcp-server-fetch"]
enabled = true
agents = ["claude", "codex"]
`;
  fs.writeFileSync(join(TEST, '.agents/mcp.toml'), master, 'utf8');
  console.log('已写母版：context7 只给 claude，fetch 给 claude+codex');

  // 记录应用前
  const claudeBefore = JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
  const codexBefore = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');

  console.log('\n=== 3. 应用（第一次）===');
  console.log(run(['mcp', 'sync']));

  // 检查结果
  const claudeAfter = JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
  const codexAfter = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');

  console.log('\n--- 结果 ---');
  const claudeNames = Object.keys(claudeAfter.mcpServers || {});
  console.log(`claude 有: ${claudeNames.join(', ')}`);
  console.log(`  context7 在: ${claudeNames.includes('context7')}`);
  console.log(`  fetch 在: ${claudeNames.includes('fetch')}`);
  console.log(`  原有 time/sequential-thinking 保留: ${claudeNames.includes('time') && claudeNames.includes('sequential-thinking')}`);

  const codexHasCtx = codexAfter.includes('[mcp_servers.context7]');
  const codexHasFetch = codexAfter.includes('[mcp_servers.fetch]');
  console.log(`codex: context7=${codexHasCtx}（应 false，只分配给 claude）fetch=${codexHasFetch}（应 true）`);

  console.log('\n=== 4. 改变分配：context7 改成给 codex，claude 取消 ===');
  const newMaster = master.replace(
    'agents = ["claude"]',
    'agents = ["claude", "codex"]',
  );
  fs.writeFileSync(join(TEST, '.agents/mcp.toml'), newMaster, 'utf8');

  console.log(run(['mcp', 'sync']));

  const codexAfter2 = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');
  console.log(`\ncodex 现在有 context7: ${codexAfter2.includes('[mcp_servers.context7]')}`);

  console.log('\n=== 5. 移除测试：把 fetch 从 claude 取消 ===');
  const master3 = newMaster.replace(
    'id = "fetch"\nname = "Fetch"\ntransport = "stdio"\ncommand = "uvx"\nargs = ["mcp-server-fetch"]\nenabled = true\nagents = ["claude", "codex"]',
    'id = "fetch"\nname = "Fetch"\ntransport = "stdio"\ncommand = "uvx"\nargs = ["mcp-server-fetch"]\nenabled = true\nagents = ["codex"]',
  );
  fs.writeFileSync(join(TEST, '.agents/mcp.toml'), master3, 'utf8');
  console.log(run(['mcp', 'sync']));

  const claudeAfter3 = JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
  const names3 = Object.keys(claudeAfter3.mcpServers || {});
  console.log(`\nclaude 现在: ${names3.join(', ')}`);
  console.log(`  fetch 已移除: ${!names3.includes('fetch')}`);
  console.log(`  context7 仍在: ${names3.includes('context7')}`);
  console.log(`  用户手动的 time 保留: ${names3.includes('time')}`);
});
