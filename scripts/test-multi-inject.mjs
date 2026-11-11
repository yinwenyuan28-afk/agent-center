// 在临时副本上验证 Pi 和 opencode 注入器。
//
// 用 AGENTCENTER_HOME 把整个工具重定向到一个假家目录，
// 真实配置完全不受影响。
//
// 用法：node scripts/test-multi-inject.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/home2';

/** 确保目录存在并复制一个文件（不存在就跳过）。 */
function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

// 准备假家目录的文件结构
const piDir = join(TEST, '.pi', 'agent');
const ocDir = join(TEST, '.config', 'opencode');
const ocAuthDir = join(TEST, '.local', 'share', 'opencode');

const staged = {
  piAuth: stage(join(REAL, '.pi/agent/auth.json'), join(piDir, 'auth.json')),
  piModels: stage(join(REAL, '.pi/agent/models.json'), join(piDir, 'models.json')),
  ocConfig: stage(join(REAL, '.config/opencode/opencode.json'), join(ocDir, 'opencode.json')),
  ocAuth: stage(
    join(REAL, '.local/share/opencode/auth.json'),
    join(ocAuthDir, 'auth.json'),
  ),
};

console.log('已复制的文件：');
for (const [k, v] of Object.entries(staged)) console.log(`  ${k.padEnd(10)} ${v ? '✓' : '（源不存在）'}`);

// 从真实 pi 取 command 的 key，构造测试母版
const piAuth = JSON.parse(readFileSync(join(REAL, '.pi/agent/auth.json'), 'utf8'));
const commandKey = piAuth.command.key;

const master = `[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "${commandKey}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["pi", "opencode"]

[[provider]]
id = "oc-test"
name = "opencode 测试provider"
base_url = "https://oc-test.example.com/v1"
kind = "openai-compatible"
api_key = "sk-octest-1234567890abcdef"
models = ["oc-model-a", "oc-model-b"]
agents = ["opencode"]
`;

mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents', 'providers.toml'), master, 'utf8');

// 记录同步前的状态
const snap = {};
for (const p of [
  join(piDir, 'auth.json'),
  join(piDir, 'models.json'),
  join(ocDir, 'opencode.json'),
  join(ocAuthDir, 'auth.json'),
]) {
  snap[p] = existsSync(p) ? readFileSync(p, 'utf8') : '';
}

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

// 对比结果
console.log('=== 结果 ===\n');
for (const [p, before] of Object.entries(snap)) {
  const after = existsSync(p) ? readFileSync(p, 'utf8') : '';
  const changed = before !== after;
  console.log(`${p.replace(TEST, '~').padEnd(46)} ${changed ? '已修改' : '未变'}`);

  if (changed) {
    // 检查原有内容有没有丢
    try {
      const b = JSON.parse(before);
      const a = JSON.parse(after);
      const bKeys = Object.keys(b).sort();
      const aKeys = Object.keys(a).sort();
      const lost = bKeys.filter((k) => !aKeys.includes(k));
      if (lost.length) console.log(`  !! 丢失顶层键：${lost.join(', ')}`);
      else console.log(`  顶层键完整（${bKeys.length} 个）`);
    } catch {
      console.log('  无法解析对比');
    }
  }
}

// 幂等性
console.log('\n第二次同步（验证幂等）…\n');
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
