<script lang="ts">
  /**
   * Git 页：~/.agents 的版本管理。
   *
   * 核心约束：绝不自动提交，绝不用 git add -A。
   * 提交框默认只列出勾选的路径，用户不勾的不会被写进暂存区。
   */
  import { onMount } from 'svelte';
  import { RefreshCw, GitBranch, ArrowUp, ArrowDown } from '@lucide/svelte';
  import { gitCommit, gitPush, gitStatus } from '../lib/ipc';
  import type { GitStatus } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';

  let status = $state<GitStatus | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let message = $state('');
  let checked = $state<Set<string>>(new Set());
  let log = $state('');

  async function load() {
    loading = true;
    try {
      status = await gitStatus();
      // 每次都清空勾选，强迫用户至少看一眼要提交什么
      checked = new Set();
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  function toggle(path: string) {
    const next = new Set(checked);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    checked = next;
  }

  function toggleAll() {
    if (!status) return;
    checked =
      checked.size === status.changed.length
        ? new Set()
        : new Set(status.changed.map((c) => c.path));
  }

  async function commit() {
    if (!message.trim() || checked.size === 0) return;
    busy = true;
    try {
      const r = await gitCommit([...checked], message.trim());
      log = r.output;
      if (r.ok) {
        message = '';
        await load();
      }
    } catch (e) {
      log = `提交失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function push() {
    busy = true;
    try {
      const r = await gitPush();
      log = r.output;
      await load();
    } catch (e) {
      log = `推送失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  onMount(load);

  /** 状态码转中文，保留原始码在 title 里方便对 git 文档。 */
  function statusLabel(code: string): { text: string; tone: 'ok' | 'warn' | 'danger' | 'info' | 'plain' } {
    if (code === '??') return { text: '未跟踪', tone: 'info' };
    if (code.includes('M')) return { text: '修改', tone: 'warn' };
    if (code.includes('A')) return { text: '新增', tone: 'ok' };
    if (code.includes('D')) return { text: '删除', tone: 'danger' };
    if (code.includes('R')) return { text: '改名', tone: 'info' };
    return { text: code, tone: 'plain' };
  }
</script>

<PageHead title="Git">
  <code class="font-[var(--font-mono)] text-[12px]">~/.agents</code> 的版本状态。提交只包含你勾选的路径，工具不会使用 <code class="font-[var(--font-mono)] text-[12px]">git add -A</code>，也不会自动提交。
</PageHead>

{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(4) as _}
      <div class="h-9 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else if !status}
  <div class="py-12 text-center text-[13px] text-[var(--color-text-faint)]">读取不到 git 状态。</div>
{:else if !status.isRepo}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    <code class="font-[var(--font-mono)]">~/.agents</code> 还不是 git 仓库。先执行
    <code class="font-[var(--font-mono)]">git init</code> 与
    <code class="font-[var(--font-mono)]">git remote add origin ...</code>。
  </div>
{:else}
  <!-- 仓库概览 -->
  <div class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
    <div class="flex flex-wrap items-center gap-x-8 gap-y-3">
      <div>
        <div class="text-[11px] text-[var(--color-text-muted)]">分支</div>
        <div class="flex items-center gap-1 font-[var(--font-mono)] text-[13px]">
          <GitBranch size={13} class="text-[var(--color-text-faint)]" />
          {status.branch}
        </div>
      </div>
      <div>
        <div class="text-[11px] text-[var(--color-text-muted)]">领先 / 落后</div>
        <div class="flex items-center gap-3 font-[var(--font-mono)] text-[13px]">
          <span class="flex items-center gap-0.5">
            <ArrowUp size={12} class="text-[var(--color-text-faint)]" />{status.ahead}
          </span>
          <span class="flex items-center gap-0.5">
            <ArrowDown size={12} class="text-[var(--color-text-faint)]" />{status.behind}
          </span>
        </div>
      </div>
      <div class="min-w-0">
        <div class="text-[11px] text-[var(--color-text-muted)]">远端</div>
        <div class="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-muted)]">
          {status.remote || '未配置'}
        </div>
      </div>
      <span class="flex-1"></span>
      <div class="flex gap-2">
        <Button variant="ghost" size="sm" onclick={load} disabled={busy}>
          <RefreshCw size={13} />
          刷新
        </Button>
        <Button size="sm" onclick={push} disabled={busy || status.ahead === 0}>
          推送（{status.ahead}）
        </Button>
      </div>
    </div>
  </div>

  <!-- 改动清单 -->
  <div class="mb-3 flex items-center gap-2">
    <strong class="text-[13px]">改动文件（{status.changed.length}）</strong>
    {#if status.changed.length > 0}
      <Button variant="ghost" size="sm" onclick={toggleAll}>
        {checked.size === status.changed.length ? '全不选' : '全选'}
      </Button>
    {/if}
    <span class="flex-1"></span>
    <span class="text-[12px] text-[var(--color-text-muted)]">已选 {checked.size} 个</span>
  </div>

  {#if status.changed.length === 0}
    <div
      class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] py-12 text-center text-[13px] text-[var(--color-text-faint)]"
    >
      工作区干净，没有未提交的改动。
    </div>
  {:else}
    <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
      <table class="w-full border-collapse text-[13px]">
        <thead>
          <tr
            class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
          >
            <th class="w-8 px-4 py-2.5"></th>
            <th class="w-20 px-4 py-2.5 font-medium">状态</th>
            <th class="px-4 py-2.5 font-medium">路径</th>
          </tr>
        </thead>
        <tbody>
          {#each status.changed as c (c.path)}
            {@const s = statusLabel(c.status)}
            <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
              <td class="px-4 py-2.5">
                <input
                  type="checkbox"
                  class="accent-[var(--color-accent)]"
                  checked={checked.has(c.path)}
                  onchange={() => toggle(c.path)}
                  aria-label={`选择 ${c.path}`}
                />
              </td>
              <td class="px-4 py-2.5"><Badge tone={s.tone} title={c.status}>{s.text}</Badge></td>
              <td class="px-4 py-2.5 font-[var(--font-mono)] text-[12px] text-[var(--color-text-muted)]">
                {c.path}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <!-- 提交 -->
  <div class="mt-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
    <h2 class="mb-1 text-[14px] font-semibold">提交</h2>
    <p class="mb-3 text-[12px] text-[var(--color-text-muted)]">
      只提交已勾选的 {checked.size} 个路径。
    </p>
    <textarea
      class="w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-3 py-2 font-[var(--font-mono)] text-[12px] text-[var(--color-text)] placeholder:text-[var(--color-text-faint)] focus:border-transparent focus:outline-2 focus:outline-[var(--color-accent)]"
      rows="3"
      bind:value={message}
      placeholder="提交信息，例如：feat(skills): 收进两个 cursor 里的 skill"
    ></textarea>
    <div class="mt-3">
      <Button
        variant="primary"
        onclick={commit}
        disabled={busy || !message.trim() || checked.size === 0}
      >
        {busy ? '提交中…' : '提交选中路径'}
      </Button>
    </div>
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} />
  </div>
{/if}
