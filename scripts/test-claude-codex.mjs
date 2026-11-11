// 在临时副本上验证 Claude 和 Codex 注入器。
//
// 重点验证：
// 1. Claude 只注入一个 provider（单槽位限制）
// 2. Claude 不碰 JAVA_HOME / permissions 等无关键
// 3. Codex 的 TOML 注释保留
// 4. Codex 生成 codex-env.ps1
//
// 用法：node scripts/test-claude-codex.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/home3';

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

// 准备文件
const claudeDir = join(TEST, '.claude');
const codexDir = join(TEST, '.codex');

const staged = {
  claudeSettings: stage(join(REAL, '.claude/settings.json'), join(claudeDir, 'settings.json')),
  codexConfig: stage(join(REAL, '.codex/config.toml'), join(codexDir, 'config.toml')),
};
console.log('已复制：', staged);

const piAuth = JSON.parse(readFileSync(join(REAL, '.pi/agent/auth.json'), 'utf8'));
const commandKey = piAuth.command.key;

const master = `[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "${commandKey}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["claude", "codex"]

[[provider]]
id = "codex-alt"
name = "Codex 备用"
base_url = "https://codex-alt.example.com/v1"
kind = "openai-compatible"
api_key = "sk-codexalt-1234567890abcd"
models = ["codex-model"]
agents = ["codex"]
`;

mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

// 记录同步前
const claudeBefore = readFileSync(join(claudeDir, 'settings.json'), 'utf8');
const codexBefore = readFileSync(join(codexDir, 'config.toml'), 'utf8');

console.log('\n运行同步…\n');
try {
  const out = execFileSync(
    'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe',
    ['providers', 'sync'],
    { env: { ...process.env, AGENTCENTER_HOME: TEST }, encoding: 'utf8' },
  );
  console.log(out);
} catch (e) {
  console.log(e.stdout || '');
  console.log('错误：', e.stderr || e.message);
}

// ==== Claude 验证 ====
console.log('\n=== Claude 验证 ===\n');
const claudeAfter = JSON.parse(readFileSync(join(claudeDir, 'settings.json'), 'utf8'));
const claudeBeforeJson = JSON.parse(claudeBefore);

// 1. 无关键是否保留
const unrelatedBefore = Object.keys(claudeBeforeJson).filter((k) => k !== 'env');
const unrelatedAfter = Object.keys(claudeAfter).filter((k) => k !== 'env');
console.log(`顶层非 env 键：${JSON.stringify(unrelatedBefore)} → ${JSON.stringify(unrelatedAfter)}`);
console.log(`保留完整：${JSON.stringify(unrelatedBefore.sort()) === JSON.stringify(unrelatedAfter.sort())}`);

// 2. env 里的无关变量是否保留（JAVA_HOME）
const javaHomeBefore = claudeBeforeJson.env?.JAVA_HOME;
const javaHomeAfter = claudeAfter.env?.JAVA_HOME;
console.log(`JAVA_HOME：${javaHomeBefore ? '有' : '无'} → ${javaHomeAfter ? '有' : '无'}（应保持不变）`);

// 3. 只注入第一个 provider（command），第二个（codex-alt）不该出现
const baseUrl = claudeAfter.env?.ANTHROPIC_BASE_URL;
console.log(`ANTHROPIC_BASE_URL：${baseUrl}`);
console.log(`是否为 command：${baseUrl === 'https://api.commandcode.ai/provider/v1'}`);
console.log(`是否误注入 codex-alt：${String(baseUrl).includes('codex-alt')}`);

// ==== Codex 验证 ====
console.log('\n=== Codex 验证 ===\n');
const codexAfter = readFileSync(join(codexDir, 'config.toml'), 'utf8');

// 1. 注释是否保留
const commentsBefore = (codexBefore.match(/^\s*#/gm) || []).length;
const commentsAfter = (codexAfter.match(/^\s*#/gm) || []).length;
console.log(`注释行数：${commentsBefore} → ${commentsAfter}`);

// 2. 原有表是否保留
for (const tbl of ['mcp_servers', 'projects', 'plugins', 'desktop']) {
  const before = codexBefore.includes(`[${tbl}`);
  const after = codexAfter.includes(`[${tbl}`);
  console.log(`  ${tbl.padEnd(14)} ${before ? '原有' : '无  '} → ${after ? '保留' : '丢失'}`);
}

// 3. 新增的 provider 表
console.log(`\n含 model_providers：${codexAfter.includes('[model_providers')}`);
console.log(`含 codex-alt：${codexAfter.includes('codex-alt')}`);
console.log(`含 env_key：${codexAfter.includes('env_key')}`);

// 4. 环境变量脚本
const envScript = join(TEST, '.agents/codex-env.ps1');
if (existsSync(envScript)) {
  const s = readFileSync(envScript, 'utf8');
  console.log(`\n生成 codex-env.ps1（${s.length} 字节）`);
  console.log(`变量行数：${(s.match(/^\$env:/gm) || []).length}`);
  // 不打印 key
  console.log(
    '变量名：' +
      (s.match(/^\$env:(\w+)/gm) || []).map((m) => m.replace('$env:', '')).join(', '),
  );
} else {
  console.log('\n!! codex-env.ps1 未生成');
}

// 5. 幂等性
console.log('\n第二次同步（幂等验证）…\n');
try {
  const out2 = execFileSync(
    'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe',
    ['providers', 'sync'],
    { env: { ...process.env, AGENTCENTER_HOME: TEST }, encoding: 'utf8' },
  );
  console.log(out2);
} catch (e) {
  console.log(e.stdout || '');
}
