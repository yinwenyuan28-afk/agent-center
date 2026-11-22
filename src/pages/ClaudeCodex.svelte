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
   */
  import { onMount } from 'svelte';
  import { RefreshCw, TriangleAlert } from '@lucide/svelte';
  import {
    claudeOnboardingStatus,
    listProviders,
    setClaudeOnboardingSkip,
    setCodexRequiresOpenaiAuth,
  } from '../lib/ipc';
  import type { ClaudeOnboardingStatus, ProviderView } from '../lib/types';
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
      const [status, list] = await Promise.all([
        claudeOnboardingStatus().catch(() => null),
        listProviders(),
      ]);
      onboarding = status;
      providers = list.map((p) => ({ ...p, codexRequiresOpenaiAuth: p.codexRequiresOpenaiAuth ?? null }));
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

  /** 切换 Codex 后端开关。value 为 null 表示自动。 */
  async function setCodexAuth(value: boolean | null) {
    const p = codexProvider;
    if (!p) return;
    const prev = p.codexRequiresOpenaiAuth ?? null;
    if (prev === value) return;

    providers = providers.map((x) =>
      x.id === p.id ? { ...x, codexRequiresOpenaiAuth: value } : x,
    );
    busy = true;
    try {
      const r = await setCodexRequiresOpenaiAuth(p.id, value);
      log = r.output;
      if (!r.ok) throw new Error(r.output);
      await load();
    } catch (e) {
      providers = providers.map((x) =>
        x.id === p.id ? { ...x, codexRequiresOpenaiAuth: prev } : x,
      );
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  onMount(load);
</script>

<PageHead title="Claude & Codex">
  这两个设置只能改配置文件，两家工具都没有界面开关。
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
        <p class="text-[12px] leading-relaxed text-[var(--color-text-muted)]">
          Codex 是单槽位：同一时刻只有一个 provider 生效。去
          <span class="font-medium text-[var(--color-text)]">Provider</span>
          页把某个 provider 分配给 Codex，这里就会出现它的后端开关。
        </p>
      {:else}
        <div class="mb-3">
          <div class="flex flex-wrap items-center gap-2">
            <span class="font-[var(--font-mono)] text-[13px] font-medium">{codexProvider.id}</span>
            {#if codexProvider.name !== codexProvider.id}
              <span class="text-[12px] text-[var(--color-text-muted)]">{codexProvider.name}</span>
            {/if}
          </div>
          <div
            class="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
            title={codexProvider.baseUrl}
          >
            {codexProvider.baseUrl}
          </div>
        </div>

        <div class="flex flex-col gap-2">
          {#each CODEX_AUTH_OPTIONS as opt}
            {@const selected = (codexProvider.codexRequiresOpenaiAuth ?? null) === opt.value}
            <button
              type="button"
              class="rounded-[var(--radius-md)] border px-3 py-2.5 text-left transition-colors {selected
                ? 'border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)]'
                : 'border-[var(--color-border-strong)] hover:border-[var(--color-accent)]'}"
              aria-pressed={selected}
              disabled={busy}
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
            这个值写到 <code class="font-[var(--font-mono)]">~/.codex/config.toml</code> 的
            <code class="font-[var(--font-mono)]">[model_providers.custom].requires_openai_auth</code>。
            选「走 ChatGPT 后端」会把你的官方登录凭据发给该 provider 的地址。
          </span>
        </div>
      {/if}
    </section>
  </div>
{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} maxHeight="240px" />
  </div>
{/if}
