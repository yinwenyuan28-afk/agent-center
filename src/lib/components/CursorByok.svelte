<script lang="ts">
  /**
   * Cursor 自带密钥（BYOK）的管理面板。
   *
   * 单独成组件而不是写在 Providers.svelte 里，有两个原因：
   *
   * 1. 用户要求把它从「provider 管理」视角里移走 —— 那个视角只管 provider
   *    池子的增删改，Cursor 的 BYOK 是另一套东西（SQLite + 加密密钥）。
   * 2. 「启用 provider」视角里它要和别的 agent 卡片长得一致，所以样式要能
   *    跟着卡片走，而不是一个独立的大折叠块。
   *
   * 组件的职责边界：只管「把某个 provider 写进 Cursor」。选哪个 provider
   * 由外部传 `providers` + `bind:providerId` 决定，这样嵌进 agent 卡片时
   * 就可以直接沿用那一行选中的 provider。
   */
  import { onMount } from 'svelte';
  import { RefreshCw, TriangleAlert, Check, Trash2 } from '@lucide/svelte';
  import {
    cursorByokStatus,
    cursorByokPlan,
    cursorByokApply,
    cursorByokRemoveModels,
    cursorByokReset,
  } from '../ipc';
  import type { CursorByokPlan, CursorByokState, ProviderView, RunResult } from '../types';

  let {
    providers = [],
    /** 当前要写进 Cursor 的 provider id，双向绑定 */
    providerId = $bindable(''),
    /**
     * 紧凑模式：嵌在 agent 卡片里时用。
     *
     * 紧凑模式去掉外层边框和大标题，只保留状态行与操作行，
     * 让它和旁边的 provider 勾选卡片视觉重量一致。
     */
    compact = false,
    /** 操作完成后回调，方便父组件刷新别的部分 */
    onDone = () => {},
  }: {
    providers?: ProviderView[];
    providerId?: string;
    compact?: boolean;
    onDone?: (r: RunResult) => void;
  } = $props();

  let byok = $state<CursorByokState | null>(null);
  let plan = $state<CursorByokPlan | null>(null);
  let busy = $state(false);
  let error = $state('');
  /** 用户自己的操作结果，显示在组件内部而不是父组件的日志区 */
  let note = $state('');

  /**
   * 勾选要加进 Cursor 模型选择器的名字。
   *
   * 母版里一条 provider 往往有好几个模型，但用户可能只想用其中一两个，
   * 所以这里由用户勾，不自动全加。
   */
  let pickedModels = $state<Set<string>>(new Set());

  /** 当前 provider 在母版里定义的模型 id 列表。 */
  let availableModels = $derived(
    (providers.find((p) => p.id === providerId)?.models ?? []).map((m) => m.id).filter(Boolean),
  );

  /** 勾选集合转成数组，给 IPC 用。 */
  function pickedList(): string[] {
    return [...pickedModels];
  }

  /** 读状态并算一次计划。 */
  async function refresh() {
    try {
      byok = await cursorByokStatus();
      error = '';
      if (!providerId) {
        providerId = providers.find((p) => p.keyLen > 0)?.id ?? providers[0]?.id ?? '';
      }
      if (providerId) {
        plan = await cursorByokPlan(providerId, pickedList());
      } else {
        plan = null;
      }
    } catch (e) {
      byok = null;
      plan = null;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /** provider 变了，重算计划。 */
  async function replan() {
    if (!providerId) {
      plan = null;
      return;
    }
    try {
      plan = await cursorByokPlan(providerId, pickedList());
      error = '';
    } catch (e) {
      plan = null;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /** 切换某个模型是否加进选择器。 */
  async function toggleModel(id: string) {
    const next = new Set(pickedModels);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    pickedModels = next;
    await replan();
  }

  /**
   * 删掉 Cursor 里已存在的一个模型。
   *
   * 只改 userAddedModels，不动开关/地址/密钥 —— 所以单独一个 IPC。
   */
  async function removeModel(name: string) {
    if (!confirm(`从 Cursor 的模型选择器里移除「${name}」？\n\n这只删名字，不动接口配置。`)) return;
    busy = true;
    note = '';
    try {
      const r = await cursorByokRemoveModels([name]);
      note = r.output;
      await refresh();
      onDone(r);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  /** 写进 Cursor。Cursor 在运行时后端会拒绝。 */
  async function apply() {
    if (!providerId) return;
    busy = true;
    // 先清掉旧计划，避免写入过程中把上一轮的「还差什么」再画一遍
    plan = null;
    note = '';
    try {
      const r = await cursorByokApply(providerId, pickedList());
      note = r.output;
      // 写成功后清掉勾选，避免下次进来又「勾着但已加过」
      pickedModels = new Set();
      await refresh();
      onDone(r);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  /** 关掉自带密钥。密钥本身留在库里不清。 */
  async function reset() {
    if (!confirm('关掉 Cursor 的自带密钥、清空接口地址？\n\n密钥本身会留在库里不清，下次启用不用重填。')) {
      return;
    }
    busy = true;
    plan = null;
    note = '';
    try {
      const r = await cursorByokReset();
      note = r.output;
      await refresh();
      onDone(r);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  onMount(refresh);
</script>

{#if byok}
  <div class={compact ? '' : 'rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4'}>
    {#if !compact}
      <div class="mb-3 flex items-center gap-2">
        <span class="text-[13px] font-medium text-[var(--color-text)]">Cursor 自带密钥</span>
        {#if byok.running}
          <span class="rounded-full bg-[var(--color-warn-bg,#7a5a00)]/20 px-2 py-0.5 text-[11px] text-[var(--color-warn,#e0b341)]">
            Cursor 运行中
          </span>
        {:else if byok.useOpenaiKey}
          <span class="rounded-full bg-[var(--color-accent)]/15 px-2 py-0.5 text-[11px] text-[var(--color-accent)]">
            已生效
          </span>
        {/if}
        <span class="flex-1"></span>
        <button
          type="button"
          class="flex items-center gap-1 text-[12px] text-[var(--color-text-muted)] hover:text-[var(--color-text)]"
          onclick={refresh}
          disabled={busy}
        >
          <RefreshCw size={13} />
          刷新
        </button>
      </div>
    {/if}

    {#if !byok.installed}
      <p class="text-[12px] text-[var(--color-text-muted)]">
        没找到 Cursor 的配置库。装过并启动过一次之后再来。
        <br />
        <code class="font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
          >{byok.dbPath}</code
        >
      </p>
    {:else}
      <!-- 状态三格。和 agent 卡片里的说明文字同一套字号 -->
      <div class="mb-3 grid grid-cols-2 gap-x-4 gap-y-1.5 text-[12px] sm:grid-cols-4">
        <div>
          <div class="text-[var(--color-text-faint)]">总开关</div>
          <div class={byok.useOpenaiKey ? 'text-[var(--color-text)]' : 'text-[var(--color-danger)]'}>
            {byok.useOpenaiKey ? '已打开' : '未打开'}
          </div>
        </div>
        <div class="col-span-2">
          <div class="text-[var(--color-text-faint)]">接口地址</div>
          <div class="truncate font-[var(--font-mono)] text-[11px]" title={byok.baseUrl ?? ''}>
            {byok.baseUrl ?? '（未设置，走 OpenAI 官方）'}
          </div>
        </div>
        <div>
          <div class="text-[var(--color-text-faint)]">密钥</div>
          <div class="font-[var(--font-mono)] text-[11px]">
            {byok.keyLen === 0 ? '未存' : `${byok.keyLen} 字符 · …${byok.keyTail}`}
          </div>
        </div>
      </div>

      <!-- 操作行：选 provider + 写入 -->
      <div class="flex flex-wrap items-center gap-2">
        <select
          class="field-control min-w-0"
          bind:value={providerId}
          onchange={replan}
          disabled={busy}
        >
          {#each providers as p (p.id)}
            <option value={p.id}>{p.id} · {p.name}</option>
          {/each}
        </select>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[var(--radius-sm)] border px-2.5 py-1.5 text-[12px] transition-colors {plan?.noop
            ? 'border-[var(--color-border)] text-[var(--color-text-faint)]'
            : 'border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-text)]'}"
          disabled={busy || !providerId || byok.running || !plan || plan.noop}
          onclick={apply}
        >
          {#if plan?.noop}
            <Check size={12} />
          {/if}
          {busy ? '写入中…' : plan?.noop ? '已一致' : '写入 Cursor'}
        </button>
        {#if byok.useOpenaiKey}
          <button
            type="button"
            class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1.5 text-[12px] text-[var(--color-text-muted)] hover:text-[var(--color-text)]"
            disabled={busy || byok.running}
            onclick={reset}
          >
            关掉自带密钥
          </button>
        {/if}
      </div>

      <!-- 模型选择器。
           分两段：上面是母版里这个 provider 定义的模型（勾选加入），
           下面是 Cursor 里已经加过的（可删）。两者独立，因为 Cursor 里
           可能还留着别的 provider 或用户手加的模型。 -->
      <div class="mt-3 border-t border-[var(--color-border)] pt-3">
        <div class="mb-1.5 flex flex-wrap items-center gap-2">
          <span class="text-[12px] font-medium text-[var(--color-text)]">模型选择器</span>
          <span class="text-[11px] text-[var(--color-text-faint)]">
            勾选的会加进 Cursor 的模型列表；不勾不加
          </span>
        </div>

        <!-- 母版里的模型 -->
        {#if availableModels.length === 0}
          <div class="text-[11px] text-[var(--color-text-faint)]">
            这条 provider 在母版里还没有模型。先在「provider 管理」里加。
          </div>
        {:else}
          <div class="flex flex-col gap-1.5">
            {#each availableModels as id (id)}
              {@const inCursor = byok.userAddedModels.includes(id)}
              {@const checked = inCursor || pickedModels.has(id)}
              <div class="flex items-center gap-2">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-2 rounded-[var(--radius-sm)] border px-2.5 py-1.5 text-left transition-colors {checked
                    ? 'border-[var(--color-accent)] bg-[var(--color-accent)]/10'
                    : 'border-[var(--color-border-strong)] hover:border-[var(--color-accent)]'}"
                  disabled={busy || byok.running || inCursor}
                  aria-pressed={checked}
                  title={inCursor ? '已经在 Cursor 里了' : `把「${id}」加进 Cursor 的模型列表`}
                  onclick={() => toggleModel(id)}
                >
                  <span
                    class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border {checked
                      ? 'border-[var(--color-accent)] bg-[var(--color-accent)]'
                      : 'border-[var(--color-border-strong)]'}"
                  >
                    {#if checked}
                      <Check size={10} class="text-white" />
                    {/if}
                  </span>
                  <span class="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px]">
                    {id}
                  </span>
                  {#if inCursor}
                    <span class="shrink-0 text-[10px] text-[var(--color-text-faint)]">已在 Cursor</span>
                  {/if}
                </button>
                {#if inCursor}
                  <button
                    type="button"
                    class="shrink-0 rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] p-1.5 text-[var(--color-text-faint)] transition-colors hover:border-[var(--color-danger)] hover:text-[var(--color-danger)]"
                    title={`从 Cursor 移除 ${id}`}
                    aria-label={`从 Cursor 移除 ${id}`}
                    disabled={busy || byok.running}
                    onclick={() => removeModel(id)}
                  >
                    <Trash2 size={13} />
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Cursor 里有、但不在这个 provider 的模型列表里的 -->
        {#if byok.userAddedModels.filter((m) => !availableModels.includes(m)).length > 0}
          {@const extras = byok.userAddedModels.filter((m) => !availableModels.includes(m))}
          <div class="mt-2">
            <div class="mb-1 text-[11px] text-[var(--color-text-faint)]">
              Cursor 里还有这些模型，不在这条 provider 的列表里：
            </div>
            <div class="flex flex-wrap gap-1.5">
              {#each extras as m (m)}
                <span
                  class="flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] py-1 pl-2 pr-1 text-[11px] font-[var(--font-mono)] text-[var(--color-text-muted)]"
                >
                  {m}
                  <button
                    type="button"
                    class="rounded p-0.5 text-[var(--color-text-faint)] transition-colors hover:text-[var(--color-danger)]"
                    title={`从 Cursor 移除 ${m}`}
                    aria-label={`从 Cursor 移除 ${m}`}
                    disabled={busy || byok.running}
                    onclick={() => removeModel(m)}
                  >
                    <Trash2 size={11} />
                  </button>
                </span>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- 还差什么 -->
      {#if error}
        <div class="mt-3 flex items-start gap-1.5 text-[12px] text-[var(--color-danger)]">
          <TriangleAlert size={13} class="mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      {:else if plan && !plan.noop}
        <div
          class="mt-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg-muted)] px-3 py-2"
        >
          <div class="mb-1 text-[12px] font-medium text-[var(--color-text)]">
            要让 {providerId} 生效，需要做这些事
          </div>
          <ol class="ml-4 list-decimal space-y-0.5 text-[12px] text-[var(--color-text-muted)]">
            {#each plan.summary as line, i (i)}
              <li>{line}</li>
            {/each}
          </ol>
        </div>
      {/if}

      <!-- Cursor 在运行时不允许写 -->
      {#if byok.running}
        <div class="mt-3 flex items-start gap-1.5 text-[12px] text-[var(--color-danger)]">
          <TriangleAlert size={13} class="mt-0.5 shrink-0" />
          <span>
            Cursor 正在运行，现在写入会被它内存里的状态覆盖。请完全退出 Cursor（托盘图标也要退）后再操作。
          </span>
        </div>
      {/if}

      {#if !compact}
        <p class="mt-3 text-[12px] leading-relaxed text-[var(--color-text-faint)]">
          Cursor 的配置存在
          <code class="font-[var(--font-mono)] text-[11px]">byok.vscdb</code>
          里，密钥经 Electron safeStorage 加密，所以它不走「应用到各 agent」。
          写入后需要重启 Cursor 才生效。
        </p>
      {/if}
    {/if}

    {#if note}
      <pre
        class="mt-3 max-h-32 overflow-auto whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg-muted)] px-3 py-2 text-[11px] text-[var(--color-text-muted)]">{note}</pre>
    {/if}
  </div>
{/if}
