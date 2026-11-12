// Claude Code 首次引导开关（hasCompletedOnboarding）。
//
// 重点验证两件事：
//   1. 只动这一个键，~/.claude.json 里其他内容一个字节都不变
//   2. 关闭时是删键，不是写 false（Claude Code 只认 true）
//
// 只写 C:/temp 副本。
// 用法：node scripts/test-claude-onboarding.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const TEST = 'C:/temp/provider-test/claude-onboarding';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

function run(args) {
  try {
    return execFileSync(EXE, args, {
      env: { ...process.env, AGENTCENTER_HOME: TEST },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

function readClaudeJson() {
  return JSON.parse(readFileSync(join(TEST, '.claude.json'), 'utf8'));
}

function reset(content) {
  if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
  mkdirSync(join(TEST, '.agents'), { recursive: true });
  mkdirSync(join(TEST, '.claude'), { recursive: true });
  if (content !== null) {
    writeFileSync(join(TEST, '.claude.json'), content, 'utf8');
  }
}

// 一份接近真实的 ~/.claude.json：有很多不该被碰的键
const REALISTIC = JSON.stringify(
  {
    numStartups: 42,
    installMethod: 'native',
    userID: 'abc123',
    hasCompletedOnboarding: false,
    projects: { 'C:/dev/x': { allowedTools: [] } },
    mcpServers: { time: { command: 'uvx', args: ['mcp-server-time'] } },
    oauthAccount: { accountUuid: 'default' },
    tipsHistory: { a: 1 },
  },
  null,
  2,
);

// ---------------------------------------------------------------- 开启
console.log('=== 1. 开启跳过引导 ===');
reset(REALISTIC);
const before = readClaudeJson();
const out1 = run(['claude', 'skip']);
console.log('  ' + out1.trim().split('\n').slice(-1)[0]);
let after = readClaudeJson();
check('hasCompletedOnboarding 变成 true', after.hasCompletedOnboarding === true);
check('其他键全部保留', after.numStartups === 42 && after.userID === 'abc123');
check('projects 保留', !!after.projects?.['C:/dev/x']);
check('mcpServers 保留', !!after.mcpServers?.time);
check('oauthAccount 保留', after.oauthAccount?.accountUuid === 'default');
check(
  '键的数量只增不减',
  Object.keys(after).length === Object.keys(before).length,
  `${Object.keys(before).length} → ${Object.keys(after).length}`,
);

// ---------------------------------------------------------------- 幂等
console.log('\n=== 2. 再开一次（应无改动）===');
const out2 = run(['claude', 'skip']);
console.log('  ' + out2.trim().split('\n').slice(-1)[0]);
check('第二次没有写入', /写入 0 个文件|无需改动|已经跳过/.test(out2), out2.trim().split('\n').slice(-1)[0]);

// ---------------------------------------------------------------- 关闭
console.log('\n=== 3. 关闭（应删键，不是写 false）===');
const out3 = run(['claude', 'restore']);
console.log('  ' + out3.trim().split('\n').slice(-1)[0]);
after = readClaudeJson();
check('hasCompletedOnboarding 已被删除', !('hasCompletedOnboarding' in after));
check('不是写成 false', after.hasCompletedOnboarding === undefined);
check('其他键全部保留', after.numStartups === 42 && !!after.mcpServers?.time);

// ---------------------------------------------------------------- 全新机器
console.log('\n=== 4. 文件不存在时开启（应新建）===');
reset(null);
const out4 = run(['claude', 'skip']);
console.log('  ' + out4.trim().split('\n').slice(-1)[0]);
check('.claude.json 被创建', existsSync(join(TEST, '.claude.json')));
check('只有一个键且为 true', readClaudeJson().hasCompletedOnboarding === true);

// ---------------------------------------------------------------- 状态查询
console.log('\n=== 5. 状态查询 ===');
reset(REALISTIC);
const status1 = run(['claude', 'status']);
check(
  '未跳过时如实报告',
  /首次引导：会显示/.test(status1),
  status1.trim().split('\n').slice(-1)[0],
);
run(['claude', 'skip']);
const status2 = run(['claude', 'status']);
check(
  '已跳过时如实报告',
  /首次引导：已跳过/.test(status2),
  status2.trim().split('\n').slice(-1)[0],
);

console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
for (const f of failed) console.log(`  FAIL: ${f.name}`);
process.exit(failed.length === 0 ? 0 : 1);
