// 在副本上验证 WorkBuddy 和 DSH 注入器。
//
// 用 AGENTCENTER_HOME 重定向，真实配置不受影响。
//
// 用法：node scripts/test-workbuddy-dsh.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/home4';

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

const wbPath = join(TEST, '.workbuddy/models.json');
const dshPatch = join(TEST, '.dsh/profiles/desktop/cordis.patch.yml');
const dshCred = join(TEST, '.dsh/.credentials.yaml');

const staged = {
  workbuddy: stage(join(REAL, '.workbuddy/models.json'), wbPath),
  dshPatch: stage(join(REAL, '.dsh/profiles/desktop/cordis.patch.yml'), dshPatch),
  dshCred: stage(join(REAL, '.dsh/.credentials.yaml'), dshCred),
};
console.log('已复制：', staged);

// 从真实 pi 取 command 的 key
const piAuth = JSON.parse(readFileSync(join(REAL, '.pi/agent/auth.json'), 'utf8'));
const commandKey = piAuth.command.key;

// 母版：加入 workbuddy 和 dsh 的注入目标
const master = `[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "${commandKey}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["workbuddy", "dsh"]

[[provider]]
id = "responses-test"
name = "Responses 协议测试"
base_url = "https://responses.example.com/v1"
kind = "openai-responses"
api_key = "sk-responses-1234567890abcdef"
models = ["resp-model"]
agents = ["dsh"]
`;

mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

// 记录同步前
const snap = {
  workbuddy: existsSync(wbPath) ? readFileSync(wbPath, 'utf8') : '',
  dshPatch: existsSync(dshPatch) ? readFileSync(dshPatch, 'utf8') : '',
  dshCred: existsSync(dshCred) ? readFileSync(dshCred, 'utf8') : '',
};

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

// ==== WorkBuddy 验证 ====
console.log('\n=== WorkBuddy 验证 ===\n');
const wbAfter = readFileSync(wbPath, 'utf8');
const wbBeforeJson = JSON.parse(snap.workbuddy);
const wbAfterJson = JSON.parse(wbAfter);

console.log(`条目数：${wbBeforeJson.length} → ${wbAfterJson.length}`);
console.log(`仍是数组：${Array.isArray(wbAfterJson)}`);

// 检查原有字段是否保留
const first = wbAfterJson[0];
const requiredFields = ['id', 'name', 'vendor', 'url', 'apiKey', 'supportsToolCall', 'maxInputTokens'];
const missing = requiredFields.filter((f) => !(f in first));
console.log(`原有字段保留：${missing.length === 0 ? '是' : '缺 ' + missing.join(',')}`);

// 检查 key 是否统一
const urls = new Set(wbAfterJson.map((m) => m.url));
console.log(`不同 url 数：${urls.size}`);

// ==== DSH 验证 ====
console.log('\n=== DSH 验证 ===\n');
const patchAfter = readFileSync(dshPatch, 'utf8');
const credAfter = readFileSync(dshCred, 'utf8');

// 注释保留
const commentsBefore = (snap.dshPatch.match(/^\s*#/gm) || []).length;
const commentsAfter = (patchAfter.match(/^\s*#/gm) || []).length;
console.log(`patch 注释行：${commentsBefore} → ${commentsAfter}`);

// 没有重复的 providers 段
const provHeaders = (patchAfter.match(/^providers:/gm) || []).length;
console.log(`providers 段数量：${provHeaders}（应为 1）`);

// 新增的 provider 在不在
console.log(`含 responses-test：${patchAfter.includes('responses-test')}`);
console.log(`含 openai-responses 协议：${patchAfter.includes('openai-responses')}`);

// credentials 里有没有对应 key
console.log(`credentials 含 RESPONSES_TEST_API_KEY：${credAfter.includes('RESPONSES_TEST_API_KEY')}`);
console.log(`credentials 的 version 行保留：${credAfter.includes('version:')}`);

// 原有 records 段保留
console.log(`records 段保留：${credAfter.includes('records:')}`);

// ==== 幂等 ====
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
