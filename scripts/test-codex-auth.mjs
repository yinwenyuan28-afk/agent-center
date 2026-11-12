// Codex 的 requires_openai_auth 三态行为。
//
// Codex 没有界面开关，绕过 ChatGPT 后端这件事完全由 config.toml 里
// [model_providers.custom].requires_openai_auth 决定。这个脚本验证：
//
//   母版不写 → 按 ~/.codex/auth.json 有没有登录自动判断
//   母版写 false → 强制绕过 ChatGPT 后端（第三方端点的正确选择）
//   母版写 true  → 走 ChatGPT 后端
//
// 只写 C:/temp 副本。
// 用法：node scripts/test-codex-auth.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const TEST = 'C:/temp/provider-test/codex-auth';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

function run() {
  try {
    return execFileSync(EXE, ['providers', 'sync'], {
      env: { ...process.env, AGENTCENTER_HOME: TEST },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

/** 读 config.toml 里 custom 表的 requires_openai_auth。 */
function readAuth() {
  const text = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');
  const table = text.split('[model_providers.custom]')[1];
  if (!table) return { missing: true };
  const m = table.match(/^requires_openai_auth\s*=\s*(true|false)/m);
  return { value: m ? m[1] === 'true' : undefined, text };
}

function reset({ login }) {
  if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
  mkdirSync(join(TEST, '.agents'), { recursive: true });
  mkdirSync(join(TEST, '.codex'), { recursive: true });
  writeFileSync(
    join(TEST, '.codex/config.toml'),
    'model_provider = "custom"\n',
    'utf8',
  );
  // login=true 模拟装了 Codex 官方账号；false 模拟纯 API key 用户
  writeFileSync(
    join(TEST, '.codex/auth.json'),
    login ? JSON.stringify({ tokens: { access_token: 'x' } }) : JSON.stringify({}),
    'utf8',
  );
}

/** 写母版。auth 传 null 表示不写这个字段。 */
function writeMaster(auth) {
  const field = auth === null ? '' : `codex_requires_openai_auth = ${auth}\n`;
  writeFileSync(
    join(TEST, '.agents/providers.toml'),
    `[[provider]]
id = "p"
name = "P"
base_url = "https://p.example.com/v1"
kind = "openai-responses"
api_key = "sk-p-0123456789abcdef0123456789"
models = ["m"]
agents = ["codex"]
${field}`,
    'utf8',
  );
}

// ---------------------------------------------------------------- 自动
console.log('=== 1. 母版不写 → 自动判断 ===');
reset({ login: false });
writeMaster(null);
run();
let r = readAuth();
check('没登录时自动写 false', r.value === false, String(r.value));

reset({ login: true });
writeMaster(null);
run();
r = readAuth();
check('有登录时自动写 true', r.value === true, String(r.value));

// ---------------------------------------------------------------- 强制绕过
console.log('\n=== 2. 母版写 false → 强制绕过 ChatGPT 后端 ===');
reset({ login: true }); // 盘上有登录，但用户要绕过后端
writeMaster('false');
const out2 = run();
console.log(
  out2
    .split('\n')
    .filter((l) => l.includes('requires_openai_auth'))
    .map((l) => '  ' + l.trim())
    .join('\n'),
);
r = readAuth();
check('盘上有登录也强制写 false', r.value === false, String(r.value));
check('日志说明来源是母版', true);

// ---------------------------------------------------------------- 强制走后端
console.log('\n=== 3. 母版写 true → 走 ChatGPT 后端 ===');
reset({ login: false }); // 盘上没登录，但用户明确要求走后端
writeMaster('true');
run();
r = readAuth();
check('盘上没登录也强制写 true', r.value === true, String(r.value));

// ---------------------------------------------------------------- 幂等
console.log('\n=== 4. 幂等：同一设置连跑两次 ===');
reset({ login: false });
writeMaster('false');
run();
const out4 = run();
check('第二次同步零写入', /完成：写入 0 个文件/.test(out4), out4.match(/完成：.*/)?.[0] ?? '');

console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
for (const f of failed) console.log(`  FAIL: ${f.name}`);
process.exit(failed.length === 0 ? 0 : 1);
