// 查看 WorkBuddy 和 DSH 的 provider 配置结构。
//
// 只报字段名和长度，不打印密钥值。
//
// 用法：node scripts/inspect-workbuddy-dsh.mjs

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const HOME = 'C:/Users/leaf';

// ---- WorkBuddy models.json ----
console.log('=== WorkBuddy models.json ===\n');
try {
  const text = readFileSync(join(HOME, '.workbuddy/models.json'), 'utf8');
  const json = JSON.parse(text);

  if (Array.isArray(json)) {
    console.log(`顶层是数组，${json.length} 个条目\n`);
    json.forEach((m, i) => {
      console.log(`[${i}] 字段：${Object.keys(m).join(', ')}`);
      for (const [k, v] of Object.entries(m)) {
        let shown;
        if (typeof v === 'string') {
          // 密钥类字段只报长度
          if (/key|token|secret/i.test(k)) shown = `<字符串 len=${v.length}>`;
          else shown = v.length > 50 ? `<字符串 len=${v.length}>` : JSON.stringify(v);
        } else if (typeof v === 'object' && v !== null) {
          shown = `{ ${Object.keys(v).join(', ')} }`;
        } else {
          shown = String(v);
        }
        console.log(`    ${k} = ${shown}`);
      }
      console.log('');
    });
  } else {
    console.log('顶层是对象，键：', Object.keys(json).join(', '));
  }
} catch (e) {
  console.log('读取失败：', e.message);
}

// ---- DSH credentials ----
console.log('\n=== DSH .credentials.yaml ===\n');
try {
  const text = readFileSync(join(HOME, '.dsh/.credentials.yaml'), 'utf8');
  // 简单按行分析缩进层级
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    const indent = line.match(/^\s*/)[0].length;
    const colonAt = line.indexOf(':');
    if (colonAt < 0) continue;
    const key = line.slice(0, colonAt).trim();
    const val = line.slice(colonAt + 1).trim();
    const prefix = ' '.repeat(indent);
    if (val) {
      console.log(`${prefix}${key}: <值 len=${val.length}>`);
    } else {
      console.log(`${prefix}${key}:`);
    }
  }
} catch (e) {
  console.log('读取失败：', e.message);
}

// ---- DSH 是否有其他配置 ----
console.log('\n=== DSH 目录内容 ===\n');
import { readdirSync } from 'node:fs';
try {
  const items = readdirSync(join(HOME, '.dsh'), { withFileTypes: true });
  for (const it of items) {
    console.log(`  ${it.isDirectory() ? '[目录]' : '[文件]'} ${it.name}`);
  }
} catch (e) {
  console.log('读取失败：', e.message);
}
