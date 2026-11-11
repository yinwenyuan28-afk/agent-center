// 生成 provider 测试母版，用于验证 diff 和注入。
//
// 从真实 pi auth.json 提取 command provider 的 key，写进临时母版。
// 不打印 key 本身，只报长度和指纹。
//
// 用法：node scripts/make-test-provider.mjs

import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const HOME = 'C:/Users/leaf';
const OUT_DIR = 'C:/temp/provider-test';

const pi = JSON.parse(readFileSync(join(HOME, '.pi/agent/auth.json'), 'utf8'));
const key = pi.command?.key;

if (!key) {
  console.error('pi auth.json 里没有 command provider 的 key');
  process.exit(1);
}

const fp = createHash('sha256').update(key).digest('hex').slice(0, 8);

const toml = `[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "${key}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["claude", "zcode", "pi"]
note = "测试用，2026-11-10 生成"
`;

mkdirSync(OUT_DIR, { recursive: true });
const out = join(OUT_DIR, 'providers.toml');
writeFileSync(out, toml, 'utf8');

console.log(`已生成 ${out}`);
console.log(`key 长度 ${key.length}，指纹 ${fp}`);
