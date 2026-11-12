// 用真实母版在副本上跑一次 Pi 注入，再用 Pi 自己的 typebox schema 校验结果。
//
// 只写 C:/temp 下的副本，不碰真实配置。
// 用法：node scripts/verify-pi-schema.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const REAL = 'C:/Users/leaf';
const TEST = 'C:/temp/provider-test/pi-schema';
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

const PI_ROOT = 'C:/dev/env/fnm/dir/aliases/default/node_modules/@earendil-works/pi-coding-agent';
const MODEL_CONFIG = join(PI_ROOT, 'dist/core/model-config.js');

if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
mkdirSync(join(TEST, '.agents'), { recursive: true });
mkdirSync(join(TEST, '.pi/agent'), { recursive: true });
copyFileSync(join(REAL, '.agents/providers.toml'), join(TEST, '.agents/providers.toml'));
copyFileSync(join(REAL, '.pi/agent/models.json'), join(TEST, '.pi/agent/models.json'));

console.log('用真实母版跑一次 Pi 注入（只写副本）…\n');
const out = execFileSync(EXE, ['providers', 'sync'], {
  env: { ...process.env, AGENTCENTER_HOME: TEST },
  encoding: 'utf8',
});
for (const line of out.split('\n')) {
  if (line.includes('pi:') || line.includes('完成：')) console.log(line);
}

// 用 Pi 自己的 ModelConfig 加载副本，它会走真正的 typebox 校验。
// ModelConfig 的构造是私有的，这里直接调它的静态加载路径。
const { ModelConfig } = await import(pathToFileURL(MODEL_CONFIG).href);

const modelsPath = join(TEST, '.pi/agent/models.json');
const config = await ModelConfig.load(modelsPath);
const error = config.getError?.() ?? config.error ?? null;
const ids = config.getProviderIds();

console.log('\nPi 加载结果：');
console.log('  providers:', ids.length ? ids.join(', ') : '（空）');
if (error) {
  console.log('  ERROR:', String(error).split('\n').slice(0, 6).join('\n         '));
}

// 额外做一次显式检查，避免上面的 API 名字对不上时静默通过
const json = JSON.parse(readFileSync(modelsPath, 'utf8'));
const bad = [];
for (const [id, prov] of Object.entries(json.providers ?? {})) {
  for (const m of prov.models ?? []) {
    for (const mod of m.input ?? []) {
      if (mod !== 'text' && mod !== 'image') bad.push(`${id}/${m.id}: ${mod}`);
    }
  }
}

console.log('\n直接检查 input 取值：');
console.log(bad.length === 0 ? '  全部合法（只有 text / image）' : '  非法：' + bad.join(', '));

const ok = !error && bad.length === 0;
console.log('\n结论：', ok ? '通过' : '未通过');
process.exit(ok ? 0 : 1);
