<script lang="ts">
  /**
   * token 用量统计页。
   *
   * 数据不在这里记账，全部从各 agent 自己写的 session 文件里读。
   * 所以这一页本质是「汇总查看器」，没有任何写盘按钮 —— 唯一的写操作
   * 是切换某家是否纳入统计（存进 agentcenter.toml 的 [usage] enabled）。
   *
   * ## 为什么默认一家都不统计
   *
   * 这些数据源加起来上 GB（本机 pi 595 MB + codex 191 MB + zcode 1 GB 的库）。
   * 读不读、读谁，是用户自己的决定。页面首次打开时列表全灰，
   * 每行一个开关，勾上才读。
   *
   * ## 口径提示为什么必须显示
   *
   * 四家的原始 token 口径不一样，后端已经统一折算成「输入含缓存」。
   * 但折算规则和已知缺口（如 Codex 的压缩历史未纳入）必须让用户看见，
   * 否则看到数字却不知道它怎么来的，比不给数字更糟。
   */
  import { onMount } from 'svelte';
  import { RefreshCw, Coins, ChevronDown } from '@lucide/svelte';
  import { setUsageEnabled, usageReport } from '../lib/ipc';
  import type { UsageAgentStatus, UsageReport, UsageRow } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Switch from '../lib/components/Switch.svelte';
  import UsageTrend from '../lib/components/UsageTrend.svelte';
  import UsageShare from '../lib/components/UsageShare.svelte';

  /** 可选的时间窗。0 表示全部历史。 */
  const windows: { days: number; label: string }[] = [
    { days: 7, label: '最近 7 天' },
    { days: 30, label: '最近 30 天' },
    { days: 90, label: '最近 90 天' },
    { days: 0, label: '全部' },
  ];

  let days = $state(7);
  let report = $state<UsageReport | null>(null);
  let loading = $state(true);
  let busy = $state<string | null>(null);
  let error = $state('');
  /** 统计开关面板默认收起：日常看的是数字，改配置是偶发动作。 */
  let showPanel = $state(false);
  /** 按天明细表同样默认收起：几百行逐日数字是查问题用的，不是日常首屏。 */
  let showDaily = $state(false);

  /** 已启用的 agent 数。为 0 时页面主体是引导文案而不是空表格。 */
  const enabledCount = $derived(report?.agents.filter((a) => a.enabled).length ?? 0);

  /** 只有实现了采集的 agent 值得展示成「可开关」的一行。 */
  const supportedAgents = $derived(report?.agents.filter((a) => a.supported) ?? []);
  const unsupportedAgents = $derived(report?.agents.filter((a) => !a.supported) ?? []);

  /** 汇总：这四列直接相加是对的，后端已经归一过口径。 */
  const totals = $derived.by(() => {
    const rows = report?.rows ?? [];
    return {
      input: rows.reduce((n, r) => n + r.input, 0),
      output: rows.reduce((n, r) => n + r.output, 0),
      calls: rows.reduce((n, r) => n + r.calls, 0),
    };
  });

  /** 按 agent 汇总，用于顶部的小计条。 */
  const byAgent = $derived.by(() => {
    const map = new Map<string, { input: number; output: number; calls: number }>();
    for (const r of report?.rows ?? []) {
      const cur = map.get(r.agent) ?? { input: 0, output: 0, calls: 0 };
      cur.input += r.input;
      cur.output += r.output;
      cur.calls += r.calls;
      map.set(r.agent, cur);
    }
    // 按 input 降序，用得最多的排前面
    return [...map.entries()].sort((a, b) => b[1].input - a[1].input);
  });

  async function load(refresh = false) {
    loading = true;
    error = '';
    try {
      report = await usageReport(days, refresh);
    } catch (e) {
      error = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  /** 切换时间窗后立刻重查。数据在磁盘上，必须重新读。 */
  async function pickWindow(d: number) {
    if (d === days) return;
    days = d;
    await load();
  }

  /**
   * 切换某家的统计开关。
   *
   * 只写配置。真正读盘发生在紧接着的 load() —— 打开后立刻重查，
   * 用户不用再点一次「刷新」才知道有没有数据。
   */
  async function toggle(a: UsageAgentStatus, on: boolean) {
    busy = a.agent;
    try {
      const r = await setUsageEnabled(a.agent, on);
      if (!r.ok) {
        error = r.output;
        return;
      }
      await load();
    } catch (e) {
      error = `切换失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = null;
    }
  }

  /**
   * 大数字压缩成易读的写法。
   *
   * 几十亿 token 直接显示会占满一列且数不清位数，所以用万 / 亿为单位。
   * 但保留原始值的完整信息在 title 里，鼠标悬停能看到精确数字。
   */
  function fmt(n: number): string {
    if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(2)} 亿`;
    if (n >= 10_000) return `${(n / 10_000).toFixed(1)} 万`;
    return n.toLocaleString('zh-CN');
  }

  /** 原始数字加千分位，用于 title 提示。 */
  function exact(n: number): string {
    return n.toLocaleString('zh-CN');
  }

  /** agent id → 展示名。状态列表里已经有 label，这里取不到时退回 id。 */
  function labelOf(agent: string): string {
    return report?.agents.find((a) => a.agent === agent)?.label ?? agent;
  }

  /** 把行按日期分组，表格里用「日期」做分组标题。 */
  const groupedRows = $derived.by(() => {
    const groups: { date: string; rows: UsageRow[]; input: number; output: number }[] = [];
    for (const r of report?.rows ?? []) {
      const last = groups[groups.length - 1];
      if (last && last.date === r.date) {
        last.rows.push(r);
        last.input += r.input;
        last.output += r.output;
      } else {
        groups.push({ date: r.date, rows: [r], input: r.input, output: r.output });
      }
    }
    return groups;
  });

  onMount(load);
</script>

<PageHead title="Token 用量">
  从各 agent 自己写的会话文件里读用量，不额外记账。数据源动辄上 GB，所以默认一家都不读——
  在页面底部的「纳入统计的 agent」里勾选才开始统计。各家的原始口径不同（有的 input 含缓存、
  有的不含），这里已统一折算成「输入含缓存」，可以直接横向比较。
</PageHead>

<!-- 时间窗 -->
<div class="mb-4 flex flex-wrap items-center gap-2">
  {#each windows as w (w.days)}
    <Button
      variant={days === w.days ? 'primary' : 'outline'}
      size="sm"
      onclick={() => pickWindow(w.days)}
      disabled={loading || busy !== null}
    >
      {w.label}
    </Button>
  {/each}
  <span class="flex-1"></span>
  <Button onclick={() => load()} disabled={loading || busy !== null}>
    <RefreshCw size={14} class={loading ? 'animate-spin' : ''} />
    {loading ? '读取中…' : '重新统计'}
  </Button>
  <!--
    重算按钮和「重新统计」分开：平时那个只读新增的数据（不到半秒），
    这个会丢掉全部缓存从头扫（几十秒）。用户只有在怀疑数字不对时才需要它，
    所以做得低调一些，并带确认提示
  -->
  <Button
    variant="outline"
    onclick={async () => {
      if (!confirm('丢掉缓存从头重算？会重读全部历史数据，可能等几十秒。')) return;
      await load(true);
    }}
    disabled={loading || busy !== null}
    title="平时不需要：统计是增量的，只读新增的数据。只有怀疑数字不对时才用"
  >
    全量重算
  </Button>
</div>

{#if error}
  <div
    class="mb-4 rounded-[var(--radius-md)] border border-[var(--color-danger)] bg-[var(--color-danger-bg)] px-3 py-2 text-[12px] text-[var(--color-danger)]"
  >
    {error}
  </div>
{/if}

<!-- 结果区 -->
{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(4) as _}
      <div class="h-12 animate-pulse rounded-[var(--radius-lg)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else if enabledCount === 0}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] px-6 py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    还没有启用任何 agent。<br />
    展开页面底部的「纳入统计的 agent」，勾选一行右侧的开关就会开始统计。
  </div>
{:else if (report?.rows.length ?? 0) === 0}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] px-6 py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    已启用的 agent 在这个时间窗内没有用量记录。<br />
    试试切到「全部」，或者确认那家最近确实用过。
  </div>
{:else}
  <!-- 总计 -->
  <div class="mb-4 grid grid-cols-3 gap-3">
    <div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] px-4 py-3">
      <div class="text-[11px] tracking-wide text-[var(--color-text-faint)]">输入（含缓存）</div>
      <div class="mt-1 text-[20px] font-semibold tabular-nums" title={exact(totals.input)}>
        {fmt(totals.input)}
      </div>
    </div>
    <div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] px-4 py-3">
      <div class="text-[11px] tracking-wide text-[var(--color-text-faint)]">输出</div>
      <div class="mt-1 text-[20px] font-semibold tabular-nums" title={exact(totals.output)}>
        {fmt(totals.output)}
      </div>
    </div>
    <div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] px-4 py-3">
      <div class="text-[11px] tracking-wide text-[var(--color-text-faint)]">调用次数</div>
      <div class="mt-1 text-[20px] font-semibold tabular-nums" title={exact(totals.calls)}>
        {exact(totals.calls)}
      </div>
    </div>
  </div>

  <!-- 趋势折线图 -->
  {#if report && report.charts.models.length > 0}
    <div class="mb-4">
      <UsageTrend
        models={report.charts.models}
        trend={report.charts.trend}
        dates={report.charts.dates}
        format={fmt}
      />
    </div>

    <!-- 模型占比环形图 -->
    <div class="mb-4">
      <UsageShare shares={report.charts.shares} format={fmt} />
    </div>
  {/if}

  <!-- 按 agent 小计 -->
  <div class="mb-4 overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
    <table class="w-full border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="px-4 py-2.5 font-medium">agent</th>
          <th class="px-4 py-2.5 text-right font-medium">输入</th>
          <th class="px-4 py-2.5 text-right font-medium">输出</th>
          <th class="px-4 py-2.5 text-right font-medium">调用</th>
        </tr>
      </thead>
      <tbody>
        {#each byAgent as [agent, t] (agent)}
          <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
            <td class="px-4 py-2.5 font-medium">{labelOf(agent)}</td>
            <td class="px-4 py-2.5 text-right tabular-nums" title={exact(t.input)}>{fmt(t.input)}</td>
            <td class="px-4 py-2.5 text-right tabular-nums" title={exact(t.output)}>{fmt(t.output)}</td>
            <td class="px-4 py-2.5 text-right tabular-nums">{exact(t.calls)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <!--
    按天明细。默认收起：日常看的是上面的汇总和图表，逐行数字是查问题时才用的，
    全部历史摊开有几百行，默认展开会把页面拉得非常长
  -->
  <div class="rounded-[var(--radius-lg)] border border-[var(--color-border)]">
    <button
      type="button"
      onclick={() => (showDaily = !showDaily)}
      aria-expanded={showDaily}
      class="flex w-full items-center gap-1.5 px-4 py-3 text-left hover:bg-[var(--color-bg-hover)]"
    >
      <ChevronDown
        size={15}
        class="shrink-0 text-[var(--color-text-muted)] transition-transform {showDaily ? '' : '-rotate-90'}"
      />
      <Coins size={15} class="shrink-0 text-[var(--color-text-muted)]" />
      <span class="text-[14px] font-semibold">按天 × 模型</span>
      <span class="text-[11px] text-[var(--color-text-faint)]">
        {#if groupedRows.length > 0}
          {groupedRows.length} 天 · {report?.rows.length ?? 0} 行明细
        {:else}
          暂无明细
        {/if}
      </span>
    </button>

    {#if showDaily}
      <div class="overflow-hidden border-t border-[var(--color-border)]">
        <table class="w-full border-collapse text-[13px]">
      <thead>
        <tr
          class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
        >
          <th class="px-4 py-2.5 font-medium">日期</th>
          <th class="px-4 py-2.5 font-medium">agent</th>
          <th class="px-4 py-2.5 font-medium">模型</th>
          <th class="px-4 py-2.5 text-right font-medium">输入</th>
          <th class="px-4 py-2.5 text-right font-medium">输出</th>
          <th class="px-4 py-2.5 text-right font-medium">调用</th>
        </tr>
      </thead>
      <tbody>
        {#each groupedRows as g (g.date)}
          <!-- 日期做分组标题，同一天的行不再重复写日期 -->
          <tr class="border-b border-[var(--color-border)] bg-[var(--color-bg-subtle)]">
            <td class="px-4 py-2 font-medium" colspan="3">
              {g.date}
              <span class="ml-2 text-[11px] font-normal text-[var(--color-text-faint)]">
                小计 {fmt(g.input)} 输入 / {fmt(g.output)} 输出
              </span>
            </td>
            <td class="px-4 py-2 text-right tabular-nums text-[var(--color-text-muted)]" title={exact(g.input)}>
              {fmt(g.input)}
            </td>
            <td class="px-4 py-2 text-right tabular-nums text-[var(--color-text-muted)]" title={exact(g.output)}>
              {fmt(g.output)}
            </td>
            <td class="px-4 py-2 text-right text-[var(--color-text-faint)]">
              {g.rows.reduce((n, r) => n + r.calls, 0)}
            </td>
          </tr>
          {#each g.rows as r (r.agent + r.model)}
            <tr class="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-bg-subtle)]">
              <td class="px-4 py-2 text-[var(--color-text-faint)]"></td>
              <td class="px-4 py-2">{labelOf(r.agent)}</td>
              <td class="px-4 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-muted)]">
                {r.model}
              </td>
              <td class="px-4 py-2 text-right tabular-nums" title={exact(r.input)}>{fmt(r.input)}</td>
              <td class="px-4 py-2 text-right tabular-nums" title={exact(r.output)}>{fmt(r.output)}</td>
              <td class="px-4 py-2 text-right tabular-nums">{r.calls}</td>
            </tr>
          {/each}
        {/each}
      </tbody>
        </table>
      </div>
    {/if}
  </div>

  <p class="mt-3 text-[11px] leading-relaxed text-[var(--color-text-faint)]">
    说明：只统计真正消耗 token 的模型调用。请求失败或空回包（全 0 token）的记录已排除，否则调用次数会虚高。
    时间按本机时区切日。
  </p>
{/if}

<!-- 统计开关面板。放在最底部并默认收起——日常看数字，改配置是偶发动作 -->
<div class="mt-6 rounded-[var(--radius-lg)] border border-[var(--color-border)]">
  <button
    type="button"
    onclick={() => (showPanel = !showPanel)}
    aria-expanded={showPanel}
    class="flex w-full items-center gap-2 px-4 py-3 text-left hover:bg-[var(--color-bg-hover)]"
  >
    <ChevronDown
      size={15}
      class="shrink-0 text-[var(--color-text-muted)] transition-transform {showPanel ? '' : '-rotate-90'}"
    />
    <span class="text-[14px] font-semibold">纳入统计的 agent</span>
    <span class="text-[11px] text-[var(--color-text-faint)]">
      {enabledCount > 0 ? `${enabledCount} 家已启用` : '当前未启用任何一家'}
    </span>
  </button>

  {#if showPanel}
    <div class="border-t border-[var(--color-border)] px-4 py-2">
      <p class="py-1 text-[12px] text-[var(--color-text-muted)]">
        勾选后立刻读盘统计。关掉只是不再读，已发生的数据不会丢——它们本来就存在 agent 自己的目录里。
      </p>
    </div>

    <div class="divide-y divide-[var(--color-border)] border-t border-[var(--color-border)]">
      {#each supportedAgents as a (a.agent)}
        <div class="flex items-start gap-3 px-4 py-3">
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-[13px] font-medium">{a.label}</span>
              {#if a.enabled}
                {#if a.error}
                  <Badge tone="danger">读取失败</Badge>
                {:else if !a.available}
                  <Badge tone="warn">数据源不存在</Badge>
                {:else}
                  <Badge tone="ok">{exact(a.records)} 条记录</Badge>
                {/if}
              {:else}
                <Badge>未启用</Badge>
              {/if}
            </div>
            <!-- 数据源和口径说明都要给。用户得知道自己打开的是什么、数字怎么来的 -->
            <div class="mt-1 font-[var(--font-mono)] text-[11px] break-all text-[var(--color-text-faint)]">
              {a.source}
            </div>
            {#if a.note}
              <div class="mt-1 text-[11px] leading-relaxed text-[var(--color-text-muted)]">{a.note}</div>
            {/if}
            {#if a.error}
              <div class="mt-1 text-[11px] text-[var(--color-danger)]">{a.error}</div>
            {/if}
          </div>
          <Switch
            checked={a.enabled}
            disabled={busy !== null}
            label={`${a.label} 纳入统计`}
            onchange={(on) => toggle(a, on)}
          />
        </div>
      {/each}
    </div>

    {#if unsupportedAgents.length > 0}
      <div class="border-t border-[var(--color-border)] px-4 py-3">
        <div class="text-[12px] text-[var(--color-text-muted)]">
          暂不支持用量统计：
          <span class="font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]">
            {unsupportedAgents.map((a) => a.agent).join('、')}
          </span>
        </div>
      </div>
    {/if}
  {/if}
</div>
