// 验证「协议按 agent 分开」这个模型。
//
// 核心场景：同一个 provider，ZCode 用 anthropic、Pi 用 openai-responses。
// 重构前协议只能写在 provider 上，这种配置根本表达不出来。
//
// 用 AGENTCENTER_HOME 隔离，真实配置不受影响。
//
// 用法：node scripts/test-agent-kinds.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/kinds-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

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

function read(rel) {
  return readFileSync(join(TEST, rel), 'utf8');
}

function readJson(rel) {
  return JSON.parse(read(rel));
}

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond, extra });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

function reset() {
  if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
  mkdirSync(join(TEST, '.agents'), { recursive: true });
  mkdirSync(join(TEST, '.zcode/v2'), { recursive: true });
  mkdirSync(join(TEST, '.pi/agent'), { recursive: true });
}

// 造一个干净的 ZCode 配置（只有一个空 provider map）
const zcodeCfg = { provider: {} };
// 造一个干净的 Pi models.json
const piModels = { providers: {} };
// Pi 的 auth.json
const piAuth = {};

// 母版：同一个端点，ZCode 用 anthropic、Pi 用 openai-responses
const master = `[[provider]]
id = "multi"
name = "Multi Protocol"
base_url = "https://multi.example.com/v1"
kind = "anthropic"
api_key = "sk-multi-0123456789abcdef0123456789abcdef"
models = ["m1"]
agents = ["zcode", "pi"]

[provider.agent_kinds]
pi = "openai-responses"
`;

// ---------------------------------------------------------------- 场景 1
console.log('=== 1. 同一 provider，不同 agent 用不同协议 ===');
reset();
writeFileSync(join(TEST, '.zcode/v2/config.json'), JSON.stringify(zcodeCfg, null, 2), 'utf8');
writeFileSync(join(TEST, '.pi/agent/models.json'), JSON.stringify(piModels, null, 2), 'utf8');
writeFileSync(join(TEST, '.pi/agent/auth.json'), JSON.stringify(piAuth, null, 2), 'utf8');
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

const syncOut = run(['providers', 'sync']);
console.log('  [sync]', syncOut.trim().split('\n').slice(0, 6).join(' / '));

// ZCode 应该拿到 anthropic
const zcode = readJson('.zcode/v2/config.json');
const zcEntry = Object.values(zcode.provider || {}).find(
  (p) => p?.options?.baseURL === 'https://multi.example.com/v1',
);
check('ZCode 写入了这个 provider', !!zcEntry);
check(
  'ZCode 用 anthropic（默认值）',
  zcEntry?.kind === 'anthropic',
  zcEntry?.kind,
);

// Pi 应该拿到 openai-responses
const pi = readJson('.pi/agent/models.json');
const piEntry = pi.providers?.multi;
check('Pi 写入了这个 provider', !!piEntry);
check(
  'Pi 用 openai-responses（agent_kinds 覆盖）',
  piEntry?.api === 'openai-responses',
  piEntry?.api,
);

// ---------------------------------------------------------------- 场景 2
console.log('\n=== 2. 母版往返保留 agent_kinds ===');
const masterAfter = read('..agents/providers.toml'.replace('..', '.'));
const roundTrip = read('.agents/providers.toml');
check('母版里保留了 agent_kinds', /agent_kinds/.test(roundTrip), roundTrip.split('\n').filter((l) => l.includes('agent_kinds') || l.includes('pi =')).join(' | '));

// ---------------------------------------------------------------- 场景 3
console.log('\n=== 3. 去掉覆盖后回到默认协议 ===');
const noOverride = master.replace(/\n\[provider\.agent_kinds\]\npi = "openai-responses"\n/, '\n');
writeFileSync(join(TEST, '.agents/providers.toml'), noOverride, 'utf8');
run(['providers', 'sync']);

const pi2 = readJson('.pi/agent/models.json');
check(
  'Pi 改用默认 anthropic',
  pi2.providers?.multi?.api === 'anthropic-messages',
  pi2.providers?.multi?.api,
);

// ---------------------------------------------------------------- 场景 4
console.log('\n=== 4. 扫描能读回各 agent 的实际协议 ===');
// 把 ZCode 里改成 openai-compatible，然后用 backfill 看能否发现不一致
reset();
writeFileSync(join(TEST, '.zcode/v2/config.json'), JSON.stringify(zcodeCfg, null, 2), 'utf8');
writeFileSync(join(TEST, '.pi/agent/models.json'), JSON.stringify(piModels, null, 2), 'utf8');
writeFileSync(join(TEST, '.pi/agent/auth.json'), JSON.stringify(piAuth, null, 2), 'utf8');
// 母版默认 anthropic，没有覆盖
const plainMaster = master.replace(/\n\[provider\.agent_kinds\]\npi = "openai-responses"\n/, '\n');
writeFileSync(join(TEST, '.agents/providers.toml'), plainMaster, 'utf8');
run(['providers', 'sync']);

// 手动把 ZCode 里的 kind 改成 openai-compatible，模拟「实际在用另一种协议」
const zcManual = readJson('.zcode/v2/config.json');
for (const p of Object.values(zcManual.provider || {})) {
  if (p?.options?.baseURL === 'https://multi.example.com/v1') {
    p.kind = 'openai-compatible';
  }
}
writeFileSync(join(TEST, '.zcode/v2/config.json'), JSON.stringify(zcManual, null, 2), 'utf8');

const backfillOut = run(['models', 'backfill', '--dry-run']);
check(
  'backfill 报告协议不一致',
  /协议/.test(backfillOut) && /openai-chat/.test(backfillOut),
  backfillOut.split('\n').filter((l) => l.includes('multi') || l.includes('协议')).join(' / '),
);

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
if (failed.length > 0) {
  for (const f of failed) console.log(`  FAIL: ${f.name}${f.extra ? ` — ${f.extra}` : ''}`);
  process.exit(1);
}
