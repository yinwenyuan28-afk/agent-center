<script lang="ts">
  /**
   * 指令映射页。
   *
   * 这是工具最核心的一屏：列出每个 agent 的源头、落脚点、实际用的链接方式、
   * 以及当前是否健康。健康状态靠真后端检测，浏览器预览时走假数据。
   */
  import { onMount } from 'svelte';
  import { RefreshCw } from '@lucide/svelte';
  import {
    createDoc as createDocApi,
    listMappings,
    readDoc,
    repairMapping,
    saveDoc as saveDocApi,
    syncMappings,
  } from '../lib/ipc';
  import type { Mapping } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';

  let mappings = $state<Mapping[]>([]);
  let loading = $state(true);
  let busy = $state<string | null>(null);
  let log = $state('');

  // 母版内容区状态
  /** idle 表示还没读；ok 有内容；missing 文件不存在；error 读取失败 */
  let docState = $state<'loading' | 'ok' | 'missing' | 'error'>('loading');
  let docContent = $state('');
  let docDraft = $state('');
  let docError = $state('');
  let editingDoc = $state(false);
  let docBusy = $state(false);

  /** 草稿与已保存内容不同即视为有未保存改动。 */
  const docDirty = $derived(editingDoc && docDraft !== docContent);

  /** 把健康状态翻成界面用词，只在需要行动时用强色。 */
  function healthLabel(m: Mapping): { text: string; tone: 'ok' | 'warn' | 'danger' | 'plain' } {
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

  function startEdit() {
    docDraft = docContent;
    editingDoc = true;
  }

  function cancelDoc() {
    // 有改动时要求确认，避免误点丢掉编辑
    if (docDirty && !confirm('放弃未保存的改动？')) return;
    editingDoc = false;
    docDraft = '';
  }

  async function saveDoc() {
    docBusy = true;
    try {
      const r = await saveDocApi(docDraft);
      log = r.output;
      if (r.ok) {
        docContent = docDraft;
        editingDoc = false;
        // 内容变了，链接健康状态可能与之前不同，重新读一遍
        await load();
      }
    } catch (e) {
      log = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      docBusy = false;
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

  onMount(async () => {
    await load();
    await loadDoc();
  });

  const online = $derived(mappings.filter((m) => m.installed).length);
  const healthy = $derived(mappings.filter((m) => m.health === 'ok').length);
</script>

<PageHead title="AGENTS文档映射">
  把这些 agent 各自读取的 AGENTS 文档，统一指向同一份母版。每一行显示实际生效的链接方式，以及它现在是否还连着。
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

<!-- 母版内容区：查看与编辑 AGENTS.md 本体 -->
<div class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-border)]">
  <div class="flex items-center gap-2 border-b border-[var(--color-border)] px-4 py-2.5">
    <span class="text-[13px] font-medium">母版 AGENTS.md</span>
    <span class="font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]">
      {mappings[0]?.source ?? '~/.agents/AGENTS.md'}
    </span>
    <span class="flex-1"></span>
    {#if docState === 'loading'}
      <Badge>读取中</Badge>
    {:else if docState === 'missing'}
      <Badge tone="warn">文件不存在</Badge>
    {:else if docState === 'error'}
      <Badge tone="danger">读取失败</Badge>
    {:else if editingDoc}
      <Button size="sm" variant="primary" onclick={saveDoc} disabled={docBusy}>
        {docBusy ? '保存中…' : '保存'}
      </Button>
      <Button size="sm" variant="ghost" onclick={cancelDoc} disabled={docBusy}>取消</Button>
    {:else}
      <Button size="sm" onclick={startEdit} disabled={docState !== 'ok'}>编辑</Button>
      <Button size="sm" variant="ghost" onclick={loadDoc}> 
        <RefreshCw size={13} />
      </Button>
    {/if}
  </div>

  <div class="p-4">
    {#if docState === 'loading'}
      <div class="h-24 animate-pulse rounded-[var(--radius-sm)] bg-[var(--color-bg-muted)]"></div>
    {:else if docState === 'missing'}
      <div class="py-6 text-center text-[13px] text-[var(--color-text-faint)]">
        母版文件还不存在。先在终端执行 <code class="font-[var(--font-mono)]">agentcenter doc init</code> 建立，
        或点下面的按钮用当前内容创建。
        <div class="mt-3">
          <Button size="sm" variant="primary" onclick={createDoc} disabled={docBusy}>
            {docBusy ? '创建中…' : '创建母版文件'}
          </Button>
        </div>
      </div>
    {:else if docState === 'error'}
      <div class="py-6 text-center text-[13px] text-[var(--color-danger)]">{docError}</div>
    {:else if editingDoc}
      <textarea
        class="h-[320px] w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-3 py-2 font-[var(--font-mono)] text-[12px] leading-relaxed text-[var(--color-text)] focus:border-transparent focus:outline-2 focus:outline-[var(--color-accent)]"
        bind:value={docDraft}
        spellcheck="false"
      ></textarea>
      {#if docDirty}
        <div class="mt-1.5 text-[11px] text-[var(--color-warn)]">有未保存的改动</div>
      {/if}
    {:else}
      <pre
        class="m-0 max-h-[320px] overflow-y-auto whitespace-pre-wrap break-words font-[var(--font-mono)] text-[12px] leading-relaxed text-[var(--color-text-muted)]">{docContent}</pre>
      <div class="mt-2 text-[11px] text-[var(--color-text-faint)]">
        {docContent.split('\n').length} 行 · {new Blob([docContent]).size} 字节
      </div>
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
              {#if m.installed && m.health !== 'ok'}
                <Button size="sm" onclick={() => repair(m.agent)} disabled={busy !== null}>
                  {busy === m.agent ? '修复中…' : '修复'}
                </Button>
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
