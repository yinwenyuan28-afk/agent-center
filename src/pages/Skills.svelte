<script lang="ts">
  /**
   * Skills 页：中心库清单 + 标签分类 + 散落副本收集。
   *
   * 标签是外部索引，不写进 skill 目录 —— 那 6 个由 npx skills 管理的 skill
   * 一旦目录内被改动，下次 update 就会冲突。
   */
  import { onMount } from 'svelte';
  import { Search, Plus, X } from '@lucide/svelte';
  import { collectSkills, listSkills, scanOrphans, setSkillTags } from '../lib/ipc';
  import type { CollectPlan, OrphanSkill, Skill } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';
  import { cn } from '../lib/utils';

  let skills = $state<Skill[]>([]);
  let orphans = $state<OrphanSkill[]>([]);
  let plan = $state<CollectPlan | null>(null);
  let loading = $state(true);
  let filter = $state('');
  /** 标签筛选。'__untagged__' 是特殊值，表示只看没有标签的。 */
  let activeTag = $state<string | null>(null);
  /** 来源筛选：all 全部 / third 第三方受管 / local 本地自建。 */
  let origin = $state<'all' | 'third' | 'local'>('all');
  let log = $state('');
  let editing = $state<string | null>(null);
  let draftTag = $state('');

  /** 未分类的哨兵值。用不可能出现在真实标签里的字符串，避免与用户标签撞名。 */
  const UNTAGGED = '__untagged__';

  /** 所有出现过的标签，按出现次数降序，常用的排前面。 */
  const allTags = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const s of skills) {
      for (const t of s.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([t]) => t);
  });

  /** 没有任何标签的 skill 数量，用于决定「未分类」筛选项是否显示。 */
  const untaggedCount = $derived(skills.filter((s) => s.tags.length === 0).length);

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return skills.filter((s) => {
      // 来源筛选
      if (origin === 'third' && !s.managed) return false;
      if (origin === 'local' && s.managed) return false;
      // 标签筛选
      if (activeTag === UNTAGGED) {
        if (s.tags.length > 0) return false;
      } else if (activeTag && !s.tags.includes(activeTag)) {
        return false;
      }
      // 关键词
      if (!q) return true;
      return s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q);
    });
  });

  async function load() {
    loading = true;
    try {
      skills = await listSkills();
      orphans = await scanOrphans();
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  async function addTag(name: string) {
    const t = draftTag.trim();
    if (!t) {
      editing = null;
      return;
    }
    const skill = skills.find((s) => s.name === name);
    if (!skill || skill.tags.includes(t)) {
      draftTag = '';
      editing = null;
      return;
    }
    const next = [...skill.tags, t];
    try {
      await setSkillTags(name, next);
      skills = skills.map((s) => (s.name === name ? { ...s, tags: next } : s));
    } catch (e) {
      log = `改标签失败：${e instanceof Error ? e.message : String(e)}`;
    }
    draftTag = '';
    editing = null;
  }

  async function removeTag(name: string, tag: string) {
    const skill = skills.find((s) => s.name === name);
    if (!skill) return;
    const next = skill.tags.filter((t) => t !== tag);
    try {
      await setSkillTags(name, next);
      skills = skills.map((s) => (s.name === name ? { ...s, tags: next } : s));
      // 如果当前筛选的标签已经没人用了，退回「全部」
      if (activeTag === tag && !skills.some((s) => s.tags.includes(tag))) activeTag = null;
    } catch (e) {
      log = `改标签失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function runCollect(apply: boolean) {
    try {
      plan = await collectSkills(apply);
      if (apply) {
        log = '收集完成';
        await load();
      }
    } catch (e) {
      log = `收集失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  function kb(n: number): string {
    return n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`;
  }

  onMount(load);
</script>

<PageHead title="Skills">
  中心库 <code class="font-[var(--font-mono)] text-[12px]">~/.agents/skills</code> 里的全部 skill。标签只记录在配置里，不改动 skill 目录本身。
</PageHead>

<div class="mb-3 flex flex-wrap items-center gap-2">
  <div class="relative">
    <Search
      size={14}
      class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--color-text-faint)]"
    />
    <input
      class="h-9 w-[240px] rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] pl-8 pr-3 text-[13px] text-[var(--color-text)] placeholder:text-[var(--color-text-faint)] focus:border-transparent focus:outline-2 focus:outline-[var(--color-accent)]"
      placeholder="搜索名称或描述"
      bind:value={filter}
    />
  </div>
  <span class="flex-1"></span>
  {#if orphans.length > 0}
    <span class="text-xs text-[var(--color-text-muted)]">{orphans.length} 个散落副本</span>
  {/if}
  <Button onclick={() => runCollect(false)} disabled={loading}>扫描</Button>
  <Button variant="primary" onclick={() => runCollect(true)} disabled={loading}>
    收集到中心库
  </Button>
</div>

{#if allTags.length > 0 || untaggedCount > 0}
  <div class="mb-2 flex flex-wrap gap-1.5">
    <button
      class={cn(
        'rounded-full px-2.5 py-0.5 text-[11px] transition-colors',
        activeTag === null
          ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
          : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
      )}
      onclick={() => (activeTag = null)}
    >
      全部
    </button>
    {#each allTags as t (t)}
      <button
        class={cn(
          'rounded-full px-2.5 py-0.5 text-[11px] transition-colors',
          activeTag === t
            ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
            : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
        )}
        onclick={() => (activeTag = t)}
      >
        {t}
      </button>
    {/each}
    {#if untaggedCount > 0}
      <button
        class={cn(
          'rounded-full px-2.5 py-0.5 text-[11px] transition-colors',
          activeTag === UNTAGGED
            ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
            : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
        )}
        onclick={() => (activeTag = UNTAGGED)}
      >
        未分类 {untaggedCount}
      </button>
    {/if}
  </div>
{/if}

<!-- 来源筛选：区分 npx skills 装的第三方和自己写的 -->
<div class="mb-4 flex flex-wrap items-center gap-1.5">
  <span class="mr-1 text-[11px] text-[var(--color-text-faint)]">来源</span>
  {#each [['all', '全部'], ['third', '第三方'], ['local', '本地自建']] as const as [val, label] (val)}
    <button
      class={cn(
        'rounded-full px-2.5 py-0.5 text-[11px] transition-colors',
        origin === val
          ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
          : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
      )}
      onclick={() => (origin = val)}
    >
      {label}
    </button>
  {/each}
  <span class="flex-1"></span>
  <span class="text-[11px] text-[var(--color-text-faint)]">显示 {shown.length} / {skills.length}</span>
</div>

{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(8) as _}
      <div class="h-9 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else}
  <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
    <!-- table-fixed + 列宽：不加这个的话，长描述会把整张表撑宽，
         再配合 line-clamp 才能真正把行高压住 -->
    <table class="w-full table-fixed border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="w-[18%] px-4 py-2.5 font-medium">名称</th>
          <th class="w-[30%] px-4 py-2.5 font-medium">说明</th>
          <th class="w-[24%] px-4 py-2.5 font-medium">标签</th>
          <th class="w-[14%] px-4 py-2.5 font-medium">来源</th>
          <th class="w-[6%] px-4 py-2.5 text-right font-medium">文件</th>
          <!-- 大小列要给足宽度。"62.2 KB" 在 5% 宽度下会被折成两行，
               而 Windows 上常见大 skill（minimax-docx 1.5MB）需要更多空间 -->
          <th class="w-[8%] px-4 py-2.5 text-right font-medium whitespace-nowrap">大小</th>
        </tr>
      </thead>
      <tbody>
        {#each shown as s (s.name)}
          <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
            <td class="whitespace-nowrap px-4 py-2.5">
              <span class="font-[var(--font-mono)] text-[12px]">{s.name}</span>
              {#if s.managed}
                <Badge tone="info" class="ml-1.5">受管</Badge>
              {/if}
            </td>
            <td class="px-4 py-2.5 text-[var(--color-text-muted)]">
              <!-- 描述最长有 900 多字符，必须截断。两行截断 + 悬停看全文。
                   长英文单词不含空格时也要能断行，否则会把列撑宽。 -->
              <div
                class="line-clamp-2 max-w-[36ch] break-words text-[12px] leading-snug"
                title={s.description}
              >
                {s.description || '—'}
              </div>
            </td>
            <td class="min-w-[150px] px-4 py-2.5">
              <div class="flex flex-wrap items-center gap-1">
                {#each s.tags as t (t)}
                  <span
                    class="inline-flex items-center gap-0.5 rounded-[var(--radius-sm)] bg-[var(--color-info-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-info)]"
                  >
                    {t}
                    <button
                      class="opacity-50 hover:opacity-100"
                      title="移除标签"
                      aria-label={`从 ${s.name} 移除标签 ${t}`}
                      onclick={() => removeTag(s.name, t)}
                    >
                      <X size={10} />
                    </button>
                  </span>
                {/each}
                {#if editing === s.name}
                  <input
                    class="w-[54px] border-none bg-transparent text-[11px] text-[var(--color-text)] outline-none placeholder:text-[var(--color-text-faint)]"
                    bind:value={draftTag}
                    placeholder="标签"
                    onkeydown={(e) => {
                      if (e.key === 'Enter') addTag(s.name);
                      if (e.key === 'Escape') {
                        editing = null;
                        draftTag = '';
                      }
                    }}
                    onblur={() => addTag(s.name)}
                  />
                {:else}
                  <button
                    class="rounded-[var(--radius-sm)] px-1 text-[var(--color-text-faint)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
                    title="添加标签"
                    aria-label={`为 ${s.name} 添加标签`}
                    onclick={() => {
                      editing = s.name;
                      draftTag = '';
                    }}
                  >
                    <Plus size={11} />
                  </button>
                {/if}
              </div>
            </td>
            <td class="max-w-[22ch] px-4 py-2.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">
              {s.source ?? '—'}
            </td>
            <td class="px-4 py-2.5 text-right tabular-nums whitespace-nowrap">{s.fileCount}</td>
            <td class="px-4 py-2.5 text-right tabular-nums whitespace-nowrap">{kb(s.sizeBytes)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if shown.length === 0}
    <div class="py-12 text-center text-[13px] text-[var(--color-text-faint)]">没有匹配的 skill。</div>
  {/if}
{/if}

{#if plan}
  <div class="mt-6 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
    <h2 class="mb-1 text-[14px] font-semibold">收集计划</h2>
    <p class="mb-3 text-[12px] text-[var(--color-text-muted)]">
      移入 = 收进中心库并在原位置建链接；去重 = 内容相同只删副本；冲突 = 内容不同，需要你决定。
    </p>
    <table class="w-full border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="px-4 py-2 font-medium">处理</th>
          <th class="px-4 py-2 font-medium">名称</th>
          <th class="px-4 py-2 font-medium">当前位置</th>
        </tr>
      </thead>
      <tbody>
        {#each plan.toImport as o (o.path)}
          <tr class="border-b border-[var(--color-border)] last:border-0">
            <td class="px-4 py-2"><Badge tone="ok">移入</Badge></td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[12px]">{o.name}</td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">{o.location}</td>
          </tr>
        {/each}
        {#each plan.toDedupe as o (o.path)}
          <tr class="border-b border-[var(--color-border)] last:border-0">
            <td class="px-4 py-2"><Badge tone="info">去重</Badge></td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[12px]">{o.name}</td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">{o.location}</td>
          </tr>
        {/each}
        {#each plan.conflicts as o (o.path)}
          <tr class="border-b border-[var(--color-border)] last:border-0">
            <td class="px-4 py-2"><Badge tone="warn">冲突</Badge></td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[12px]">{o.name}</td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">{o.location}</td>
          </tr>
        {/each}
        {#each plan.broken as o (o.path)}
          <tr class="border-b border-[var(--color-border)] last:border-0">
            <td class="px-4 py-2"><Badge tone="danger">坏链接</Badge></td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[12px]">{o.name}</td>
            <td class="px-4 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">{o.location}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if plan.toImport.length === 0 && plan.toDedupe.length === 0 && plan.conflicts.length === 0 && plan.broken.length === 0}
      <div class="py-8 text-center text-[13px] text-[var(--color-text-faint)]">没有需要收集的 skill。</div>
    {/if}
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} />
  </div>
{/if}
