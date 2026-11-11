// 把母版里 id 为 minimax 和 minimax-cn 的两条合并成一条。
//
// 合并规则：
// - 保留 minimax-cn（端点 api.minimax.cn/anthropic，ZCode 在用）
// - 删掉 minimax 那条
// - agents 合并成 ["pi", "zcode"]
// - models 合并去重
//
// 为什么用文本操作而不是解析再序列化：母版里有明文 key，
// 重新序列化会把用户手写的注释和排版全丢掉。这里只做块级替换。
//
// 用法：node scripts/merge-minimax.mjs

import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';

const MASTER = 'C:/Users/leaf/.agents/providers.toml';

const text = readFileSync(MASTER, 'utf8');

// 按 [[provider]] 切块
const parts = text.split(/(?=\[\[provider\]\])/);
const header = parts[0];
const blocks = parts.slice(1);

/** 从块里读某个键的字符串值。 */
function getStr(block, key) {
  const m = block.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm'));
  return m ? m[1] : '';
}

/** 从块里读某个数组键的所有元素。 */
function getArr(block, key) {
  const m = block.match(new RegExp(`^${key}\\s*=\\s*\\[([^\\]]*)\\]`, 'm'));
  if (!m) return [];
  return [...m[1].matchAll(/"([^"]*)"/g)].map((x) => x[1]);
}

const idxCn = blocks.findIndex((b) => getStr(b, 'id') === 'minimax-cn');
const idxPlain = blocks.findIndex((b) => getStr(b, 'id') === 'minimax');

if (idxCn < 0) {
  console.error('找不到 minimax-cn');
  process.exit(1);
}
if (idxPlain < 0) {
  console.log('已经没有独立的 minimax 条目，无需合并');
  process.exit(0);
}

const cn = blocks[idxCn];
const plain = blocks[idxPlain];

// 合并 agents 和 models
const agents = [...new Set([...getArr(cn, 'agents'), ...getArr(plain, 'agents')])];
const models = [...new Set([...getArr(cn, 'models'), ...getArr(plain, 'models')])];

// 重写 minimax-cn 块的 agents / models / note
let merged = cn
  .replace(/^agents\s*=\s*\[[^\]]*\]/m, `agents = [${agents.map((a) => `"${a}"`).join(', ')}]`)
  .replace(/^models\s*=\s*\[[^\]]*\]/m, `models = [${models.map((m) => `"${m}"`).join(', ')}]`)
  .replace(/^note\s*=\s*"[^"]*"/m, `note = "合并自 pi/opencode 与 zcode，2026-11-10"`);

// 统一 name 更可读
merged = merged.replace(/^name\s*=\s*"[^"]*"/m, `name = "MiniMax 中文站"`);

// 组装：cn 块用合并后的，删掉 plain 块
const out = [header, ...blocks.map((b, i) => (i === idxCn ? merged : b))].filter(
  (_, i) => i !== idxPlain + 1,
);

// 备份
copyFileSync(MASTER, MASTER + '.bak');
writeFileSync(MASTER, out.join(''), 'utf8');

console.log('已合并 minimax-cn 与 minimax');
console.log(`  端点：${getStr(merged, 'base_url')}`);
console.log(`  agents：${agents.join(', ')}`);
console.log(`  models：${models.length} 个`);
console.log(`  备份：${MASTER}.bak`);
