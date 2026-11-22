<script lang="ts">
  /**
   * 指令映射页。
   *
   * 这是工具最核心的一屏：列出每个 agent 的源头、落脚点、实际用的链接方式、
   * 以及当前是否健康。健康状态靠真后端检测，浏览器预览时走假数据。
   */
  import { onMount } from 'svelte';
  import { RefreshCw, Plus } from '@lucide/svelte';
  import {
    composeAgentDocs,
    createDoc as createDocApi,
    listAgentDocs,
    listMappings,
    readAgentDoc,
    readDoc,
    remap,
    repairMapping,
    saveAgentDoc,
    saveDoc as saveDocApi,
    syncMappings,
    unmap,
  } from '../lib/ipc';
  import type { AgentDoc, Mapping } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';
  import { cn } from '../lib/utils';

  let mappings = $state<Mapping[]>([]);
  let loading = $state(true);
  let busy = $state<string | null>(null);
  let log = $state('');

  // 母版内容区状态
  /** idle 表示还没读；ok 有内容；missing 文件不存在；error 读取失败 */
  let docState = $state<'loading' | 'ok' | 'missing' | 'error'>('loading');
  let docContent = $state('');
  let docError = $state('');
  let docBusy = $state(false);

  // ---- 片段选项卡 ----
  /** 每个 agent 的片段与合成状态。 */
  let agentDocs = $state<AgentDoc[]>([]);
  /** 当前编辑的是哪个片段。null = 通用母版。 */
  let activeDoc = $state<string | null>(null);
  /** 当前选中片段的已保存内容。 */
  let fragContent = $state('');
  /** 编辑中的草稿。通用和专用共用一套状态，切换时重置。 */
  let fragDraft = $state('');
  let fragEditing = $state(false);
  let fragBusy = $state(false);

  /** 当前片段是否有未保存改动。 */
  const fragDirty = $derived(fragEditing && fragDraft !== fragContent);

  /** 有专用内容的 agent 数量，用于选项卡上的计数。 */
  const withSpecific = $derived(agentDocs.filter((d) => d.specificExists).length);

  /** 合成文件落后于片段内容的 agent 数量。 */
  const staleCount = $derived(agentDocs.filter((d) => d.composedStale).length);

  /** 当前选中的 agent 记录。null 表示在看通用母版。 */
  const activeAgent = $derived(
    activeDoc === null ? null : (agentDocs.find((d) => d.agent === activeDoc) ?? null),
  );

  /** 把健康状态翻成界面用词，只在需要行动时用强色。 */
  function healthLabel(m: Mapping): { text: string; tone: 'ok' | 'warn' | 'danger' | 'plain' } {
    // 已取消优先：用户主动关掉的东西不该显示成「坏了」
    if (m.disabled) return { text: '已取消', tone: 'plain' };
    if (!m.installed) return { text: '未安装', tone: 'plain' };
    switch (m.health) {
      case 'ok':
        return { text: '正常', tone: 'ok' };
      case 'drifted':
        return { text: '内容已漂移', tone: 'warn' };
      case 'broken':
        return { text: '链接断开', tone: 'danger' };
      default:
        return { text: '待同步', tone: 'warn' };
    }
  }

  /** 链接方式的中文说明。用户不需要知道技术细节，但需要知道有没有权限要求。 */
  function kindLabel(kind: Mapping['actual']): string {
    switch (kind) {
      case 'symlink':
        return '符号链接';
      case 'junction':
        return '目录连接';
      case 'hardlink':
        return '硬链接';
      case 'copy':
        return '副本';
      default:
        return '—';
    }
  }

  async function load() {
    loading = true;
    try {
      mappings = await listMappings();
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  async function runSync(dryRun: boolean) {
    busy = dryRun ? 'preview' : 'sync';
    try {
      const r = await syncMappings(dryRun);
      log = r.output;
      if (!dryRun) await load();
    } catch (e) {
      log = `失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  async function repair(agent: string) {
    busy = agent;
    try {
      const r = await repairMapping(agent);
      log = r.output;
      await load();
    } catch (e) {
      log = `失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  /** 取消映射：删掉目标位置的链接，并记下不再同步。 */
  async function cancelMapping(m: Mapping) {
    const ok = confirm(
      `取消 ${m.label} 的映射？\n\n` +
        `会删掉 ${m.target} 处的链接。\n` +
        `母版文件不受影响；如果那个文件内容与母版不同，会先移进 ~/.agents/backup/。\n\n` +
        `取消后「同步全部」会跳过它，随时可以点「恢复映射」重建。`,
    );
    if (!ok) return;

    busy = m.agent;
    try {
      const r = await unmap(m.agent);
      log = r.output;
      await load();
    } catch (e) {
      log = `取消失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  /** 恢复映射：清掉「已取消」标记并立刻重建链接。 */
  async function restoreMapping(m: Mapping) {
    busy = m.agent;
    try {
      const r = await remap(m.agent);
      log = r.output;
      await load();
    } catch (e) {
      log = `恢复失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  /** 读取母版文件内容。文件不存在不算错误，单独用 missing 状态表达。 */
  async function loadDoc() {
    docState = 'loading';
    try {
      const r = await readDoc();
      if (!r.exists) {
        docState = 'missing';
        docContent = '';
        return;
      }
      docContent = r.content;
      docState = 'ok';
    } catch (e) {
      docError = e instanceof Error ? e.message : String(e);
      docState = 'error';
    }
  }

  /** 用当前已有内容或一个空白模板创建母版文件。 */
  async function createDoc() {
    docBusy = true;
    try {
      const template = '# 全局工作方法\n\n在这里写所有 agent 共用的规则。\n';
      const r = await createDocApi(template);
      log = r.output;
      if (r.ok) {
        await loadDoc();
        await load();
      }
    } catch (e) {
      log = `创建失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      docBusy = false;
    }
  }

  // ---- 片段选项卡 ----

  /** 读所有 agent 的片段与合成状态。 */
  async function loadAgentDocs() {
    try {
      agentDocs = await listAgentDocs();
    } catch (e) {
      log = `读取片段状态失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** 切到某个片段（null = 通用母版）。有未保存改动时先确认。 */
  async function switchDoc(agent: string | null) {
    if (fragEditing && fragDirty && !confirm('放弃未保存的改动？')) return;

    activeDoc = agent;
    fragEditing = false;
    fragDraft = '';

    if (agent === null) {
      await loadDoc();
      return;
    }

    fragContent = '';
    try {
      const r = await readAgentDoc(agent);
      fragContent = r.exists ? r.content : '';
    } catch (e) {
      log = `读取片段失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** 进入编辑。专用片段不存在时也能进入 —— 这就是"新建"的入口。 */
  function startFragEdit() {
    fragDraft = activeDoc === null ? docContent : fragContent;
    fragEditing = true;
  }

  function cancelFragEdit() {
    if (fragDirty && !confirm('放弃未保存的改动？')) return;
    fragEditing = false;
    fragDraft = '';
  }

  /**
   * 保存当前片段。
   *
   * 通用母版和专用片段的保存走不同命令，但成功后的动作一样：
   * 重新拉片段状态（合成会变成"待更新"），再刷新映射健康度。
   */
  async function saveFrag() {
    fragBusy = true;
    try {
      if (activeDoc === null) {
        const r = await saveDocApi(fragDraft);
        log = r.output;
        if (!r.ok) return;
        docContent = fragDraft;
      } else {
        const r = await saveAgentDoc(activeDoc, fragDraft);
        log = r.output;
        if (!r.ok) return;
        fragContent = fragDraft;
      }
      fragEditing = false;
      fragDraft = '';
      await loadAgentDocs();
      await load();
    } catch (e) {
      log = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      fragBusy = false;
    }
  }

  /**
   * 取消某个 agent 的专用片段。
   *
   * 传空内容给后端即可：它会删片段、清合成文件，链接下次同步时收回通用母版。
   */
  async function clearSpecific(agent: string) {
    if (
      !confirm(
        `取消 ${agent} 的专用片段？\n\n片段文件会被删（旧版先备份到 ~/.agents/backup/），` +
          `${agent} 之后只读通用母版。下次同步时会自动改回链接通用母版。`,
      )
    ) {
      return;
    }
    fragBusy = true;
    try {
      const r = await saveAgentDoc(agent, '');
      log = r.output;
      if (r.ok) {
        fragContent = '';
        fragEditing = false;
        fragDraft = '';
        await loadAgentDocs();
        await load();
      }
    } catch (e) {
      log = `取消失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      fragBusy = false;
    }
  }

  /** 生成所有落后的合成文件，然后同步一次链接。 */
  async function composeAndSync() {
    busy = 'compose';
    try {
      const r = await composeAgentDocs(false);
      log = r.output;
      await loadAgentDocs();
      await runSync(false);
    } catch (e) {
      log = `生成失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  onMount(async () => {
    await load();
    await loadDoc();
    await loadAgentDocs();
  });

  const online = $derived(mappings.filter((m) => m.installed).length);
  const healthy = $derived(mappings.filter((m) => m.health === 'ok').length);
</script>

<PageHead title="AGENTS文档映射">
  把这些 agent 各自读取的 AGENTS 文档，统一指向同一份母版。
</PageHead>

<div class="mb-4 flex flex-wrap items-center gap-2">
  <Button variant="primary" onclick={() => runSync(false)} disabled={busy !== null}>
    {busy === 'sync' ? '同步中…' : '同步全部'}
  </Button>
  <Button onclick={() => runSync(true)} disabled={busy !== null}>预览变更</Button>
  <Button variant="ghost" onclick={load} disabled={busy !== null}>
    <RefreshCw size={14} />
    刷新
  </Button>
  <span class="flex-1"></span>
  {#if !loading}
    <span class="text-xs text-[var(--color-text-muted)]">
      {healthy} / {online} 个已同步
    </span>
  {/if}
</div>

<!-- 文档片段区：通用母版 + 每个 agent 的专用补充 -->
<div class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-border)]">
  <div class="flex items-center gap-2 border-b border-[var(--color-border)] px-4 py-2.5">
    <span class="text-[13px] font-medium">指令文档</span>
    <span class="font-[var(--color-text-faint)] text-[11px]">
      {activeDoc === null
        ? (agentDocs[0]?.commonPath ?? '~/.agents/AGENTS.md')
        : (activeAgent?.specificPath ?? '')}
    </span>
    <span class="flex-1"></span>
    {#if fragEditing}
      <Button size="sm" variant="primary" onclick={saveFrag} disabled={fragBusy}>
        {fragBusy ? '保存中…' : '保存'}
      </Button>
      <Button size="sm" variant="ghost" onclick={cancelFragEdit} disabled={fragBusy}>取消</Button>
    {:else}
      <Button size="sm" onclick={startFragEdit} disabled={activeDoc === null && docState !== 'ok'}>
        编辑
      </Button>
      <Button size="sm" variant="ghost" onclick={() => switchDoc(activeDoc)}>
        <RefreshCw size={13} />
      </Button>
    {/if}
  </div>

  <!-- 片段选项卡。通用永远在最左；agent 只在有专用片段时标 ✎ -->
  <div class="flex flex-wrap gap-1 border-b border-[var(--color-border)] px-4 py-2">
    <button
      class={cn(
        'rounded-[var(--radius-sm)] px-2.5 py-1 text-[12px] transition-colors',
        activeDoc === null
          ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
          : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
      )}
      onclick={() => switchDoc(null)}
    >
      通用
    </button>
    {#each agentDocs as d (d.agent)}
      <button
        class={cn(
          'inline-flex items-center gap-1 rounded-[var(--radius-sm)] px-2.5 py-1 text-[12px] transition-colors',
          activeDoc === d.agent
            ? 'bg-[var(--color-text)] text-[var(--color-bg)]'
            : d.specificExists
              ? 'bg-[var(--color-info-bg)] text-[var(--color-info)] hover:brightness-95'
              : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]',
        )}
        title={d.specificExists
          ? `${d.label} 读「通用 + 专用」，合成后 ${d.bytes} 字节`
          : `${d.label} 只读通用母版，点此添加专用补充`}
        onclick={() => switchDoc(d.agent)}
      >
        {d.label}
        {#if d.specificExists}
          <span aria-label="有专用内容">✎</span>
        {/if}
        {#if d.composedStale}
          <span class="text-[var(--color-warn)]" title="合成文件待更新">•</span>
        {/if}
      </button>
    {/each}
    <span class="ml-1 self-center text-[11px] text-[var(--color-text-faint)]">
      {withSpecific} / {agentDocs.length} 个有专用内容
    </span>
  </div>

  <div class="p-4">
    <!-- 顶部提示：有合成文件落后时统一给一个动作 -->
    {#if staleCount > 0}
      <div
        class="mb-3 flex flex-wrap items-center gap-2 rounded-[var(--radius-md)] border border-[var(--color-warn)] bg-[var(--color-warn-bg)] px-3 py-2"
      >
        <span class="text-[12px] text-[var(--color-warn)]">
          {staleCount} 个 agent 的合成文档还没更新，它们现在读到的还是旧内容。
        </span>
        <span class="flex-1"></span>
        <Button size="sm" variant="primary" onclick={composeAndSync} disabled={busy !== null || fragBusy}>
          {busy === 'compose' ? '生成中…' : '生成并同步'}
        </Button>
      </div>
    {/if}

    {#if fragEditing}
      <textarea
        class="h-[320px] w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-3 py-2 font-[var(--font-mono)] text-[12px] leading-relaxed text-[var(--color-text)] focus:border-transparent focus:outline-2 focus:outline-[var(--color-accent)]"
        bind:value={fragDraft}
        spellcheck="false"
        placeholder={activeDoc === null
          ? '所有 agent 共用的规则写这里'
          : `只给 ${activeAgent?.label ?? ''} 的补充内容。留空保存等于取消专用。`}
      ></textarea>
      {#if fragDirty}
        <div class="mt-1.5 text-[11px] text-[var(--color-warn)]">有未保存的改动</div>
      {/if}
      {#if activeDoc !== null && fragContent}
        <div class="mt-1.5 flex items-center gap-2">
          <span class="text-[11px] text-[var(--color-text-faint)]">不想保留这条专用内容？</span>
          <button
            class="text-[11px] text-[var(--color-danger)] underline-offset-2 hover:underline"
            onclick={() => clearSpecific(activeDoc!)}
            disabled={fragBusy}
          >
            取消专用片段
          </button>
        </div>
      {/if}
    {:else if activeDoc === null}
      {#if docState === 'loading'}
        <div class="h-24 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
      {:else if docState === 'missing'}
        <div class="py-6 text-center text-[13px] text-[var(--color-text-faint)]">
          通用母版还不存在。所有 agent 现在都读不到任何指令。
          <div class="mt-3">
            <Button size="sm" variant="primary" onclick={createDoc} disabled={docBusy}>
              {docBusy ? '创建中…' : '创建通用母版'}
            </Button>
          </div>
        </div>
      {:else if docState === 'error'}
        <div class="py-6 text-center text-[13px] text-[var(--color-danger)]">{docError}</div>
      {:else}
        <pre
          class="m-0 max-h-[320px] overflow-y-auto whitespace-pre-wrap break-words font-[var(--font-mono)] text-[12px] leading-relaxed text-[var(--color-text-muted)]">{docContent}</pre>
        <div class="mt-2 text-[11px] text-[var(--color-text-faint)]">
          {docContent.split('\n').length} 行 · {new Blob([docContent]).size} 字节 ·
          {agentDocs.filter((d) => !d.specificExists).length} 个 agent 直接读这份
        </div>
      {/if}
    {:else}
      <!-- 某个 agent 的专用片段 -->
      {#if !fragContent}
        <div class="py-6 text-center text-[13px] text-[var(--color-text-faint)]">
          {activeAgent?.label} 目前没有专用内容，只读通用母版。
          <div class="mt-3">
            <Button size="sm" variant="primary" onclick={startFragEdit}>
              <Plus size={13} />
              添加专用补充
            </Button>
          </div>
        </div>
      {:else}
        <pre
          class="m-0 max-h-[320px] overflow-y-auto whitespace-pre-wrap break-words font-[var(--font-mono)] text-[12px] leading-relaxed text-[var(--color-text-muted)]">{fragContent}</pre>
        <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[var(--color-text-faint)]">
          <span>
            合成后 {activeAgent?.bytes ?? 0} 字节 · {activeAgent?.label} 读「通用 + 专用」
          </span>
          <span class="font-[var(--font-mono)]">
            落点 {activeAgent?.target}
          </span>
          {#if activeAgent?.composedStale}
            <Badge tone="warn">合成待更新</Badge>
          {:else}
            <Badge tone="ok">合成已最新</Badge>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>

{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(6) as _}
      <div class="h-9 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else if mappings.length === 0}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    没有可用的映射。检查 ~/.agents/agentcenter.toml 是否存在。
  </div>
{:else}
  <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
    <table class="w-full border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="px-4 py-2.5 font-medium">Agent</th>
          <th class="px-4 py-2.5 font-medium">母版</th>
          <th class="px-4 py-2.5 font-medium">目标文件</th>
          <th class="px-4 py-2.5 font-medium">链接方式</th>
          <th class="px-4 py-2.5 font-medium">状态</th>
          <th class="px-4 py-2.5"></th>
        </tr>
      </thead>
      <tbody>
        {#each mappings as m (m.agent)}
          {@const h = healthLabel(m)}
          <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
            <td class="whitespace-nowrap px-4 py-2.5">{m.label}</td>
            <td class="px-4 py-2.5 font-[var(--font-mono)] text-[12px] text-[var(--color-text-muted)]">
              {m.source}
            </td>
            <td class="px-4 py-2.5 font-[var(--font-mono)] text-[12px] text-[var(--color-text-muted)]">
              {m.target}
            </td>
            <td class="px-4 py-2.5">
              {kindLabel(m.actual)}
              {#if m.mode !== 'auto' && m.actual && m.mode !== m.actual}
                <Badge class="ml-1">配置要求 {kindLabel(m.mode)}</Badge>
              {/if}
            </td>
            <td class="px-4 py-2.5">
              <Badge tone={h.tone}>{h.text}</Badge>
              {#if m.note}
                <div class="mt-0.5 text-[11px] text-[var(--color-text-faint)]">{m.note}</div>
              {/if}
            </td>
            <td class="px-4 py-2.5 text-right">
              <div class="flex items-center justify-end gap-1.5">
                {#if m.disabled}
                  <Button size="sm" onclick={() => restoreMapping(m)} disabled={busy !== null}>
                    {busy === m.agent ? '恢复中…' : '恢复映射'}
                  </Button>
                {:else}
                  {#if m.installed && m.health !== 'ok'}
                    <Button size="sm" onclick={() => repair(m.agent)} disabled={busy !== null}>
                      {busy === m.agent ? '修复中…' : '修复'}
                    </Button>
                  {/if}
                  {#if m.installed}
                    <Button
                      size="sm"
                      variant="ghost"
                      onclick={() => cancelMapping(m)}
                      disabled={busy !== null}
                      title="删除这个 agent 目录里的链接，并让后续同步跳过它"
                    >
                      取消映射
                    </Button>
                  {/if}
                {/if}
              </div>
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
