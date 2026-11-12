// 五个「本机未安装」注入器的冒烟测试。
//
// 这些工具本机没有，装不上，所以只能验证「对着一份合理的配置写进去之后
// 结构是对的、且第二次同步零改动」。真实文件格式将来要再核一遍。
//
// 覆盖：Gemini CLI / Grok Build / OpenClaw / Kimi Code / MiniMax Code
//
// 用法：node scripts/test-new-injectors.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const TEST = 'C:/temp/provider-test/new-injectors';
const HOME = join(TEST, 'home');
const MINIMAX_DIR = join(TEST, 'minimax');
const KIMI_DIR = join(TEST, 'kimi');
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

function write(rel, content) {
  const path = join(HOME, rel);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, 'utf8');
  return path;
}

function run() {
  try {
    return execFileSync(EXE, ['providers', 'sync'], {
      env: {
        ...process.env,
        AGENTCENTER_HOME: HOME,
        MINIMAX_DATA_DIR: MINIMAX_DIR,
        MAVIS_DATA_DIR: '',
        KIMI_CODE_HOME: KIMI_DIR,
        // 让没装的 agent 不进候选
        HERMES_HOME: join(TEST, 'hermes-none'),
        DSH_HOME: join(TEST, 'dsh-none'),
      },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

// ---- 准备各家的最小可用配置 ----
if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
mkdirSync(join(HOME, '.agents'), { recursive: true });

// Gemini：注释行必须保留
write(
  '.gemini/.env',
  ['# 我的注释', 'GEMINI_SANDBOX=false', 'GEMINI_API_KEY=old-key-1234567890', ''].join('\n'),
);
write(
  '.gemini/settings.json',
  JSON.stringify({ security: { auth: { selectedType: 'oauth-personal' } }, theme: 'dark' }, null, 2),
);

// Grok：用户自己的表不能被动
write(
  '.grok/config.toml',
  [
    '# 注释要保留',
    '[models]',
    'default = "my-own"',
    '',
    '[model."my-own"]',
    'model = "x-ai/grok-4.7"',
    'base_url = "https://mine.example.com/v1"',
    'name = "我自己配的"',
    'api_key = "sk-mine"',
    'api_backend = "responses"',
    'context_window = 500000',
    '',
  ].join('\n'),
);

// OpenClaw
write(
  '.openclaw/openclaw.json',
  JSON.stringify(
    {
      models: { mode: 'merge', providers: { mine: { baseUrl: 'https://mine.example.com', apiKey: 'sk-mine' } } },
      agents: { defaults: { model: { primary: 'mine/some-model' } } },
    },
    null,
    2,
  ),
);

// Kimi
mkdirSync(KIMI_DIR, { recursive: true });
writeFileSync(
  join(KIMI_DIR, 'config.toml'),
  ['# 注释', 'default_model = "mine"', '', '[providers.mine]', 'type = "openai"', 'base_url = "https://mine.example.com/v1"', 'api_key = "sk-mine"', ''].join('\n'),
  'utf8',
);

// MiniMax
mkdirSync(MINIMAX_DIR, { recursive: true });
writeFileSync(
  join(MINIMAX_DIR, 'config.yaml'),
  [
    '# 注释',
    'defaultModel: custom_provider:mine/m1',
    'custom_provider:',
    '  mine:',
    '    name: 我自己配的',
    '    kind: custom',
    '    enabled: true',
    '    api: openai-completions',
    '    options:',
    '      baseURL: https://mine.example.com/v1',
    '      apiKey: sk-mine',
    '    models:',
    '      m1:',
    '        name: m1',
    '        limit:',
    '          context: 100000',
    '          output: 8000',
    '',
  ].join('\n'),
  'utf8',
);

const master = `[[provider]]
id = "newprov"
name = "New Provider"
base_url = "https://new.example.com/v1"
kind = "openai-chat"
api_key = "sk-new-0123456789abcdef0123456789abcdef"
models = [{ id = "new-model", context_window = 200000, max_output_tokens = 64000 }]
agents = ["gemini", "grok", "openclaw", "kimi", "minimax"]
`;
writeFileSync(join(HOME, '.agents/providers.toml'), master, 'utf8');

console.log('运行同步…');
const out1 = run();
console.log(out1);

// ---------------------------------------------------------------- Gemini
const geminiEnv = readFileSync(join(HOME, '.gemini/.env'), 'utf8');
check('gemini: 写入了 base_url', /GOOGLE_GEMINI_BASE_URL=https:\/\/new\.example\.com\/v1/.test(geminiEnv), geminiEnv.trim().replace(/\n/g, ' | '));
check('gemini: 写入了 key', /GEMINI_API_KEY=sk-new/.test(geminiEnv));
check('gemini: 旧的 key 被替换', !geminiEnv.includes('old-key'));
check('gemini: 注释保留', geminiEnv.includes('# 我的注释'));
check('gemini: CLI 设置保留', geminiEnv.includes('GEMINI_SANDBOX=false'));
const geminiSettings = JSON.parse(readFileSync(join(HOME, '.gemini/settings.json'), 'utf8'));
check('gemini: selectedType 改成 gemini-api-key', geminiSettings.security?.auth?.selectedType === 'gemini-api-key', geminiSettings.security?.auth?.selectedType);
check('gemini: 其他设置保留', geminiSettings.theme === 'dark');

// ---------------------------------------------------------------- Grok
const grok = readFileSync(join(HOME, '.grok/config.toml'), 'utf8');
check('grok: 写了 model 表', /\[model\.agentcenter-grok\]/.test(grok));
check('grok: models.default 指向新表', /^default = "agentcenter-grok"$/m.test(grok));
check('grok: session_summary 同步', /^session_summary = "agentcenter-grok"$/m.test(grok));
check('grok: 用户自己的表还在', /\[model\."my-own"\]/.test(grok));
check('grok: 注释保留', grok.includes('# 注释要保留'));

// ---------------------------------------------------------------- OpenClaw
const openclaw = JSON.parse(readFileSync(join(HOME, '.openclaw/openclaw.json'), 'utf8'));
const ocProv = openclaw.models?.providers ?? {};
check('openclaw: 新 provider 写入', !!ocProv.newprov);
check('openclaw: key 内联', ocProv.newprov?.apiKey?.startsWith('sk-new'));
check('openclaw: 模型元数据写全', ocProv.newprov?.models?.[0]?.contextWindow === 200000 && ocProv.newprov?.models?.[0]?.maxTokens === 64000);
check('openclaw: 用户自己的 provider 保留', !!ocProv.mine);
check('openclaw: 当前模型指针不动', openclaw.agents?.defaults?.model?.primary === 'mine/some-model');

// ---------------------------------------------------------------- Kimi
const kimi = readFileSync(join(KIMI_DIR, 'config.toml'), 'utf8');
check('kimi: provider 写入', /\[providers\.newprov\]/.test(kimi));
check('kimi: type 是 openai', /^type = "openai"$/m.test(kimi));
check('kimi: 模型别名写入', /\[models\.agentcenter-newprov\]/.test(kimi));
check('kimi: 模型别名带 provider 归属', /^provider = "newprov"$/m.test(kimi));
check('kimi: max_context_size 是正整数', /^max_context_size = 200000$/m.test(kimi));
check('kimi: 用户自己的 provider 保留', /\[providers\.mine\]/.test(kimi));
check('kimi: default_model 不主动改', /^default_model = "mine"$/m.test(kimi));
check('kimi: 注释保留', kimi.includes('# 注释'));

// ---------------------------------------------------------------- MiniMax
const minimax = readFileSync(join(MINIMAX_DIR, 'config.yaml'), 'utf8');
check('minimax: provider 写入', /^  newprov:$/m.test(minimax), minimax.split('\n').filter((l) => l.includes('newprov')).join(' | '));
check('minimax: 标了 kind: custom', /^    kind: "custom"$/m.test(minimax));
check('minimax: key 内联', /^      apiKey: sk-new/m.test(minimax));
check('minimax: limit.context 正确', /^          context: 200000$/m.test(minimax));
check('minimax: 用户自己的 provider 保留', /^  mine:$/m.test(minimax));
check('minimax: defaultModel 不动', /^defaultModel: custom_provider:mine\/m1$/m.test(minimax));
check('minimax: 注释保留', minimax.includes('# 注释'));

// ---------------------------------------------------------------- 幂等
console.log('\n第二次同步（幂等验证）…');
const out2 = run();
console.log(out2);
check('第二次同步零写入', /完成：写入 0 个文件/.test(out2), out2.match(/完成：.*/)?.[0] ?? '');

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
for (const f of failed) console.log(`  FAIL: ${f.name}`);
process.exit(failed.length === 0 ? 0 : 1);
