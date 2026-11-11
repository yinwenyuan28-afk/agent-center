// 验证 Hermes / ZCode 的 provider 和 MCP 注入。
//
// 用 AGENTCENTER_HOME 隔离，真实配置不受影响。
// 注意 HERMES_HOME 是独立环境变量，测试时也要指向副本。
//
// 用法：node scripts/test-hermes-zcode.mjs

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REAL = 'C:/Users/leaf';
const REAL_HERMES = 'C:/dev/ide/hermes-agent/Hermes Agent CN Desktop/data/hermes-home';
const TEST = 'C:/temp/provider-test/hermes-home';
const TEST_HERMES = join(TEST, 'hermes');
const EXE = 'C:/dev/code/agentcenter/src-tauri/target/debug/agentcenter.exe';

function run(args) {
  try {
    return execFileSync(EXE, args, {
      env: {
        ...process.env,
        AGENTCENTER_HOME: TEST,
        HERMES_HOME: TEST_HERMES,
      },
      encoding: 'utf8',
    });
  } catch (e) {
    return (e.stdout || '') + (e.stderr || '');
  }
}

function read(rel) {
  return readFileSync(join(TEST, rel), 'utf8');
}

const results = [];
function check(name, cond, extra = '') {
  results.push({ name, ok: !!cond, extra });
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${extra ? ` — ${extra}` : ''}`);
}

function reset() {
  if (existsSync(TEST)) rmSync(TEST, { recursive: true, force: true });
  mkdirSync(join(TEST, '.agents'), { recursive: true });
  mkdirSync(TEST_HERMES, { recursive: true });
  mkdirSync(join(TEST, '.zcode/cli'), { recursive: true });
}

// ---------------------------------------------------------------- Hermes provider
console.log('=== 1. Hermes provider 注入 ===');
reset();
copyFileSync(join(REAL_HERMES, 'config.yaml'), join(TEST_HERMES, 'config.yaml'));

const master = `[[provider]]
id = "testprov"
name = "Test Provider"
base_url = "https://test.example.com/v1"
kind = "openai-chat"
api_key = "sk-test-0123456789abcdef0123456789abcdef"
agents = ["hermes"]

[[provider.models]]
id = "test-model"
context_window = 128000
`;
writeFileSync(join(TEST, '.agents/providers.toml'), master, 'utf8');

const out = run(['providers', 'sync']);
console.log('  [sync]', out.trim().split('\n').slice(0, 4).join(' / '));

const yaml = read('hermes/config.yaml');
check('provider 写进了 config.yaml', /testprov:/.test(yaml));
check('base_url 正确', /base_url: https:\/\/test\.example\.com\/v1/.test(yaml));
check('api_mode 是 chat_completions', /api_mode: chat_completions/.test(yaml));
check('模型写进了 models', /test-model:/.test(yaml));
check('context_length 正确', /context_length: 128000/.test(yaml));
check('原有注释保留', /# ── Security ──/.test(yaml));
check('原有 skills.disabled 保留', /skills:/.test(yaml) && /disabled:/.test(yaml));
check(
  '原有 opencode-go provider 保留',
  /opencode-go:/.test(yaml),
  '这是文件里原有的',
);

// ---------------------------------------------------------------- Hermes MCP
console.log('\n=== 2. Hermes MCP 注入 ===');
const mcpMaster = `[[server]]
id = "testmcp"
name = "Test MCP"
transport = "stdio"
command = "npx"
args = ["-y", "test-mcp-server"]
enabled = true
agents = ["hermes"]

[[server]]
id = "remote-mcp"
name = "Remote MCP"
transport = "http"
url = "https://remote.example.com/mcp"
enabled = true
agents = ["hermes"]
`;
writeFileSync(join(TEST, '.agents/mcp.toml'), mcpMaster, 'utf8');

const mcpOut = run(['mcp', 'sync']);
console.log('  [sync]', mcpOut.trim().split('\n').slice(0, 5).join(' / '));

const yaml2 = read('hermes/config.yaml');
check('mcp_servers 段被创建', /mcp_servers:/.test(yaml2));
check('stdio server 写入', /testmcp:/.test(yaml2) && /command: npx/.test(yaml2));
check('args 写成数组', /args: \["-y", "test-mcp-server"\]/.test(yaml2));
check('remote server 写入 url', /url: https:\/\/remote\.example\.com\/mcp/.test(yaml2));

// ---------------------------------------------------------------- ZCode MCP
console.log('\n=== 3. ZCode MCP 注入 ===');
const zcodeCfg = JSON.stringify({ plugins: { enabled: ['some-plugin'] } }, null, 2);
writeFileSync(join(TEST, '.zcode/cli/config.json'), zcodeCfg, 'utf8');

const zcodeMaster = `[[server]]
id = "zcode-mcp"
name = "ZCode MCP"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "zcode-test"]
enabled = true
agents = ["zcode"]
`;
writeFileSync(join(TEST, '.agents/mcp.toml'), zcodeMaster, 'utf8');

run(['mcp', 'sync']);
const zjson = JSON.parse(read('.zcode/cli/config.json'));
check('保留了原有的 plugins 段', !!zjson.plugins, JSON.stringify(zjson.plugins).slice(0, 60));
check('mcp.servers 两层结构', !!zjson.mcp?.servers);
check('server 写入了', !!zjson.mcp?.servers?.['zcode-mcp']);
check(
  'command 是字符串不是数组',
  typeof zjson.mcp?.servers?.['zcode-mcp']?.command === 'string',
  typeof zjson.mcp?.servers?.['zcode-mcp']?.command,
);
check(
  'args 是数组',
  Array.isArray(zjson.mcp?.servers?.['zcode-mcp']?.args),
  JSON.stringify(zjson.mcp?.servers?.['zcode-mcp']?.args),
);

// ---------------------------------------------------------------- 取消分配
console.log('\n=== 4. 取消分配后移除 ===');
//
// 注意这里的姿势：**保留母版记录，只把 agents 里的 zcode 去掉**。
// 直接删掉整条 mcp.toml 记录的话，注入器就不知道要清理哪些 id 了 ——
// 这是当前设计的边界（池子里没了的 id，一律不动，避免误删用户手配的）。
writeFileSync(
  join(TEST, '.agents/mcp.toml'),
  `[[server]]
id = "zcode-mcp"
name = "ZCode MCP"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "zcode-test"]
enabled = true
agents = []

[[server]]
id = "keep"
name = "Keep"
transport = "stdio"
command = "cmd"
args = ["/c", "echo"]
enabled = true
agents = ["zcode"]
`,
  'utf8',
);
run(['mcp', 'sync']);

const zjson2 = JSON.parse(read('.zcode/cli/config.json'));
check(
  '取消分配后 zcode-mcp 被移除',
  !zjson2.mcp?.servers?.['zcode-mcp'],
  JSON.stringify(Object.keys(zjson2.mcp?.servers || {})),
);
check('新 server 写入', !!zjson2.mcp?.servers?.keep);
check('plugins 段仍然保留', !!zjson2.plugins);

// ---------------------------------------------------------------- 汇总
console.log('\n=== 汇总 ===');
const failed = results.filter((r) => !r.ok);
console.log(`共 ${results.length} 项，通过 ${results.length - failed.length}，失败 ${failed.length}`);
if (failed.length > 0) {
  for (const f of failed) console.log(`  FAIL: ${f.name}${f.extra ? ` — ${f.extra}` : ''}`);
  process.exit(1);
}
