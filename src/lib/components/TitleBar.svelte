<script lang="ts">
  /**
   * 自定义标题栏。
   *
   * 原生标题栏通过 tauri.conf.json 的 `decorations: false` 关掉了，
   * 最小化/最大化/关闭要自己实现。
   *
   * 关键细节：
   * - 拖拽用 `data-tauri-drag-region`，由 Tauri 接管，比手写鼠标事件可靠
   * - 按钮上不能带 drag-region，否则点不动
   * - 浏览器预览时 Tauri API 不存在，整条栏直接不渲染，
   *   因为没有窗口可控制（浏览器自己有标签页）
   */
  import { Minus, Square, X } from '@lucide/svelte';
  import { isNative } from '../ipc';

  let maximized = $state(false);

  /** 拿到当前窗口对象。只在 Tauri 环境里可用。 */
  async function currentWindow() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    return getCurrentWindow();
  }

  async function minimize() {
    if (!isNative) return;
    await (await currentWindow()).minimize();
  }

  async function toggleMaximize() {
    if (!isNative) return;
    const w = await currentWindow();
    await w.toggleMaximize();
    maximized = await w.isMaximized();
  }

  async function close() {
    if (!isNative) return;
    await (await currentWindow()).close();
  }
</script>

{#if isNative}
  <div
    data-tauri-drag-region
    class="flex h-9 shrink-0 select-none items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg-subtle)] pl-3"
  >
    <!-- 左侧：图标 + 名称。整块都可拖拽 -->
    <div data-tauri-drag-region class="flex items-center gap-2">
      <span
        data-tauri-drag-region
        class="flex h-4 w-4 items-center justify-center rounded-[3px] bg-[var(--color-accent)] text-[9px] font-bold text-white"
      >
        A
      </span>
      <span data-tauri-drag-region class="text-[12px] text-[var(--color-text-muted)]">
        agentcenter
      </span>
    </div>

    <!-- 右侧：窗口按钮。不带 drag-region，否则点击会被拖拽吞掉 -->
    <div class="flex h-full">
      <button
        class="flex h-full w-11 items-center justify-center text-[var(--color-text-muted)] transition-colors hover:bg-[var(--color-bg-hover)]"
        title="最小化"
        aria-label="最小化"
        onclick={minimize}
      >
        <Minus size={14} />
      </button>
      <button
        class="flex h-full w-11 items-center justify-center text-[var(--color-text-muted)] transition-colors hover:bg-[var(--color-bg-hover)]"
        title={maximized ? '还原' : '最大化'}
        aria-label={maximized ? '还原' : '最大化'}
        onclick={toggleMaximize}
      >
        <Square size={11} />
      </button>
      <button
        class="flex h-full w-11 items-center justify-center text-[var(--color-text-muted)] transition-colors hover:bg-[#c5341c] hover:text-white"
        title="关闭"
        aria-label="关闭"
        onclick={close}
      >
        <X size={14} />
      </button>
    </div>
  </div>
{/if}
