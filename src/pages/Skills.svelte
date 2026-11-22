<script lang="ts">
  /**
   * Skills 页：中心库清单 + 标签分类 + 散落副本收集。
   *
   * 标签是外部索引，不写进 skill 目录 —— 那 6 个由 npx skills 管理的 skill
   * 一旦目录内被改动，下次 update 就会冲突。
   */
  import { onMount } from 'svelte';
  import { Search, Plus, X, RefreshCw, FolderInput, Link2, Lock } from '@lucide/svelte';
  import {
    applyOrphanAction,
    collectSkills,
    listSkillMappings,
    listSkills,
    removeSkillMapping,
    scanOrphans,
    setSkillTags,
    syncSkillMappings,
  } from '../lib/ipc';
  import type {
    OrphanAction,
    OrphanActionResult,
    OrphanSkill,
    Skill,
    SkillMappingView,
  } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';
  import { cn } from '../lib/utils';

  let skills = $state<Skill[]>([]);
  let orphans = $state<OrphanSkill[]>([]);
  /** 专用 skill 记录。和 `skills` 里的专用项是同一批东西的两种视角。 */
  let privates = $state<SkillMappingView[]>([]);
  let loading = $state(true);
  let filter = $state('');
  /** 标签筛选。'__untagged__' 是特殊值，表示只看没有标签的。 */
  let activeTag = $state<string | null>(null);
  /** 来源筛选：all 全部 / third 第三方受管 / local 本地自建。 */
  let origin = $state<'all' | 'third' | 'local'>('all');
  /** 范围筛选：all 全部 / public 公共 / private 专用。 */
  let scope = $state<'all' | 'public' | 'private'>('all');
  let log = $state('');
  let editing = $state<string | null>(null);
  let draftTag = $state('');

  // ---- 散落 skill 处置对话框 ----
  /** 对话框是否打开。打开时列表来自最近一次扫描。 */
  let showOrphans = $state(false);
  /** 正在处理的那条（按完整路径区分，同一个 skill 名可能在多处出现）。 */
  let orphanBusy = $state<string | null>(null);
  /** 本次对话框里已经执行过的处置，逐条显示。 */
  let orphanResults = $state<OrphanActionResult[]>([]);
  /** 一键处理（按关系套固定策略）是否在执行。 */
  let collecting = $state(false);

  // ---- 专用 skill 管理 ----
  /** 正在同步链接。 */
  let syncing = $state(false);
  /** 正在取消的那一条。 */
  let removing = $state<string | null>(null);

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

  /** 专用 skill 数量，用于决定「专用」筛选项是否显示。 */
  const privateCount = $derived(skills.filter((s) => s.agents.length > 0).length);

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return skills.filter((s) => {
      // 范围筛选
      if (scope === 'public' && s.agents.length > 0) return false;
      if (scope === 'private' && s.agents.length === 0) return false;
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
      privates = await listSkillMappings();
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
      await collectSkills(apply);
      if (apply) {
        log = '收集完成';
        await load();
        // 处理完就重扫，对话框里的清单立刻反映现状
        if (showOrphans) await rescanOrphans();
      }
    } catch (e) {
      log = `收集失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /**
   * 一键收集：按关系套固定策略批量处理。
   *
   * 只处理「中心库没有」和「内容相同」两类 —— 冲突和坏链接必须人看着决定，
   * 工具替用户选哪一份风险太大。所以这里先确认再执行。
   */
  async function collectAll() {
    const importable = orphans.filter((o) => o.relation === 'unique').length;
    const dedupable = orphans.filter((o) => o.relation === 'identical').length;
    const skipped = orphans.length - importable - dedupable;

    const ok = confirm(
      `按关系批量处理散落 skill？\n\n` +
        `· 移入中心库：${importable} 个（中心库没有的自建 skill）\n` +
        `· 删副本改链接：${dedupable} 个（内容与中心库相同）\n` +
        (skipped > 0 ? `· 不动：${skipped} 个（冲突或坏链接，需要你逐条决定）\n` : '') +
        `\n要逐条决定请点「扫描散落 skill」。`,
    );
    if (!ok) return;

    collecting = true;
    try {
      await runCollect(true);
      if (showOrphans) await rescanOrphans();
    } finally {
      collecting = false;
    }
  }

  /** 重新扫描散落 skill，用于对话框打开状态下刷新清单。 */
  async function rescanOrphans() {
    orphanBusy = 'rescan';
    try {
      orphans = await scanOrphans();
      orphanResults = [];
    } catch (e) {
      log = `扫描失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      orphanBusy = null;
    }
  }

  /** 打开散落清单对话框，先重新扫一遍，避免显示上次的旧结果。 */
  async function openOrphansDialog() {
    showOrphans = true;
    orphanResults = [];
    await rescanOrphans();
  }

  /** 对一条散落 skill 执行用户选定的处置。 */
  async function handleOrphan(o: OrphanSkill, action: OrphanAction) {
    // 每个选项都会动磁盘，先确认。文案里写清后果，避免用户误点
    if (!confirm(confirmText(o, action))) return;

    orphanBusy = o.path;
    try {
      const r = await applyOrphanAction(o.path, action);
      orphanResults = [...orphanResults, r];
      if (r.ok) {
        // 成功后立刻从清单里移除，不用整表重扫
        orphans = orphans.filter((x) => x.path !== o.path);
        // 移入中心库 / 标记专用都会让中心库和专用记录多一条，两张表都要刷新
        skills = await listSkills();
        if (action === 'privatize') privates = await listSkillMappings();
      }
    } catch (e) {
      orphanResults = [
        ...orphanResults,
        {
          name: o.name,
          path: o.path,
          action,
          ok: false,
          message: e instanceof Error ? e.message : String(e),
        },
      ];
    } finally {
      orphanBusy = null;
    }
  }

  /** 关系 → 展示用的徽标文案与颜色。 */
  function relationBadge(rel: OrphanSkill['relation']): {
    text: string;
    tone: 'ok' | 'warn' | 'danger' | 'info' | 'plain';
  } {
    switch (rel) {
      case 'unique':
        return { text: '中心库没有', tone: 'info' };
      case 'identical':
        return { text: '内容相同', tone: 'ok' };
      case 'conflict':
        return { text: '同名不同内容', tone: 'warn' };
      default:
        return { text: '坏链接', tone: 'danger' };
    }
  }

  /** 每种关系可用的处置按钮。delete 在所有关系里都可用。 */
  function actionsFor(o: OrphanSkill): {
    action: OrphanAction;
    label: string;
    hint: string;
    variant: 'primary' | 'outline' | 'ghost' | 'danger';
  }[] {
    switch (o.relation) {
      case 'unique':
        return [
          {
            action: 'import',
            label: '移入中心库',
            hint: '搬进 ~/.agents/skills，原位置建链接，所有 agent 都能读到',
            variant: 'primary',
          },
          // 只有知道属于哪个 agent 才能标记专用 —— 专用库按 agent 分目录
          ...(o.agent
            ? [
                {
                  action: 'privatize' as OrphanAction,
                  label: `标记为 ${o.agent} 专用`,
                  hint: `搬进 ~/.agents/skills-mapping-${o.agent}，不进公共库，只有 ${o.agent} 能读到；原位置建链接`,
                  variant: 'outline' as const,
                },
              ]
            : []),
          {
            action: 'delete',
            label: '删除',
            hint: '直接删掉这份散落副本，不建链接',
            variant: 'danger',
          },
        ];
      case 'identical':
        return [
          {
            action: 'dedupe',
            label: '删副本改链接',
            hint: '中心库那份内容相同，删掉这份，原位置建链接指向中心库',
            variant: 'primary',
          },
          {
            action: 'delete',
            label: '删除',
            hint: '删掉这份，不留链接',
            variant: 'danger',
          },
        ];
      case 'conflict':
        return [
          {
            action: 'take-orphan',
            label: '采用散落版',
            hint: '用这一份覆盖中心库（旧版会先备份），原位置建链接',
            variant: 'primary',
          },
          {
            action: 'take-shared',
            label: '采用中心库版',
            hint: '保留中心库那份，删掉这份，原位置建链接',
            variant: 'outline',
          },
          {
            action: 'delete',
            label: '删除',
            hint: '两份都留着，只删这一份散落副本',
            variant: 'danger',
          },
        ];
      default:
        return [
          {
            action: 'remove-link',
            label: '删除坏链接',
            hint: '只删这个失效的链接文件，不碰任何真实内容',
            variant: 'primary',
          },
        ];
    }
  }

  /** 处置前的确认文案。会动磁盘的操作都要说清后果。 */
  function confirmText(o: OrphanSkill, action: OrphanAction): string {
    const where = `${o.location} 下的 ${o.name}`;
    switch (action) {
      case 'import':
        return `把 ${where} 移入中心库？\n\n原位置会建一个指向中心库的链接，${o.location.split('/')[1] ?? '该 agent'} 仍然能读到这个 skill，所有 agent 都能读到。`;
      case 'privatize':
        return (
          `把 ${where} 标记为 ${o.agent} 专用？\n\n` +
          `· 本体移到 ~/.agents/skills-mapping-${o.agent}/${o.name}\n` +
          `· 不进公共库，别的 agent 看不到它\n` +
          `· 原位置建链接，${o.agent} 照常能读\n` +
          `· 会记一条归属记录，以后可以「修复链接」或取消`
        );
      case 'dedupe':
        return `删掉 ${where} 这份副本，改建成指向中心库的链接？\n\n内容与中心库完全相同，不会丢东西。`;
      case 'take-orphan':
        return `用 ${where} 覆盖中心库的同名 skill？\n\n中心库那一份会先备份到 ~/.agents/backup/，原位置建链接。`;
      case 'take-shared':
        return `保留中心库那份，删掉 ${where} 并改建链接？\n\n这一份的本地改动会丢失。`;
      case 'delete':
        return `确定删除 ${where}？\n\n这是真删除，不进备份目录。`;
      default:
        return `删除 ${where} 这个坏链接？\n\n只删链接文件本身，不碰任何真实内容。`;
    }
  }

  /** 重建全部专用 skill 的链接。幂等：正常的不动。 */
  async function runSyncPrivates() {
    syncing = true;
    try {
      const lines = await syncSkillMappings(false);
      log = lines.join('\n');
      await load();
    } catch (e) {
      log = `修复链接失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      syncing = false;
    }
  }

  /** 取消一条专用记录。toShared 决定本体是回公共库还是备份后删掉。 */
  async function cancelPrivate(m: SkillMappingView, toShared: boolean) {
    const where = `${m.skill}（${m.agents.join('、')} 专用）`;
    const ok = confirm(
      toShared
        ? `取消 ${where} 的专用？\n\n· 各 agent 目录里的链接会删掉\n· 本体移进公共库 ~/.agents/skills，之后所有 agent 都能读到`
        : `取消 ${where} 的专用？\n\n· 各 agent 目录里的链接会删掉\n· 本体先复制到 ~/.agents/backup/，然后删除\n· 这个删除不进 git，删了要从备份里找回来`,
    );
    if (!ok) return;

    removing = m.skill;
    try {
      const r = await removeSkillMapping(m.skill, toShared);
      log = r.output;
      await load();
    } catch (e) {
      log = `取消失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      removing = null;
    }
  }

  function kb(n: number): string {
    return n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`;
  }

  onMount(load);
</script>

<PageHead title="Skills" />

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
    <button
      class="text-xs text-[var(--color-warn)] underline-offset-2 hover:underline"
      onclick={openOrphansDialog}
      disabled={loading}
    >
      {orphans.length} 个散落副本，点此处置
    </button>
  {/if}
  <Button onclick={openOrphansDialog} disabled={loading}>
    <FolderInput size={14} />
    扫描散落 skill
  </Button>
  <Button variant="primary" onclick={collectAll} disabled={loading || collecting}>
    {collecting ? '处理中…' : '一键收集到中心库'}
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

<!-- 范围筛选：公共（所有 agent 都能读）/ 专用（只有指定的那几只能读） -->
<div class="mb-2 flex flex-wrap items-center gap-1.5">
  <span class="mr-1 text-[11px] text-[var(--color-text-faint)]">范围</span>
  {#each [['all', '全部'], ['public', '公共'], ['private', `专用 ${privateCount}`]] as const as [val, label] (val)}
    <button
      class={cn(
        'rounded-full px-2.5 py-0.5 text-[11px] transition-colors',
        scope === val
          ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
          : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
      )}
      onclick={() => (scope = val)}
    >
      {label}
    </button>
  {/each}
</div>

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
          <th class="w-[17%] px-4 py-2.5 font-medium">名称</th>
          <th class="w-[9%] px-4 py-2.5 font-medium">范围</th>
          <th class="w-[27%] px-4 py-2.5 font-medium">说明</th>
          <th class="w-[20%] px-4 py-2.5 font-medium">标签</th>
          <th class="w-[13%] px-4 py-2.5 font-medium">来源</th>
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
            <td class="px-4 py-2.5">
              {#if s.agents.length > 0}
                <span
                  class="inline-flex items-center gap-1 rounded-[var(--radius-sm)] bg-[var(--color-warn-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-warn)]"
                  title={s.storePath ? `本体在 ${s.storePath}` : '专用 skill'}
                >
                  <Lock size={10} />
                  {s.agents.join(',')}
                </span>
                {#if s.linkState !== 'ok'}
                  <Badge tone="danger" class="ml-1">
                    {s.linkState === 'broken' ? '断链' : '缺失'}
                  </Badge>
                {/if}
              {:else}
                <span class="text-[11px] text-[var(--color-text-faint)]">公共</span>
              {/if}
            </td>
            <td class="px-4 py-2.5 text-[var(--color-text-muted)]">
              <!-- 描述最长有 900 多字符，必须截断。两行截断 + 悬停看全文。
                   长英文单词不含空格时也要能断行，否则会把列撑宽。 -->
              <div
                class="line-clamp-2 max-w-[32ch] break-words text-[12px] leading-snug"
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

<!-- 专用 skill 管理：本体在 ~/.agents/skills-mapping-<agent>，agent 目录里靠链接读回来。
     链接一断 agent 就悄悄读不到，所以这块的重点是状态可见 + 一键重建。 -->
{#if privates.length > 0}
  <div class="mt-6">
    <div class="mb-2 flex flex-wrap items-center gap-2">
      <h2 class="text-[13px] font-semibold text-[var(--color-text)]">专用 skill</h2>
      <span class="text-[11px] text-[var(--color-text-faint)]">
        只给指定 agent 用，不进公共库；agent 目录里是链接
      </span>
      <span class="flex-1"></span>
      <Button size="sm" variant="ghost" onclick={runSyncPrivates} disabled={syncing}>
        <Link2 size={13} />
        {syncing ? '修复中…' : '修复链接'}
      </Button>
    </div>

    <div class="flex flex-col gap-2">
      {#each privates as m (m.skill)}
        {@const stateBadge =
          m.state === 'ok'
            ? { text: '正常', tone: 'ok' as const }
            : m.state === 'broken'
              ? { text: '断链', tone: 'danger' as const }
              : { text: '缺失', tone: 'warn' as const }}
        <div class="rounded-[var(--radius-md)] border border-[var(--color-border)] p-3">
          <div class="flex flex-wrap items-center gap-2">
            <span class="font-[var(--font-mono)] text-[13px] text-[var(--color-text)]">{m.skill}</span>
            <Badge tone="plain">{m.agents.join(', ')}</Badge>
            <Badge tone={stateBadge.tone}>{stateBadge.text}</Badge>
            {#if !m.storeExists}
              <span class="text-[11px] text-[var(--color-danger)]">本体目录不存在</span>
            {/if}
            <span class="flex-1"></span>
            <Button
              size="sm"
              variant="outline"
              disabled={removing !== null}
              onclick={() => cancelPrivate(m, true)}
              title="删掉各 agent 的链接，本体移进公共库，之后所有 agent 都能读"
            >
              {removing === m.skill ? '处理中…' : '移入公共库'}
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={removing !== null}
              onclick={() => cancelPrivate(m, false)}
              title="删掉各 agent 的链接，本体先备份到 ~/.agents/backup/ 再删除"
            >
              取消并删除
            </Button>
          </div>

          <div
            class="mt-1 flex flex-col gap-0.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]"
          >
            <span title={m.storePath}>本体：{m.storePath}</span>
            {#each m.links as l (l.agent)}
              <span
                class={cn(
                  l.state === 'ok' ? 'text-[var(--color-text-faint)]' : 'text-[var(--color-danger)]',
                )}
                title={l.note ?? ''}
              >
                {l.label}：{l.path}
                {#if l.state !== 'ok'}
                  <span class="ml-1">— {l.note ?? l.state}</span>
                {:else if l.actual}
                  <span class="ml-1 opacity-60">({l.actual})</span>
                {/if}
              </span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}

{#if showOrphans}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="orphans-title"
    onclick={(e) => {
      if (e.target === e.currentTarget && !orphanBusy) showOrphans = false;
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape' && !orphanBusy) showOrphans = false;
    }}
  >
    <div
      class="flex max-h-[min(880px,calc(100vh-2rem))] w-full max-w-4xl flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] shadow-lg"
    >
      <div class="border-b border-[var(--color-border)] px-5 py-4">
        <div class="flex items-start gap-3">
          <div class="min-w-0 flex-1">
            <h2 id="orphans-title" class="text-[15px] font-semibold text-[var(--color-text)]">
              散落 skill 处置
            </h2>
            <p class="mt-1 text-[12px] text-[var(--color-text-muted)]">
              下面这些 skill 躺在各个 agent 自己的目录里。
              通用的收进中心库 <code class="font-[var(--font-mono)]">~/.agents/skills</code>，
              只给某一个 agent 用的标成「专用」，它会搬进
              <code class="font-[var(--font-mono)]">~/.agents/skills-mapping-&lt;agent&gt;</code> 而不进公共库。
              逐条选怎么处理；每个动作都会先问一次再动磁盘。
            </p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            onclick={rescanOrphans}
            disabled={orphanBusy !== null}
          >
            <RefreshCw size={13} />
            {orphanBusy === 'rescan' ? '扫描中…' : '重新扫描'}
          </Button>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {#if orphanBusy === 'rescan'}
          <div class="flex flex-col gap-2">
            {#each Array(4) as _}
              <div class="h-14 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
            {/each}
          </div>
        {:else if orphans.length === 0}
          <div class="py-12 text-center text-[13px] text-[var(--color-text-faint)]">
            没有散落的 skill。<br />
            <span class="text-[12px]">
              各 agent 的 skills 目录里只有链接或空目录时，这里就是空的。
            </span>
          </div>
        {:else}
          <div class="flex flex-col gap-2">
            {#each orphans as o (o.path)}
              {@const rel = relationBadge(o.relation)}
              <div class="rounded-[var(--radius-md)] border border-[var(--color-border)] p-3">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="font-[var(--font-mono)] text-[13px] text-[var(--color-text)]">
                    {o.name}
                  </span>
                  <Badge tone={rel.tone}>{rel.text}</Badge>
                  {#if o.fileCount > 0}
                    <span class="text-[11px] text-[var(--color-text-faint)]">
                      {o.fileCount} 个文件 · {kb(o.sizeBytes)}
                    </span>
                  {/if}
                  <span class="flex-1"></span>
                  <span
                    class="font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
                    title={o.path}
                  >
                    {o.location}
                  </span>
                </div>

                {#if o.description}
                  <div class="mt-1.5 line-clamp-2 break-words text-[12px] leading-snug text-[var(--color-text-muted)]" title={o.description}>
                    {o.description}
                  </div>
                {/if}

                <div class="mt-2 flex flex-wrap items-center gap-1.5">
                  {#each actionsFor(o) as a (a.action)}
                    <Button
                      size="sm"
                      variant={a.variant}
                      title={a.hint}
                      disabled={orphanBusy !== null}
                      onclick={() => handleOrphan(o, a.action)}
                    >
                      {orphanBusy === o.path ? '处理中…' : a.label}
                    </Button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {/if}

        {#if orphanResults.length > 0}
          <div class="mt-4 rounded-[var(--radius-md)] border border-[var(--color-border)] p-3">
            <h3 class="mb-2 text-[12px] font-medium text-[var(--color-text)]">本次处置记录</h3>
            <div class="flex flex-col gap-1">
              {#each orphanResults as r, i (i)}
                <div class="flex items-start gap-2 text-[12px] leading-snug">
                  <Badge tone={r.ok ? 'ok' : 'danger'}>{r.ok ? '成功' : '失败'}</Badge>
                  <span class="font-[var(--font-mono)] text-[11px]">{r.name}</span>
                  <span class="min-w-0 flex-1 break-words text-[var(--color-text-muted)]">
                    {r.message}
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <div class="flex items-center gap-2 border-t border-[var(--color-border)] px-5 py-3">
        <span class="text-[11px] text-[var(--color-text-faint)]">
          「一键收集」只处理中心库没有的和内容相同的；冲突要在这里逐条选。
        </span>
        <span class="flex-1"></span>
        <Button variant="ghost" onclick={() => (showOrphans = false)} disabled={orphanBusy !== null}>
          关闭
        </Button>
        <Button
          variant="primary"
          onclick={collectAll}
          disabled={orphanBusy !== null || collecting || orphans.length === 0}
        >
          {collecting ? '处理中…' : '一键收集可自动处理的'}
        </Button>
      </div>
    </div>
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} />
  </div>
{/if}
