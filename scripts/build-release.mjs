// 构建 release 版：先停掉可能占用 exe 的进程，再交给 cargo。
//
// 为什么需要这个：Windows 上运行中的 exe 无法被覆盖，
// cargo 会报 "failed to remove file ... 拒绝访问"。
// 每次改完代码想重新构建都得先手动关窗口，很烦。
//
// ## 为什么每次都要清 build script 的 stamp
//
// `tauri_build::build()` 原本只监控 `tauri.conf.json` 和 `capabilities/`，
// 不监控 `frontendDist` 指向的 `../dist`。虽然 build.rs 里补了
// `rerun-if-changed=../dist`，但那条指令本身也存在 stamp 的缓存里 ——
// 第一次加上它的时候旧 stamp 还在，不会重新读。
//
// 症状是「npm run build 成功、cargo 也成功，但 exe 里还是旧前端」，
// 而且 exe 大小一个字节都不变。所以每次构建前主动删掉 build script 的
// 产物目录，强制它重新生成资源。
//
// 只删 `build/agentcenter-*` 和对应的 `.fingerprint`，不动依赖缓存，
// 所以重新编译仍然只需要几十秒到两分钟。
//
// 用法：node scripts/build-release.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';

const root = process.cwd();
const tauriDir = join(root, 'src-tauri');

/** 结束所有名为 agentcenter 的进程。没有在跑时静默通过。 */
function killRunning() {
  try {
    execFileSync('taskkill', ['/IM', 'agentcenter.exe', '/F'], { stdio: 'ignore' });
    console.log('已结束正在运行的 agentcenter 进程');
  } catch {
    // 没有进程在跑，taskkill 会失败，这是正常情况
  }
}

/**
 * 删掉本 crate 的 build script 产物，强制它重新生成嵌入资源。
 *
 * 只清 release profile 下的 `build/agentcenter-*` 和 `.fingerprint/agentcenter-*`。
 * 依赖（tauri、serde 等）的产物全部保留。
 */
function clearBuildScriptStamps() {
  let removed = 0;

  const buildDir = join(tauriDir, 'target', 'release', 'build');
  if (existsSync(buildDir)) {
    for (const name of readdirSync(buildDir)) {
      if (name.startsWith('agentcenter-')) {
        rmSync(join(buildDir, name), { recursive: true, force: true });
        removed++;
      }
    }
  }

  const fpDir = join(tauriDir, 'target', 'release', '.fingerprint');
  if (existsSync(fpDir)) {
    for (const name of readdirSync(fpDir)) {
      if (name.startsWith('agentcenter-')) {
        rmSync(join(fpDir, name), { recursive: true, force: true });
        removed++;
      }
    }
  }

  console.log(`清理 build script 产物：${removed} 项`);
}

/**
 * 构建后校验 exe 里确实嵌入了最新的前端。
 *
 * 光看「cargo 成功」不够 —— 就是它成功而前端没更新才踩的坑。
 *
 * 探针用 **dist 里的 js 文件名**，不用中文文案：Tauri 会把资源压缩后嵌入，
 * 明文搜不到中文，但资源清单里的文件名是原样保留的。
 */
function verifyEmbeddedFrontend(exePath) {
  if (!existsSync(exePath)) {
    throw new Error(`没找到 exe：${exePath}`);
  }

  const distDir = join(root, 'dist', 'assets');
  const indexJs = readdirSync(distDir).find((f) => f.startsWith('index-') && f.endsWith('.js'));
  if (!indexJs) {
    throw new Error('dist/assets 下没有 index-*.js，前端构建可能失败了');
  }

  const exeText = readFileSync(exePath).toString('utf8');
  if (!exeText.includes(indexJs)) {
    // 找出 exe 里实际嵌入的是哪个 index 文件，方便对比
    const embedded = exeText.match(/index-[A-Za-z0-9_-]{8}\.js/)?.[0] ?? '(未找到)';
    throw new Error(
      `exe 里嵌入的前端不是最新的。\n` +
        `  dist 最新产物：${indexJs}\n` +
        `  exe 里实际的：  ${embedded}\n` +
        `  说明 cargo 复用了旧的 build script 产物。` +
        `请删 src-tauri/target/release/build/agentcenter-* 后重试。`,
    );
  }

  const size = statSync(exePath).size;
  console.log(`已验证：exe 内嵌前端为 ${indexJs}（exe ${size} 字节）`);
}

killRunning();

// 先构建前端，让 dist/ 是最新的
console.log('构建前端…');
execFileSync('npm', ['run', 'build'], { stdio: 'inherit', shell: true });

// 再清掉 build script 缓存，保证新 dist 被重新嵌入
clearBuildScriptStamps();

console.log('编译 release…');
execFileSync(
  'cargo',
  ['build', '--release', '--message-format', 'short'],
  {
    cwd: tauriDir,
    stdio: 'inherit',
    shell: true,
    // PATH 里要能找到 cargo；用户环境下通常已经在了，
    // 不在的话从默认安装位置补上
    env: {
      ...process.env,
      PATH: `${process.env.USERPROFILE}\\.cargo\\bin;${process.env.PATH}`,
    },
  },
);

const exe = join(tauriDir, 'target', 'release', 'agentcenter.exe');
verifyEmbeddedFrontend(exe);
console.log(`\n完成：${exe}`);
