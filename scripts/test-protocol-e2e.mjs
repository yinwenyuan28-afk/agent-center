// 端到端验证：应用里改协议 → 注入到 agent 的配置文件里就是那个协议。
//
// 覆盖四条链路，每条都验「改应用里的设置，agent 配置真的变了」。
//
// 用法：node scripts/test-protocol-e2e.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const TEST = 'C:/temp/provider-test/proto-e2e';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

function run(args) {
  try {
    return execFileSync(EXE, args, {
      env: {
        ...process.env,
        AGENTCENTER_HOME: TEST,
        HERMES_HOME: join(TEST, 'hermes'),
      },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

function json(rel) {
  return JSON.parse(readFileSync(join(TEST, rel), 'utf8'));
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
  mkdirSync(join(TEST, 'hermes'), { recursive: true });
  writeFileSync(join(TEST, '.zcode/v2/config.json'), '{"provider":{}}', 'utf8');
  writeFileSync(join(TEST, '.pi/agent/models.json'), '{"providers":{}}', 'utf8');
  writeFileSync(join(TEST, '.pi/agent/auth.json'), '{}', 'utf8');
  // Hermes 的 config.yaml 必须存在，否则注入器直接返回空计划
  writeFileSync(join(TEST, 'hermes/config.yaml'), 'model:\n  provider: none\n', 'utf8');
}

function writeMaster(agentKindsLines, kind = 'anthropic') {
  writeFileSync(
    join(TEST, '.agents/providers.toml'),
    `[[provider]]
id = "p1"
name = "P1"
base_url = "https://p1.example.com/v1"
kind = "${kind}"
api_key = "sk-p1-0123456789abcdef0123456789abcdef"
models = ["m1"]
agents = ["zcode", "pi", "hermes"]
${agentKindsLines}
`,
    'utf8',
  );
}

// ---------------------------------------------------------------- 1. 默认协议
console.log('=== 1. 不设覆盖时，所有 agent 用默认协议 ===');
reset();
writeMaster('');
run(['providers', 'sync']);

const zc1 = json('.zcode/v2/config.json');
const zcEntry1 = Object.values(zc1.provider || {})[0];
check('ZCode 用默认的 anthropic', zcEntry1?.kind === 'anthropic', zcEntry1?.kind);

const pi1 = json('.pi/agent/models.json');
check('Pi 用默认的 anthropic-messages', pi1.providers?.p1?.api === 'anthropic-messages', pi1.providers?.p1?.api);

const hermesYaml1 = readFileSync(join(TEST, 'hermes/config.yaml'), 'utf8');
check(
  'Hermes 用默认的 anthropic_messages',
  /api_mode: anthropic_messages/.test(hermesYaml1),
  hermesYaml1.split('\n').filter((l) => l.includes('api_mode')).join(' | '),
);

// ---------------------------------------------------------------- 2. 改 Pi 的协议
console.log('\n=== 2. 只给 Pi 设 openai-responses ===');
reset();
writeMaster('[provider.agent_kinds]\npi = "openai-responses"');
run(['providers', 'sync']);

const pi2 = json('.pi/agent/models.json');
check(
  'Pi 变成 openai-responses',
  pi2.providers?.p1?.api === 'openai-responses',
  pi2.providers?.p1?.api,
);

const zc2 = json('.zcode/v2/config.json');
const zcEntry2 = Object.values(zc2.provider || {})[0];
check('ZCode 仍是默认 anthropic', zcEntry2?.kind === 'anthropic', zcEntry2?.kind);

const hermesYaml2 = readFileSync(join(TEST, 'hermes/config.yaml'), 'utf8');
check(
  'Hermes 仍是默认 anthropic_messages',
  /api_mode: anthropic_messages/.test(hermesYaml2),
);

// ---------------------------------------------------------------- 3. 改 ZCode 的协议
console.log('\n=== 3. 给 ZCode 设 openai-chat（默认改成 anthropic 之外的值）===');
reset();
writeMaster('[provider.agent_kinds]\nzcode = "openai-chat"');
run(['providers', 'sync']);

const zc3 = json('.zcode/v2/config.json');
const zcEntry3 = Object.values(zc3.provider || {})[0];
check(
  'ZCode 变成 openai-compatible',
  zcEntry3?.kind === 'openai-compatible',
  zcEntry3?.kind,
);

// ---------------------------------------------------------------- 4. CLI 设置
console.log('\n=== 4. CLI --agent-kind 也能设 ===');
reset();
writeMaster('');
const setOut = run(['providers', 'set', 'p1', '--agent-kind', 'hermes=openai-responses']);
console.log('  [set]', setOut.trim().split('\n')[0]);

const masterAfter = readFileSync(join(TEST, '.agents/providers.toml'), 'utf8');
check(
  'CLI 把覆盖写进了母版',
  /hermes = "openai-responses"/.test(masterAfter),
  masterAfter.split('\n').filter((l) => l.includes('hermes =')).join(' | '),
);

run(['providers', 'sync']);
const hermesYaml4 = readFileSync(join(TEST, 'hermes/config.yaml'), 'utf8');
check(
  'Hermes 变成 codex_responses',
  /api_mode: codex_responses/.test(hermesYaml4),
  hermesYaml4.split('\n').filter((l) => l.includes('api_mode')).join(' | '),
);

// ---------------------------------------------------------------- 5. 覆盖比分配多
console.log('\n=== 5. 给没分配的 agent 设协议，不影响注入 ===');
reset();
writeFileSync(
  join(TEST, '.agents/providers.toml'),
  `[[provider]]
id = "p1"
name = "P1"
base_url = "https://p1.example.com/v1"
kind = "anthropic"
api_key = "sk-p1-0123456789abcdef0123456789abcdef"
models = ["m1"]
agents = ["zcode"]

[provider.agent_kinds]
pi = "openai-responses"
`,
  'utf8',
);
run(['providers', 'sync']);

const pi5 = json('.pi/agent/models.json');
check('Pi 没分配，不写入', !pi5.providers?.p1, JSON.stringify(Object.keys(pi5.providers || {})));

const masterFinal = readFileSync(join(TEST, '.agents/providers.toml'), 'utf8');
check('母版里保留了 pi 的覆盖设置', /pi = "openai-responses"/.test(masterFinal));

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
if (failed.length > 0) {
  for (const f of failed) console.log(`  FAIL: ${f.name}${f.extra ? ` — ${f.extra}` : ''}`);
  process.exit(1);
}
