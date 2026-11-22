<script lang="ts">
  /**
   * Skills 更新页：展示 npx skills 管理的第三方 skill 及其更新状态。
   *
   * 关键约束（来自对 skills CLI 1.7.6 的实测）：
   * - 它记录的"版本"是内容哈希，不是版本号，所以这里不展示 v1.2.3 这种假象
   * - check 有更新时退出码为 1，靠退出码判断，不解析彩色文本
   * - update 之后必须重跑同步，因为 CLI 会按自己的目录表写文件，可能覆盖掉链接
   */
  import { onMount } from 'svelte';
  import { RefreshCw } from '@lucide/svelte';
  import { checkUpdates, listManaged, updateSkills } from '../lib/ipc';
  import type { ManagedSkill } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Tip from '../lib/components/Tip.svelte';
  import Log from '../lib/components/Log.svelte';

  let managed = $state<ManagedSkill[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');
  let selected = $state<Set<string>>(new Set());

  async function load() {
    loading = true;
    try {
      managed = await listManaged();
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  async function check() {
    busy = true;
    try {
      const r = await checkUpdates();
      log = r.output;
      // 退出码 1 表示检测到更新，不是失败
      if (r.exitCode === 1) {
        managed = managed.map((m) => ({ ...m, hasUpdate: r.output.includes(m.name) }));
      } else if (r.ok) {
        managed = managed.map((m) => ({ ...m, hasUpdate: false }));
      }
    } catch (e) {
      log = `检查失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function update(names: string[]) {
    busy = true;
    try {
      const r = await updateSkills(names);
      log = `${r.output}\n\n注意：skills CLI 会按它自己的目录表写文件，可能已经覆盖了软链接。\n请回到「AGENTS文档映射」页执行一次同步以恢复链接。`;

      // 更新完把列表重新读一遍：lock 文件里的 hash 和 updatedAt 都变了，
      // 不重读的话界面还显示旧的指纹和日期。
      await load();

      // 明确把被更新的条目状态改成「已最新」——
      // 否则它们会退回「未检查」，看不出刚才的操作生效了
      const done = new Set(names);
      managed = managed.map((m) =>
        names.length === 0 || done.has(m.name) ? { ...m, hasUpdate: false } : m,
      );

      // 清空勾选，避免重复点同一个
      selected = new Set();
    } catch (e) {
      log = `更新失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  function toggle(name: string) {
    const next = new Set(selected);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    selected = next;
  }

  /** 把 ISO 时间转成日常格式，避免整片 ISO 字符串干扰阅读。 */
  function fmtDate(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  onMount(load);

  const updatable = $derived(managed.filter((m) => m.hasUpdate === true).length);
</script>

<PageHead title="Skills 更新">
  管理由 <code class="font-[var(--font-mono)] text-[12px]">npx skills</code> 安装的第三方 skill
  <Tip label="「指纹」是什么" class="mt-1">
    这里记录的是来源仓库和内容指纹，不是语义版本号 —— 那套 CLI 本身只按目录树哈希判断有没有变化。
  </Tip>
</PageHead>

<div class="mb-4 flex flex-wrap items-center gap-2">
  <Button onclick={check} disabled={busy || loading}>
    <RefreshCw size={14} />
    {busy ? '处理中…' : '检查更新'}
  </Button>
  <Button
    variant="primary"
    onclick={() => update([...selected])}
    disabled={busy || loading || selected.size === 0}
  >
    更新选中（{selected.size}）
  </Button>
  <Button variant="ghost" onclick={() => update([])} disabled={busy || loading}>更新全部</Button>
  <span class="flex-1"></span>
  {#if updatable > 0}
    <Badge tone="warn">{updatable} 个有更新</Badge>
  {/if}
</div>

{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(5) as _}
      <div class="h-9 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else if managed.length === 0}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    没有受管的第三方 skill。用 <code class="font-[var(--font-mono)]">npx skills add -g &lt;仓库&gt;</code> 安装后会显示在这里。
  </div>
{:else}
  <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
    <table class="w-full border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="w-8 px-4 py-2.5"></th>
          <th class="px-4 py-2.5 font-medium">名称</th>
          <th class="px-4 py-2.5 font-medium">来源仓库</th>
          <th class="px-4 py-2.5 font-medium">指纹</th>
          <th class="px-4 py-2.5 font-medium">安装</th>
          <th class="px-4 py-2.5 font-medium">更新</th>
          <th class="px-4 py-2.5 font-medium">状态</th>
        </tr>
      </thead>
      <tbody>
        {#each managed as m (m.name)}
          <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
            <td class="px-4 py-2.5">
              <input
                type="checkbox"
                class="accent-[var(--color-accent)]"
                checked={selected.has(m.name)}
                onchange={() => toggle(m.name)}
                aria-label={`选择 ${m.name}`}
              />
            </td>
            <td class="whitespace-nowrap px-4 py-2.5 font-[var(--font-mono)] text-[12px]">{m.name}</td>
            <td class="px-4 py-2.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">
              {m.source}
            </td>
            <td class="px-4 py-2.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]">
              {m.hashShort}
            </td>
            <td class="whitespace-nowrap px-4 py-2.5 text-[var(--color-text-muted)]">{fmtDate(m.installedAt)}</td>
            <td class="whitespace-nowrap px-4 py-2.5 text-[var(--color-text-muted)]">{fmtDate(m.updatedAt)}</td>
            <td class="px-4 py-2.5">
              {#if m.hasUpdate === true}
                <Badge tone="warn">可更新</Badge>
              {:else if m.hasUpdate === false}
                <Badge tone="ok">已最新</Badge>
              {:else}
                <Badge>未检查</Badge>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} />
  </div>
{/if}
