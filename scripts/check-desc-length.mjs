// 统计每个 skill 的 description 长度，用于确定表格列的合理截断值。
//
// 用法：node scripts/check-desc-length.mjs

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';

const root = join(homedir(), '.agents', 'skills');
const rows = [];

for (const name of readdirSync(root)) {
  let isDir = false;
  try {
    isDir = statSync(join(root, name)).isDirectory();
  } catch {
    continue;
  }
  if (!isDir) continue;

  const p = join(root, name, 'SKILL.md');
  let text;
  try {
    text = readFileSync(p, 'utf8');
  } catch {
    continue;
  }

  const fm = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  let desc = '';
  if (fm) {
    const line = fm[1].split(/\r?\n/).find((l) => l.trim().startsWith('description:'));
    if (line) {
      desc = line.replace(/^\s*description:\s*/, '').trim().replace(/^["']|["']$/g, '');
    }
  }
  rows.push({ name, len: desc.length, desc });
}

rows.sort((a, b) => b.len - a.len);
console.log('名称                          长度  描述开头');
for (const r of rows) {
  console.log(
    r.name.padEnd(28) + String(r.len).padStart(5) + '  ' + r.desc.slice(0, 50),
  );
}
console.log('\n共 ' + rows.length + ' 个，最长 ' + (rows[0]?.len ?? 0) + ' 字符');
