// 往测试母版里加一条 provider，用于强制触发一次真实写入。
//
// 目的：验证 toml_edit 在真正改写文件时是否保留注释。
// 之前的测试因为"无变化"跳过了写入，什么都没验证到。
//
// 用法：node scripts/add-comment-test-provider.mjs

import { readFileSync, writeFileSync } from 'node:fs';

const MASTER = 'C:/temp/provider-test/home3/.agents/providers.toml';

let text = readFileSync(MASTER, 'utf8');

if (text.includes('comment-test')) {
  console.log('comment-test 已存在，无需重复添加');
  process.exit(0);
}

text += `
[[provider]]
id = "comment-test"
name = "注释保留测试"
base_url = "https://comment-test.example.com/v1"
kind = "openai-compatible"
api_key = "sk-commenttest-1234567890ab"
models = ["cm"]
agents = ["codex"]
`;

writeFileSync(MASTER, text, 'utf8');

const count = (text.match(/\[\[provider\]\]/g) || []).length;
console.log(`已加入 comment-test，母版现在有 ${count} 个 provider`);
