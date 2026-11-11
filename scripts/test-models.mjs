// 模型元数据链路的端到端验证。
//
// 覆盖五件事：
// 1. 存量纯字符串 models 能读能写，往返形状不变
// 2. 带元数据的 models 能读写
// 3. 改 context_window 后注入生效，其他字段不动
// 4. 母版留空时，agent 里已有的值不被覆盖
// 5. backfill 只填空字段，不覆盖用户已填的值
//
// 全程用 AGENTCENTER_HOME 隔离，真实配置不受影响。
//
// 用法：node scripts/test-models.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/models-home';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

function run(args) {
  try {
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

function readMaster() {
  return readFileSync(join(TEST, '.agents/providers.toml'), 'utf8');
}

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond, extra });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

// ---------------------------------------------------------------- 场景 1
console.log('=== 1. 存量纯字符串 models 往返 ===');
reset();
stage(join(REAL, '.pi/agent/auth.json'), join(TEST, '.pi/agent/auth.json'));
stage(join(REAL, '.pi/agent/models.json'), join(TEST, '.pi/agent/models.json'));

const bare = `[[provider]]
id = "bare"
name = "纯字符串"
base_url = "https://bare.example.com/v1"
kind = "anthropic"
api_key = "sk-bare-0123456789abcdef0123456789abcdef"
models = ["m1", "m2"]
agents = ["pi"]
note = ""
`;
writeFileSync(join(TEST, '.agents/providers.toml'), bare, 'utf8');

const listOut = run(['providers', 'list']);
check('能读出纯字符串 models', /m1/.test(listOut) && /m2/.test(listOut));

// 触发一次写盘
run(['providers', 'assign', 'bare', '--agents', 'pi']);
const afterWrite = readMaster();
check(
  '写回后仍是字符串数组',
  /models = \[\s*"m1",\s*"m2",?\s*\]/.test(afterWrite) || /models = \["m1", "m2"\]/.test(afterWrite),
  afterWrite.split('\n').filter((l) => l.includes('models') || l.includes('"m1"') || l.includes('"m2"')).join(' | '),
);
check('没有变成内联表', !/\{\s*id\s*=/.test(afterWrite));

// ---------------------------------------------------------------- 场景 2
console.log('\n=== 2. 带元数据的 models 读写 ===');
reset();
stage(join(REAL, '.pi/agent/auth.json'), join(TEST, '.pi/agent/auth.json'));
stage(join(REAL, '.pi/agent/models.json'), join(TEST, '.pi/agent/models.json'));

const rich = `[[provider]]
id = "rich"
name = "带元数据"
base_url = "https://rich.example.com/v1"
kind = "anthropic"
api_key = "sk-rich-0123456789abcdef0123456789abcdef"
agents = ["pi"]

[[provider.models]]
id = "big"
name = "Big Model"
context_window = 2000000
max_output_tokens = 256000
input_modalities = ["text", "image"]
output_modalities = ["text"]
reasoning = true
reasoning_efforts = ["low", "high"]
default_effort = "high"
`;
writeFileSync(join(TEST, '.agents/providers.toml'), rich, 'utf8');

const richOut = run(['providers', 'list']);
check('能读出带元数据的 models', /big/.test(richOut), richOut.split('\n').slice(0, 12).join(' / '));
if (!/big/.test(richOut)) {
  console.log('  [调试] 母版文件是否存在：', existsSync(join(TEST, '.agents/providers.toml')));
  console.log('  [调试] 母版内容前 200 字：');
  console.log(readMaster().slice(0, 200));
  console.log('  [调试] list 原始输出：');
  console.log(richOut);
}

// ---------------------------------------------------------------- 场景 3
console.log('\n=== 3. 改 context_window 后注入生效 ===');
const assignOut = run(['providers', 'assign', 'rich', '--agents', 'pi']);
console.log('  [assign]', assignOut.trim());
const syncOut = run(['providers', 'sync']);
console.log('  [sync]', syncOut.trim().split('\n').slice(0, 8).join(' / '));

const piModels = readJson('.pi/agent/models.json');
const richDef = piModels.providers?.rich;
check('provider 写进了 pi', !!richDef, richDef ? 'yes' : 'no');

const bigModel = richDef?.models?.find((m) => m.id === 'big');
check('模型 big 存在', !!bigModel);
check('contextWindow 用的是母版值', bigModel?.contextWindow === 2000000, String(bigModel?.contextWindow));
check('maxTokens 用的是母版值', bigModel?.maxTokens === 256000, String(bigModel?.maxTokens));
check(
  'input 用的是母版值',
  JSON.stringify(bigModel?.input) === JSON.stringify(['text', 'image']),
  JSON.stringify(bigModel?.input),
);
check(
  'reasoning 档位写进了 thinkingLevelMap',
  bigModel?.thinkingLevelMap?.high === 'high' && bigModel?.thinkingLevelMap?.minimal === null,
  JSON.stringify(bigModel?.thinkingLevelMap),
);

// ---------------------------------------------------------------- 场景 4
console.log('\n=== 4. 母版留空时不覆盖 agent 已有值 ===');
// 把母版里的 context_window 去掉，只留 id
const bareRich = rich.replace('context_window = 2000000\n', '').replace('max_output_tokens = 256000\n', '');
writeFileSync(join(TEST, '.agents/providers.toml'), bareRich, 'utf8');
run(['providers', 'sync']);

const afterSync = readJson('.pi/agent/models.json');
const bigAfter = afterSync.providers?.rich?.models?.find((m) => m.id === 'big');
check(
  '母版留空后 agent 里的 contextWindow 保持 2000000',
  bigAfter?.contextWindow === 2000000,
  String(bigAfter?.contextWindow),
);
check(
  '母版留空后 agent 里的 maxTokens 保持 256000',
  bigAfter?.maxTokens === 256000,
  String(bigAfter?.maxTokens),
);

// ---------------------------------------------------------------- 场景 5
console.log('\n=== 5. backfill 只填空字段 ===');
reset();
stage(join(REAL, '.pi/agent/auth.json'), join(TEST, '.pi/agent/auth.json'));
stage(join(REAL, '.pi/agent/models.json'), join(TEST, '.pi/agent/models.json'));

// 从真实 pi 配置里挑第一个有 baseUrl、有模型的 provider 造母版
const piModelsRaw = readJson('.pi/agent/models.json');
const piProviders = piModelsRaw.providers || {};
let picked = null;
for (const [id, def] of Object.entries(piProviders)) {
  const base = def.baseUrl || '';
  const modelIds = (def.models || []).map((m) => m.id);
  if (base && modelIds.length > 0) {
    picked = { id, base, modelIds };
    break;
  }
}

if (picked) {
  let toml = `[[provider]]
id = "${picked.id}"
name = "${picked.id}"
base_url = "${picked.base}"
kind = "anthropic"
api_key = "sk-test-0123456789abcdef0123456789abcdef"
agents = ["pi"]
`;
  // 第一个模型手填 context_window（应被保留），其余留空
  toml += `
[[provider.models]]
id = "${picked.modelIds[0]}"
context_window = 999999
`;
  for (const mid of picked.modelIds.slice(1)) {
    toml += `
[[provider.models]]
id = "${mid}"
`;
  }
  writeFileSync(join(TEST, '.agents/providers.toml'), toml, 'utf8');

  const backfillOut = run(['models', 'backfill']);
  check('backfill 有输出', backfillOut.length > 0, backfillOut.split('\n')[0]);

  const masterAfter = readMaster();
  check(
    '手填的 context_window=999999 被保留',
    /999999/.test(masterAfter),
    masterAfter.split('\n').filter((l) => l.includes('999999')).join(' | '),
  );

  // 第二个模型应该被回填了 context_window（如果真实配置里有）
  const secondCtx = piProviders[picked.id]?.models?.[1]?.contextWindow;
  if (secondCtx) {
    check(
      `第二个模型被回填 context_window=${secondCtx}`,
      masterAfter.includes(String(secondCtx)),
      masterAfter.split('\n').filter((l) => l.includes('context_window')).join(' | '),
    );
  } else {
    console.log('SKIP  第二个模型在真实配置里没有 contextWindow，无法验证回填');
  }
} else {
  console.log('SKIP  真实 pi 配置里没有带 baseUrl 的 provider');
}

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
if (failed.length > 0) {
  for (const f of failed) console.log(`  FAIL: ${f.name}${f.extra ? ` — ${f.extra}` : ''}`);
  process.exit(1);
}
