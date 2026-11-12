// 前后端类型契约检查。
//
// 这是本仓库唯一一处跨语言耦合：Rust 用 serde 序列化，前端用 TypeScript 消费。
// 字段名不一致时不会编译报错，只会在运行时拿到 undefined —— 这类 bug 最难查。
//
// 所以用一个零依赖脚本在两边之间做机械比对，`npm run check:contract` 即可跑。
//
// 用法：node scripts/check-contract.mjs
// 退出码 0 表示一致，1 表示有差异（差异会打印到 stdout）。

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

const tsText = readFileSync(join(root, 'src', 'lib', 'types.ts'), 'utf8');
const rsText = readFileSync(join(root, 'src-tauri', 'src', 'types.rs'), 'utf8');

/** 从 TypeScript 源码里抽出 interface 名与其字段。 */
function parseTsInterfaces(text) {
  const out = new Map();
  const re = /export interface (\w+) \{([\s\S]*?)\r?\n\}/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const fields = [...m[2].matchAll(/^\s{2}(\w+)\??:/gm)].map((x) => x[1]);
    out.set(m[1], fields);
  }
  return out;
}

/** 从 Rust 源码里抽出 struct 名、字段，以及是否用了 camelCase 序列化。 */
function parseRustStructs(text) {
  const out = new Map();
  const re = /pub struct (\w+) \{([\s\S]*?)\r?\n\}/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    // 检查这个 struct 上方 120 字符内有没有 camelCase 属性
    const before = text.slice(Math.max(0, m.index - 120), m.index);
    const camel = /rename_all = "camelCase"/.test(before);

    // 逐个字段抽取。字段级 `#[serde(rename = "default")]` 优先于
    // struct 级的 rename_all —— 契约检查必须认得这种单字段改名，
    // 否则会把 `default_model` 误报成多出来的字段。
    const fields = [];
    for (const fm of m[2].matchAll(/pub (?:r#)?(\w+):/g)) {
      const name = fm[1];
      const head = m[2].slice(Math.max(0, fm.index - 200), fm.index);
      const rename = head.match(/rename\s*=\s*"([^"]+)"/)?.[1];
      fields.push(rename ?? name);
    }
    out.set(m[1], { fields, camel });
  }
  return out;
}

/** 把 snake_case 转成 camelCase。 */
const toCamel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

const ts = parseTsInterfaces(tsText);
const rs = parseRustStructs(rsText);

const problems = [];

// 前端用到的每个接口，Rust 侧都必须有对应结构体
for (const [name, tsFields] of ts) {
  const r = rs.get(name);
  if (!r) {
    problems.push(`${name}: Rust 侧缺少同名 struct`);
    continue;
  }

  // Rust 若没声明 camelCase，实际发出去的字段名是 snake_case，与前端对不上
  if (!r.camel) {
    const snakeish = r.fields.filter((f) => f.includes('_'));
    if (snakeish.length > 0) {
      problems.push(
        `${name}: Rust 结构体未声明 #[serde(rename_all = "camelCase")]，` +
          `这些字段会被发成 snake_case：${snakeish.join(', ')}`,
      );
    }
  }

  const rsFields = r.camel ? r.fields.map(toCamel) : r.fields;
  const missing = tsFields.filter((f) => !rsFields.includes(f));
  const extra = rsFields.filter((f) => !tsFields.includes(f));

  if (missing.length) problems.push(`${name}: Rust 缺少字段 ${missing.join(', ')}`);
  if (extra.length) problems.push(`${name}: Rust 多出字段 ${extra.join(', ')}`);
}

// Rust 有、前端没用的结构体不算错误，只提示
const tsNames = new Set(ts.keys());
const unused = [...rs.keys()].filter((k) => !tsNames.has(k) && k !== 'ResolvedMapping');

if (problems.length === 0) {
  console.log(`契约一致：${ts.size} 个类型，${[...ts.values()].reduce((n, f) => n + f.length, 0)} 个字段`);
  if (unused.length) console.log(`提示：这些 struct 只在前端之外使用 —— ${unused.join(', ')}`);
  process.exit(0);
}

console.error('前后端类型不一致：');
for (const p of problems) console.error(`  - ${p}`);
process.exit(1);
