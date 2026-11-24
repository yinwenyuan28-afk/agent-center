<script lang="ts">
  /**
   * Claude & Codex 专属设置页。
   *
   * 这两家各有一个「配置文件里存在、但没有界面入口」的开关，
   * 都不属于 provider 池本身的增删改，所以单独一页：
   *
   * 1. **Claude Code — 跳过首次引导**
   *    全局开关，写 `~/.claude.json` 的 `hasCompletedOnboarding`。
   *    和用哪个 provider 无关。
   *
   * 2. **Codex — 是否绕过 ChatGPT 后端**
   *    这是**每个 provider 各自**的设置，写母版里那一条的
   *    `codex_requires_openai_auth`，再落到 `config.toml` 的
   *    `[model_providers.custom].requires_openai_auth`。
   *    Codex 是单槽位，所以只有当前分配给 Codex 的那一条会真正生效。
   *
   * 3. **Codex — 连接参数**
   *    `supports_websockets`（关掉无效重连）和 `model_catalog_url`
   *    （补回模型目录）。显示值读自 `~/.codex/config.toml`，写入走母版
   *    同一组字段；没有分配 provider 时整块变灰，因为值无处可落。
   */
  import { onMount } from 'svelte';
  import { RefreshCw, TriangleAlert } from '@lucide/svelte';
  import {
    claudeOnboardingStatus,
    codexConnectionStatus,
    listProviders,
    setClaudeOnboardingSkip,
    setCodexConnectionDirect,
    setCodexRequiresOpenaiAuthDirect,
  } from '../lib/ipc';
  import type { ClaudeOnboardingStatus, CodexConnectionStatus, ProviderView } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Switch from '../lib/components/Switch.svelte';
  import Tip from '../lib/components/Tip.svelte';
  import Log from '../lib/components/Log.svelte';

  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');

  /** Claude 首次引导状态。null 表示还没读到（或读失败）。 */
  let onboarding = $state<ClaudeOnboardingStatus | null>(null);

  /** 母版里的全部 provider，用来找分配给 codex 的那一条。 */
  let providers = $state<ProviderView[]>([]);

  /** 当前生效的 Codex provider（单槽位，最多一个）。 */
  let codexProvider = $derived(providers.find((p) => p.agents.includes('codex')) ?? null);

  /**
   * `~/.codex/config.toml` 里的连接参数实际值。
   *
   * 显示以盘上为准：用户可能直接改过配置文件，母版只作为写入记录。
   */
  let conn = $state<CodexConnectionStatus | null>(null);

  /**
   * 「连接修复」开关是否已生效。
   *
   * 两个键都到位才算开：只关 WebSocket 没补模型目录，模型列表还是空的。
   */
  const connectionOn = $derived(
    conn?.supportsWebsockets === false && !!(conn?.modelCatalogUrl ?? '').trim(),
  );

  /** 保存连接设置时置位，避免重复点击和并发写入。 */
  let connSaving = $state(false);

  /** Claude 引导的三态说明，和 Codex 的后端三态分开定义，避免混用文案。 */
  const CODEX_AUTH_OPTIONS = [
    {
      value: null as boolean | null,
      label: '自动',
      desc: '看 ~/.codex/auth.json 里有没有官方登录。有登录走 ChatGPT 后端，没登录只认 provider 里的 token。',
    },
    {
      value: false as boolean | null,
      label: '绕过 ChatGPT 后端',
      desc: 'requires_openai_auth = false。只用 provider 表里的 experimental_bearer_token。用第三方中转时选这个。',
    },
    {
      value: true as boolean | null,
      label: '走 ChatGPT 后端',
      desc: 'requires_openai_auth = true。带着官方登录去请求，只适合官方镜像。配第三方端点会把登录凭据发给对方。',
    },
  ];

  async function load() {
    loading = true;
    try {
      const [status, list, connStatus] = await Promise.all([
        claudeOnboardingStatus().catch(() => null),
        listProviders(),
        codexConnectionStatus().catch(() => null),
      ]);
      onboarding = status;
      providers = list.map((p) => ({
        ...p,
        codexRequiresOpenaiAuth: p.codexRequiresOpenaiAuth ?? null,
        codexSupportsWebsockets: p.codexSupportsWebsockets ?? null,
        codexModelCatalogUrl: p.codexModelCatalogUrl ?? null,
      }));
      conn = connStatus;
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  /** 切换 Claude 首次引导跳过。 */
  async function toggleOnboarding(skip: boolean) {
    if (!onboarding) return;
    const prev = onboarding.skipped;
    onboarding = { ...onboarding, skipped: skip };
    busy = true;
    try {
      const r = await setClaudeOnboardingSkip(skip);
      log = r.output;
      onboarding = await claudeOnboardingStatus();
    } catch (e) {
      onboarding = { ...onboarding, skipped: prev };
      log = `切换失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  /**
   * 切换 Codex 后端开关。
   *
   * 显示和写入都以 `config.toml` 当前生效的表为准：value 为 null 表示
   * 删掉 `requires_openai_auth`，交给 Codex 按盘上登录态自动判断。
   * 不经过母版，也不需要母版里分配过 provider。
   */
  async function setCodexAuth(value: boolean | null) {
    if (!conn?.providerId || busy) return;
    if ((conn.requiresOpenaiAuth ?? null) === value) return;

    busy = true;
    try {
      const r = await setCodexRequiresOpenaiAuthDirect(value);
      log = r.output;
      if (!r.ok) throw new Error(r.output);
      await load();
    } catch (e) {
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  /**
   * 猜模型目录地址：多数端点是 `<baseUrl>/models`（去掉末尾斜杠）。
   *
   * 优先用 config.toml 里生效表的 base_url —— 没分配 provider 时也有值。
   */
  function suggestCatalogUrl(): string {
    const base = conn?.baseUrl ?? codexProvider?.baseUrl ?? '';
    if (!base) return '';
    return `${base.replace(/\/+$/, '')}/models`;
  }

  /**
   * 开启/关闭「连接修复」。
   *
   * 开：一次写两个键 —— `supports_websockets = false` 和 `model_catalog_url`。
   * 目录优先沿用盘上已有值，没有就按 `base_url/models` 推导。
   * 关：两个键一起删掉，恢复 Codex 默认。
   * 用户不需要再单独保存地址。
   */
  async function toggleConnection(on: boolean) {
    if (on) {
      const url = (conn?.modelCatalogUrl ?? '').trim() || suggestCatalogUrl();
      await writeConnection(false, url || null);
    } else {
      await writeConnection(null, null);
    }
  }

  /**
   * 直接写 `~/.codex/config.toml` 当前生效的 provider 表，然后重新读盘刷新。
   *
   * 不依赖母版里有没有分配 provider：这两个键只影响 Codex 自己的连接行为。
   * 写完读盘而不是信任本地状态，保证界面显示的是文件里的真实值。
   */
  async function writeConnection(websockets: boolean | null, catalogUrl: string | null) {
    if (!conn?.providerId || connSaving) return;
    connSaving = true;
    try {
      const r = await setCodexConnectionDirect(websockets, catalogUrl);
      log = r.output;
      if (!r.ok) throw new Error(r.output);
      await load();
    } catch (e) {
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      connSaving = false;
    }
  }

  onMount(load);
</script>

<PageHead title="Claude & Codex">
  这里的开关都直接改写配置文件，显示值以盘上实际配置为准。
</PageHead>

<div class="mb-4 flex items-center gap-2">
  <span class="flex-1"></span>
  <Button variant="ghost" onclick={load} disabled={busy || loading}>
    <RefreshCw size={14} />
    刷新
  </Button>
</div>

{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(2) as _, i (i)}
      <div class="h-24 animate-pulse rounded-[var(--radius-lg)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else}
  <div class="flex flex-col gap-3">
    <!-- ============ Claude Code ============ -->
    <section class="rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
      <div class="mb-3 flex items-center gap-2">
        <h2 class="text-[14px] font-medium text-[var(--color-text)]">Claude Code</h2>
        {#if onboarding && !onboarding.available}
          <Badge tone="plain">未安装</Badge>
        {:else if onboarding?.skipped}
          <Badge tone="info">已跳过引导</Badge>
        {:else if onboarding}
          <Badge tone="warn">会显示引导</Badge>
        {/if}
      </div>

      {#if !onboarding}
        <p class="text-[12px] text-[var(--color-text-faint)]">读不到 Claude Code 的状态。</p>
      {:else if !onboarding.available}
        <p class="text-[12px] leading-relaxed text-[var(--color-text-muted)]">
          这台机器上没找到 Claude Code（<code class="text-[11px]">~/.claude.json</code>
          和 <code class="text-[11px]">~/.claude</code> 都不存在）。装了之后回来刷新即可。
        </p>
      {:else}
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0">
            <div class="text-[13px] font-medium text-[var(--color-text)]">跳过首次引导</div>
            <p class="mt-1 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
              第一次启动不再要求选主题和登录 Anthropic 账号，直接进主界面。
            </p>
            <Tip label="它改了什么">
              开启后写入 <code class="text-[11px]">~/.claude.json</code> 的
              <code class="text-[11px]">hasCompletedOnboarding: true</code>；关闭是删掉这个字段
              （不是写 false），引导会重新出现。
            </Tip>
            <p
              class="mt-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
            >
              {onboarding.path}
            </p>
          </div>
          <Switch
            checked={onboarding.skipped}
            disabled={busy}
            label="跳过首次引导"
            onchange={toggleOnboarding}
          />
        </div>
      {/if}
    </section>

    <!-- ============ Codex ============ -->
    <section class="rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
      <div class="mb-3 flex items-center gap-2">
        <h2 class="text-[14px] font-medium text-[var(--color-text)]">Codex</h2>
        {#if !codexProvider}
          <Badge tone="plain">没有分配给 Codex</Badge>
        {/if}
      </div>

      {#if !codexProvider}
        <p class="mb-4 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
          Codex 是单槽位：母版里同一时刻只有一个 provider 生效。下面显示和修改的是
          <code class="text-[11px]">config.toml</code> 里当前生效的表，直接读写，
          不依赖母版有没有分配 provider。
        </p>
      {/if}

      {#if codexProvider}
        <div class="mb-3">
          <div class="flex flex-wrap items-center gap-2">
            <span class="font-[var(--font-mono)] text-[13px] font-medium">{codexProvider.id}</span>
            {#if codexProvider.name !== codexProvider.id}
              <span class="text-[12px] text-[var(--color-text-muted)]">{codexProvider.name}</span>
            {/if}
            <span class="text-[11px] text-[var(--color-text-faint)]">母版分配</span>
          </div>
          <div
            class="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
            title={codexProvider.baseUrl}
          >
            {codexProvider.baseUrl}
          </div>
        </div>
      {/if}

      <!-- 后端开关：显示值读自 config.toml 当前生效的表，写入也直接改那张表 -->
      <div class={conn?.providerId ? '' : 'opacity-50'}>
        <div class="flex flex-col gap-2">
          {#each CODEX_AUTH_OPTIONS as opt}
            {@const selected = (conn?.requiresOpenaiAuth ?? null) === opt.value}
            <button
              type="button"
              class="rounded-[var(--radius-md)] border px-3 py-2.5 text-left transition-colors {selected
                ? 'border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)]'
                : 'border-[var(--color-border-strong)] hover:border-[var(--color-accent)]'}"
              aria-pressed={selected}
              disabled={busy || !conn?.providerId}
              onclick={() => setCodexAuth(opt.value)}
            >
              <span class="flex items-center gap-2">
                <span
                  class="flex h-4 w-4 items-center justify-center rounded-full border {selected
                    ? 'border-[var(--color-accent)]'
                    : 'border-[var(--color-border-strong)]'}"
                >
                  {#if selected}
                    <span class="h-2 w-2 rounded-full bg-[var(--color-accent)]"></span>
                  {/if}
                </span>
                <span class="text-[13px] font-medium text-[var(--color-text)]">{opt.label}</span>
                {#if opt.value === null}
                  <span class="text-[11px] text-[var(--color-text-faint)]">（默认）</span>
                {/if}
              </span>
              <span class="mt-1 block pl-6 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
                {opt.desc}
              </span>
            </button>
          {/each}
        </div>

        <div
          class="mt-3 flex items-start gap-1.5 rounded-[var(--radius-md)] bg-[var(--color-bg-subtle)] px-3 py-2 text-[11px] leading-relaxed text-[var(--color-text-muted)]"
        >
          <TriangleAlert size={12} class="mt-0.5 shrink-0 text-[var(--color-warn)]" />
          <span>
            这个值写到 <code class="font-[var(--font-mono)]">~/.codex/config.toml</code> 当前生效表的
            <code class="font-[var(--font-mono)]">requires_openai_auth</code>。
            选「走 ChatGPT 后端」会把你的官方登录凭据发给该 provider 的地址。
          </span>
        </div>
      </div>

      <!--
        连接参数：直接写 config.toml 当前生效的表，不经过母版。
        没有分配 provider 也能用 —— 用户可能自己维护着 model_providers 表。
      -->
      <div
        class="mt-4 border-t border-[var(--color-border)] pt-3 {conn?.providerId
          ? ''
          : 'opacity-50'}"
      >
        <div class="mb-1 flex flex-wrap items-center gap-2">
          <span class="text-[13px] font-medium text-[var(--color-text)]">连接参数</span>
          {#if conn?.providerId}
            <Badge tone="plain">[model_providers.{conn.providerId}]</Badge>
          {/if}
        </div>
        <p class="mb-3 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
          只影响 Codex 自己的连接，直接写
          <code class="text-[11px]">~/.codex/config.toml</code>
          当前生效的表；不需要先在母版里分配 provider。
        </p>

        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0">
            <div class="text-[13px] font-medium text-[var(--color-text)]">修复连接</div>
            <p class="mt-0.5 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
              端点不支持 WebSocket 时会反复重连，模型目录拉不到时模型列表为空。
              开启后一次性写 <code class="text-[11px]">supports_websockets = false</code> 和
              <code class="text-[11px]">model_catalog_url</code>，不需要再单独保存地址。
            </p>
          </div>
          <Switch
            checked={connectionOn}
            disabled={busy || connSaving || !conn?.providerId}
            label="修复连接"
            onchange={toggleConnection}
          />
        </div>

        <div class="mt-3 rounded-[var(--radius-md)] bg-[var(--color-bg-subtle)] px-3 py-2">
          <div class="flex items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
            <span>模型目录 model_catalog_url</span>
            {#if connectionOn}
              <Badge tone="info">已写入</Badge>
            {/if}
          </div>
          <div class="mt-0.5 break-all font-[var(--font-mono)] text-[12px] text-[var(--color-text)]">
            {#if conn?.modelCatalogUrl}
              {conn.modelCatalogUrl}
            {:else if suggestCatalogUrl()}
              {suggestCatalogUrl()}
              <span class="text-[var(--color-text-faint)]">（开启时自动写入）</span>
            {:else}
              <span class="text-[var(--color-text-faint)]">无法推导：缺少 base_url</span>
            {/if}
          </div>
        </div>

        {#if !conn?.providerId}
          <p class="mt-2 text-[12px] leading-relaxed text-[var(--color-warn)]">
            没有可写的表：config.toml 顶层 <code class="text-[11px]">model_provider</code>
            指向的 <code class="text-[11px]">[model_providers.*]</code> 表不存在。
            先让 Codex 正常启动一次，或手动建好表再回来刷新。
          </p>
        {/if}
      </div>
    </section>
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} maxHeight="240px" />
  </div>
{/if}
