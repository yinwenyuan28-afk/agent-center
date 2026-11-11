// 释放指定端口上正在监听的进程。
//
// 用途：Tauri 的 beforeDevCommand 会和手动启动的 vite 抢同一个端口，
// 端口被占时 tauri dev 直接失败。这个脚本用于清理。
//
// 用法：node scripts/free-port.mjs 5731

import { execSync } from 'node:child_process';

const port = process.argv[2] ?? '5731';

let out = '';
try {
  out = execSync(`netstat -ano -p TCP`, { encoding: 'utf8' });
} catch {
  console.log('无法执行 netstat');
  process.exit(0);
}

const pids = new Set();
for (const line of out.split('\n')) {
  // 只处理监听行，且只匹配目标端口
  if (!line.includes('LISTENING')) continue;
  const cols = line.trim().split(/\s+/);
  const local = cols[1] ?? '';
  const pid = cols[4] ?? '';
  if (!local.endsWith(`:${port}`)) continue;
  if (pid && pid !== '0') pids.add(pid);
}

if (pids.size === 0) {
  console.log(`端口 ${port} 没有监听进程`);
  process.exit(0);
}

for (const pid of pids) {
  try {
    execSync(`taskkill /PID ${pid} /F`, { stdio: 'ignore' });
    console.log(`已结束进程 ${pid}（端口 ${port}）`);
  } catch {
    console.log(`结束进程 ${pid} 失败`);
  }
}
