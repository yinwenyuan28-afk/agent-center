// 在临时副本上验证 ZCode 注入器，不碰真实配置。
//
// 做法：
// 1. 把真实 config.json 复制到临时目录
// 2. 用同样的内容建一个假的 HOME 结构，让注入器写到副本上
// 3. 对比前后差异，确认只有目标字段变化
//
// 这个脚本靠设置 HOME 环境变量来重定向，不能直接改真实文件。
//
// 用法：node scripts/test-zcode-inject.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL_HOME = 'C:/Users/leaf';
const TEST_ROOT = 'C:/temp/provider-test/home';

// 准备一个假的 HOME，里面放 ZCode 的配置副本
const fakeZcodeDir = join(TEST_ROOT, '.zcode', 'v2');
mkdirSync(fakeZcodeDir, { recursive: true });

const realConfig = join(REAL_HOME, '.zcode', 'v2', 'config.json');
const testConfig = join(fakeZcodeDir, 'config.json');

if (!existsSync(realConfig)) {
  console.error('真实配置不存在，跳过');
  process.exit(1);
}

copyFileSync(realConfig, testConfig);
const before = readFileSync(testConfig, 'utf8');
console.log(`已复制到副本：${testConfig}`);
console.log(`原始大小 ${before.length} 字节`);

// 准备测试母版（指向假 HOME）
const masterDir = join(TEST_ROOT, '.agents');
mkdirSync(masterDir, { recursive: true });

// 从 pi 的真实 auth.json 取 key 来构造母版
const piAuth = JSON.parse(readFileSync(join(REAL_HOME, '.pi', 'agent', 'auth.json'), 'utf8'));
const commandKey = piAuth.command.key;

const masterToml = `[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"
api_key = "${commandKey}"
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["zcode"]
note = "注入测试"

[[provider]]
id = "test-new-provider"
name = "测试新增"
base_url = "https://example.com/v1"
kind = "anthropic"
api_key = "sk-test-1234567890abcdefghij"
models = ["test-model"]
agents = ["zcode"]
note = "验证新增能力"
`;

mkdirSync(masterDir, { recursive: true });
writeFileSync(join(masterDir, 'providers.toml'), masterToml, 'utf8');

// 用 USERPROFILE 重定向（Windows 上 dirs::home_dir 读的是这个）
console.log('\n在假 HOME 上运行同步...\n');

try {
  const out = execFileSync(
    join('C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe'),
    ['providers', 'sync'],
    {
      env: { ...process.env, AGENTCENTER_HOME: TEST_ROOT },
      encoding: 'utf8',
    },
  );
  console.log(out);
} catch (e) {
  console.log('执行输出：');
  console.log(e.stdout || '');
  console.log('错误：');
  console.log(e.stderr || e.message);
}

// 对比前后
const after = existsSync(testConfig) ? readFileSync(testConfig, 'utf8') : '';
console.log(`\n=== 结果 ===`);
console.log(`原始大小 ${before.length} 字节 → 现在 ${after.length} 字节`);

const beforeJson = JSON.parse(before);
const afterJson = JSON.parse(after);

const beforeProviders = Object.keys(beforeJson.provider ?? {});
const afterProviders = Object.keys(afterJson.provider ?? {});

console.log(`provider 数量：${beforeProviders.length} → ${afterProviders.length}`);

// 检查顶层其他键有没有被动
const beforeTop = Object.keys(beforeJson).sort();
const afterTop = Object.keys(afterJson).sort();
console.log(`顶层键：${JSON.stringify(beforeTop)} → ${JSON.stringify(afterTop)}`);
console.log(`顶层键是否一致：${JSON.stringify(beforeTop) === JSON.stringify(afterTop)}`);

// 确认新增的 provider 出现了
const added = afterProviders.filter((k) => !beforeProviders.includes(k));
console.log(`新增的键：${added.length ? added.join(', ') : '（无）'}`);

// 检查原有 provider 的 models 字段有没有被动
let modelsUntouched = true;
for (const k of beforeProviders) {
  if (!afterJson.provider[k]) {
    console.log(`  !! 原有 provider ${k} 消失了`);
    modelsUntouched = false;
    continue;
  }
  const bm = Object.keys(beforeJson.provider[k].models ?? {}).length;
  const am = Object.keys(afterJson.provider[k].models ?? {}).length;
  if (bm !== am) {
    console.log(`  !! ${k} 的 models 数量 ${bm} → ${am}`);
    modelsUntouched = false;
  }
}
console.log(`原有 provider 的 models 未被改动：${modelsUntouched}`);
