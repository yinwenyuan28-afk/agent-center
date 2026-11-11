// 验证 TOML 注释保留。
//
// 步骤：
// 1. 往 Codex 的 config.toml 里插入几行注释
// 2. 跑同步（会触发 toml_edit 改写）
// 3. 检查注释还在不在
//
// 用法：node scripts/test-toml-comments.mjs

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const CFG = 'C:/temp/provider-test/home3/.codex/config.toml';
const TEST_HOME = 'C:/temp/provider-test/home3';
const BASE = 'C:/temp/provider-test/cfg-after.toml';

// 用一份干净的基准重建
const base = readFileSync(BASE, 'utf8');
const withComments = [
  '# 用户手动加的注释，必须保留',
  '# 第二行注释',
  '',
  base.trim(),
  '',
  '# 文件末尾注释',
  '',
].join('\n');

writeFileSync(CFG, withComments, 'utf8');

const beforeLines = withComments.split('\n');
const beforeComments = beforeLines.filter((l) => l.trim().startsWith('#'));
console.log(`写入前：${beforeLines.length} 行，${beforeComments.length} 行注释`);
console.log('前 2 行注释：');
for (const c of beforeComments.slice(0, 2)) console.log(`  ${c}`);

// 跑同步
console.log('\n运行同步…');
try {
  const out = execFileSync(
    'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe',
    ['providers', 'sync'],
    { env: { ...process.env, AGENTCENTER_HOME: TEST_HOME }, encoding: 'utf8' },
  );
  console.log(out);
} catch (e) {
  console.log(e.stdout || '');
  console.log('错误：', e.stderr || e.message);
}

// 检查结果
const after = readFileSync(CFG, 'utf8');
const afterLines = after.split('\n');
const afterComments = afterLines.filter((l) => l.trim().startsWith('#'));

console.log(`\n写入后：${afterLines.length} 行，${afterComments.length} 行注释`);
console.log('保留的注释：');
for (const c of afterComments) console.log(`  ${c}`);

const kept = beforeComments.every((c) => after.includes(c));
console.log(`\n所有原始注释都保留：${kept}`);

// 检查原有表还在
for (const tbl of ['mcp_servers', 'projects', 'plugins', 'desktop', 'model_providers']) {
  console.log(`  [${tbl}] 存在：${after.includes(`[${tbl}`)}`);
}
