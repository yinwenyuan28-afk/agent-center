// 在临时副本上验证 Claude 和 Codex 注入器。
//
// 本脚本随 cc-switch 对齐后的语义更新：
//
// Claude
//   1. 只注入一个 provider（单槽位）
//   2. 不碰 JAVA_HOME / includeCoAuthoredBy 等无关键
//   3. 代理协议（openai-chat / openai-responses）时写本地转换地址 +
//      稳定 claude-* 别名
//   4. 第二次同步零改动（幂等）
//
// Codex
//   5. 单槽位：只有一张 [model_providers.custom]，指向第一个分配的 provider
//   6. key 内联在 experimental_bearer_token，不再有 env_key / codex-env.ps1
//   7. TOML 注释与用户表（mcp_servers / plugins）保留
//   8. 第二次同步零改动（幂等）
//
// 用法：node scripts/test-claude-codex.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/home3';
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

function stage(src, dst) {
  if (!existsSync(src)) return false;
  mkdirSync(join(dst, '..'), { recursive: true });
  copyFileSync(src, dst);
  return true;
}

// ---- 准备临时副本 ----
if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
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
kind = "openai-responses"
api_key = "${commandKey}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["claude", "codex"]
codex_supports_websockets = false
codex_model_catalog_url = "https://api.commandcode.ai/provider/v1/models"

[[provider]]
id = "codex-alt"
name = "Codex 备用"
base_url = "https://codex-alt.example.com/v1"
kind = "openai-responses"
api_key = "sk-codexalt-1234567890abcdef"
models = ["codex-model"]
agents = ["codex"]
`;

mkdirSync(join(TEST, '.agents'), { recursive: true });
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

const claudeBefore = JSON.parse(readFileSync(join(claudeDir, 'settings.json'), 'utf8'));
const codexBefore = readFileSync(join(codexDir, 'config.toml'), 'utf8');
const commentsBefore = (codexBefore.match(/^\s*#/gm) || []).length;

console.log('\n运行同步…');
const out1 = run();
console.log(out1);

// ---------------------------------------------------------------- Claude
console.log('=== Claude 验证 ===');
const claudeAfter = JSON.parse(readFileSync(join(claudeDir, 'settings.json'), 'utf8'));

// 1. 顶层无关键保留
const topBefore = Object.keys(claudeBefore).filter((k) => k !== 'env').sort();
const topAfter = Object.keys(claudeAfter).filter((k) => k !== 'env').sort();
check('顶层无关键完整保留', JSON.stringify(topBefore) === JSON.stringify(topAfter), topAfter.join(','));

// 2. env 里的无关键（JAVA_HOME）保留
check('JAVA_HOME 保留', claudeAfter.env?.JAVA_HOME === claudeBefore.env?.JAVA_HOME);

// 3. 代理模式：base URL 指向本地转换，模型写稳定别名
check(
  'ANTHROPIC_BASE_URL 指向本地转换',
  claudeAfter.env?.ANTHROPIC_BASE_URL === 'http://127.0.0.1:15722',
  claudeAfter.env?.ANTHROPIC_BASE_URL,
);
check(
  'OPUS 档位写稳定别名 claude-opus-5',
  claudeAfter.env?.ANTHROPIC_DEFAULT_OPUS_MODEL === 'claude-opus-5',
  claudeAfter.env?.ANTHROPIC_DEFAULT_OPUS_MODEL,
);
check(
  'ANTHROPIC_MODEL 写稳定别名',
  claudeAfter.env?.ANTHROPIC_MODEL === 'claude-opus-5',
  claudeAfter.env?.ANTHROPIC_MODEL,
);
check(
  'ANTHROPIC_AUTH_TOKEN 用的是母版 key',
  claudeAfter.env?.ANTHROPIC_AUTH_TOKEN === commandKey,
);
check(
  '不再写 ANTHROPIC_API_KEY',
  claudeAfter.env?.ANTHROPIC_API_KEY === undefined,
);

// 4. 只注入第一个 provider，第二个不出现
check(
  '没有注入第二个 provider（codex-alt）',
  !JSON.stringify(claudeAfter).includes('codex-alt'),
);

// ---------------------------------------------------------------- Codex
console.log('\n=== Codex 验证 ===');
const codexAfter = readFileSync(join(codexDir, 'config.toml'), 'utf8');

// 5. 注释与用户表保留
const commentsAfter = (codexAfter.match(/^\s*#/gm) || []).length;
check('注释行保留', commentsAfter >= commentsBefore, `${commentsBefore} → ${commentsAfter}`);
for (const tbl of ['mcp_servers', 'projects', 'plugins', 'desktop']) {
  if (codexBefore.includes(`[${tbl}`)) {
    check(`${tbl} 表保留`, codexAfter.includes(`[${tbl}`));
  }
}

// 6. 单槽位：只有 custom 表，且指向第一个分配的 provider
check('写了 model_providers.custom', /\[model_providers\.custom\]/.test(codexAfter));
check('model_provider 指向 custom', /^model_provider = "custom"$/m.test(codexAfter));
check(
  'custom 表的 base_url 是 command',
  /base_url = "https:\/\/api\.commandcode\.ai\/provider\/v1"/.test(codexAfter),
);

// 7. key 内联，没有 env_key
check(
  'key 内联在 experimental_bearer_token',
  codexAfter.includes('experimental_bearer_token'),
);
check('不再写 env_key', !/^env_key\s*=/m.test(codexAfter));
check(
  '不再生成 codex-env.ps1',
  !existsSync(join(TEST, '.agents/codex-env.ps1')),
);

// 8. 第二个 provider 不该有自己的表
check('没有为 codex-alt 建表', !codexAfter.includes('[model_providers.codex-alt]'));

// 9. 连接参数：关 WebSocket、补模型目录
check('supports_websockets = false', /^supports_websockets = false$/m.test(codexAfter));
check(
  'model_catalog_url 写入',
  /^model_catalog_url = "https:\/\/api\.commandcode\.ai\/provider\/v1\/models"$/m.test(codexAfter),
);

// ---------------------------------------------------------------- 幂等
console.log('\n第二次同步（幂等验证）…');
const out2 = run();
console.log(out2);
check('第二次同步零写入', /完成：写入 0 个文件/.test(out2), out2.match(/完成：.*/)?.[0] ?? '');

// ---------------------------------------------------------------- 取消 Codex 分配
console.log('\n取消 Codex 分配后…');
const masterNoCodex = master
  .replace('agents = ["claude", "codex"]', 'agents = ["claude"]')
  .replace(/\[\[provider\]\]\nid = "codex-alt"[\s\S]*$/, '');
writeFileSync(join(TEST, '.agents/providers.toml'), masterNoCodex, 'utf8');
const out3 = run();
console.log(out3);
const codexCleared = readFileSync(join(codexDir, 'config.toml'), 'utf8');
check('custom 表已移除', !codexCleared.includes('[model_providers.custom]'));
check('model_provider 已清除', !/^model_provider\s*=/m.test(codexCleared));
check('用户表仍保留', codexCleared.includes('[mcp_servers'));

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
for (const f of failed) console.log(`  FAIL: ${f.name}`);
process.exit(failed.length === 0 ? 0 : 1);
