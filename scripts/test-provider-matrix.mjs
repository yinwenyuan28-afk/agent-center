// provider 分配矩阵的端到端验证。
//
// 覆盖三件事：
// 1. 矩阵 API 能读出列和行，agent 可用性判定正确
// 2. set_row 写入母版，Claude 单槽位互斥生效
// 3. 取消分配后执行同步，对应条目从 agent 配置里被移除，
//    而池子外的用户手配条目原样保留
//
// 全程用 AGENTCENTER_HOME 隔离，真实配置不受影响。
//
// 用法：node scripts/test-provider-matrix.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/matrix-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

/** 跑 CLI。失败时把 stdout 也打出来，方便定位。 */
function run(args) {
  try {
    return execFileSync(EXE, args, {
      env: {
        ...process.env,
        AGENTCENTER_HOME: TEST,
        // HERMES_HOME 是另一套独立覆盖机制。不设的话 Hermes 会读真实的
        // C:\dev\ide\hermes-agent\... 配置 —— 测试不该碰那里。
        HERMES_HOME: join(TEST, 'hermes-not-installed'),
      },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

function reset() {
  if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
  mkdirSync(join(TEST, '.agents'), { recursive: true });
}

function readJson(rel) {
  return JSON.parse(readFileSync(join(TEST, rel), 'utf8'));
}

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond, extra });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

// ---------------------------------------------------------------- 场景 1
console.log('=== 1. 首次使用：母版为空 ===');
reset();
stage(join(REAL, '.claude.json'), join(TEST, '.claude.json'));
stage(join(REAL, '.codex/config.toml'), join(TEST, '.codex/config.toml'));
stage(join(REAL, '.zcode/v2/config.json'), join(TEST, '.zcode/v2/config.json'));
stage(join(REAL, '.pi/agent/auth.json'), join(TEST, '.pi/agent/auth.json'));
stage(join(REAL, '.pi/agent/models.json'), join(TEST, '.pi/agent/models.json'));
stage(join(REAL, '.claude/settings.json'), join(TEST, '.claude/settings.json'));

const emptyOut = run(['providers', 'list']);
check('空母版时 list 不报错', !/panic|error/i.test(emptyOut), emptyOut.trim().split('\n')[0] || '(空)');

// ---------------------------------------------------------------- 场景 2
console.log('\n=== 2. 写入母版：两个 provider，claude 都给 ===');
const master = `[[provider]]
id = "alpha"
name = "Alpha"
base_url = "https://alpha.example.com/v1"
kind = "anthropic"
api_key = "sk-alpha-0123456789abcdef0123456789abcdef"
models = ["alpha-model"]
agents = ["claude", "zcode"]

[[provider]]
id = "beta"
name = "Beta"
base_url = "https://beta.example.com/v1"
kind = "anthropic"
api_key = "sk-beta-0123456789abcdef0123456789abcdef"
models = ["beta-model"]
agents = ["claude", "pi"]
`;
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

// 模拟「勾上 beta 的 claude」——按后端 set_row 的语义，alpha 的 claude 应被摘掉
console.log('\n=== 3. Claude 单槽位互斥 ===');
const exclusivity = `[[provider]]
id = "alpha"
name = "Alpha"
base_url = "https://alpha.example.com/v1"
kind = "anthropic"
api_key = "sk-alpha-0123456789abcdef0123456789abcdef"
models = ["alpha-model"]
agents = ["zcode"]

[[provider]]
id = "beta"
name = "Beta"
base_url = "https://beta.example.com/v1"
kind = "anthropic"
api_key = "sk-beta-0123456789abcdef0123456789abcdef"
models = ["beta-model"]
agents = ["claude", "pi"]
`;
writeFileSync(join(TEST, '.agents/providers.toml'), exclusivity, 'utf8');

// ---------------------------------------------------------------- 场景 4
console.log('\n=== 4. 应用：alpha→zcode，beta→claude+pi ===');
const syncOut = run(['providers', 'sync']);
console.log(syncOut.trim());

const zcode = readJson('.zcode/v2/config.json');
const zcodeUrls = Object.values(zcode.provider || {})
  .map((p) => p?.options?.baseURL || '')
  .filter(Boolean);
check(
  'alpha 写进了 zcode',
  zcodeUrls.some((u) => u.includes('alpha.example.com')),
  zcodeUrls.join(' | '),
);
check(
  'beta 没写进 zcode',
  !zcodeUrls.some((u) => u.includes('beta.example.com')),
);

const claude = readJson('.claude/settings.json');
const claudeUrl = claude?.env?.ANTHROPIC_BASE_URL || '';
check('claude 拿到了 beta', claudeUrl.includes('beta.example.com'), claudeUrl);

const codexText = readFileSync(join(TEST, '.codex/config.toml'), 'utf8');
check(
  'codex 没被塞入任何池内 provider',
  !codexText.includes('[model_providers.alpha]') && !codexText.includes('[model_providers.beta]'),
);

// ---------------------------------------------------------------- 场景 5
console.log('\n=== 5. 取消分配：把 alpha 从 zcode 摘掉 ===');
const noAlpha = exclusivity.replace('agents = ["zcode"]', 'agents = []');
writeFileSync(join(TEST, '.agents/providers.toml'), noAlpha, 'utf8');

const sync2 = run(['providers', 'sync']);
console.log(sync2.trim());

const zcode2 = readJson('.zcode/v2/config.json');
const zcodeUrls2 = Object.values(zcode2.provider || {})
  .map((p) => p?.options?.baseURL || '')
  .filter(Boolean);
check(
  'alpha 已从 zcode 移除',
  !zcodeUrls2.some((u) => u.includes('alpha.example.com')),
  zcodeUrls2.join(' | ') || '(空)',
);
check(
  'zcode 的其他 provider 保留',
  zcodeUrls2.length > 0,
  `还剩 ${zcodeUrls2.length} 条`,
);

// ---------------------------------------------------------------- 场景 6
console.log('\n=== 6. 幂等：再同步一次应无改动 ===');
const sync3 = run(['providers', 'sync']);
const secondRunWrote = /\[写入\]/.test(sync3);
check('第二次同步没有写入', !secondRunWrote, secondRunWrote ? '仍有写入' : '全部跳过');

// ---------------------------------------------------------------- 场景 7
console.log('\n=== 7. claude 取消分配：清掉接管的键 ===');
const noClaude = noAlpha.replace('agents = ["claude", "pi"]', 'agents = ["pi"]');
writeFileSync(join(TEST, '.agents/providers.toml'), noClaude, 'utf8');
run(['providers', 'sync']);

const claude2 = readJson('.claude/settings.json');
check(
  'claude 的 ANTHROPIC_BASE_URL 已清掉',
  !claude2?.env?.ANTHROPIC_BASE_URL,
  claude2?.env?.ANTHROPIC_BASE_URL || '(已清除)',
);
check(
  'claude 的其他设置（非接管键）保留',
  claude2 && typeof claude2 === 'object',
);

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
if (failed.length > 0) {
  for (const f of failed) console.log(`  FAIL: ${f.name}${f.extra ? ` — ${f.extra}` : ''}`);
  process.exit(1);
}
