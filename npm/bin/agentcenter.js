#!/usr/bin/env node
/**
 * npm 分发的转发层。
 *
 * 真正的程序是 Rust 编译出来的原生 exe。这个脚本只做三件事：
 * 1. 按平台和架构找到对应的二进制
 * 2. 找不到就给出可操作的提示，而不是抛一个看不懂的堆栈
 * 3. 把参数原样透传，用同样的退出码退出
 *
 * 为什么不在 npm 里放 JS 实现：工具的读写逻辑大量依赖文件系统语义
 * （junction、硬链接、reparse point、权限错误码），JS 在这些地方
 * 拿不到和原生一致的行为，硬做会在 Windows 上出各种静默失败。
 */
const { spawn } = require('node:child_process');
const { existsSync } = require('node:fs');
const path = require('node:path');

/** 当前平台对应的二进制文件名。 */
function binaryName() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === 'win32') {
    return arch === 'arm64' ? 'agentcenter-win32-arm64.exe' : 'agentcenter-win32-x64.exe';
  }
  if (platform === 'darwin') {
    return arch === 'arm64' ? 'agentcenter-darwin-arm64' : 'agentcenter-darwin-x64';
  }
  if (platform === 'linux') {
    return arch === 'arm64' ? 'agentcenter-linux-arm64' : 'agentcenter-linux-x64';
  }
  return null;
}

const name = binaryName();
if (!name) {
  console.error(`不支持的平台：${process.platform} ${process.arch}`);
  process.exit(1);
}

const binPath = path.join(__dirname, '..', 'binaries', name);

if (!existsSync(binPath)) {
  console.error(`找不到平台二进制：${name}`);
  console.error('');
  console.error('这个版本没有为你的平台打包。可以选：');
  console.error('  1. 用源码构建：git clone 后执行 npm run tauri build');
  console.error('  2. 提 issue 说明你的平台');
  process.exit(1);
}

// 用 stdio: 'inherit' 让子进程直接接管终端，
// 这样界面的 GUI 窗口和终端的 CLI 输出都能正常工作
const child = spawn(binPath, process.argv.slice(2), {
  stdio: 'inherit',
  windowsHide: false,
});

child.on('error', (err) => {
  console.error(`启动 agentcenter 失败：${err.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    // 被信号终止时按惯例返回 128 + 信号编号
    process.exit(1);
  }
  process.exit(code ?? 0);
});
