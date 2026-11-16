<script lang="ts">
  /**
   * 折线图：多模型每日 token 趋势。
   *
   * 手写 SVG 而不是引图表库，理由和仓库其他部分一致：只需要一条折线，
   * ECharts 会带来几百 KB。这里不到 200 行。
   *
   * ## 为什么用平滑曲线而不是折线
   *
   * 每日用量是离散点，但用户想看的趋势（爬升、平台、回落）用直线段
   * 会显得锯齿突兀。这里用 Catmull-Rom 样条转贝塞尔，并在生成控制点时
   * **限制过冲**：样条容易在陡升前先往下压一点，画出实际不存在的低谷。
   * 所以纵向控制点被夹在两端点的取值区间内。
   *
   * ## 一条线只有一个模型
   *
   * 不同模型的量级可能差几个数量级（几十亿 vs 几千）。同轴叠画会让小的
   * 那条贴在 x 轴上完全看不见。这是有意的：用户要看的是「谁吃得多」，
   * 小模型本就在饼图里体现占比。
   */
  import type { TrendPoint } from '../types';

  interface Props {
    /** 模型名列表，决定线的数量和顺序 */
    models: string[];
    /** 数据点 */
    trend: TrendPoint[];
    /** 横轴日期，升序 */
    dates: string[];
    /** 数字格式化，由父组件传入以保持单位一致 */
    format: (n: number) => string;
  }

  let { models, trend, dates, format }: Props = $props();

  /**
   * 配色。深色主题下要够亮才能分辨，同时相邻色相拉开距离。
   * 8 个颜色对应后端保留的最大模型数（其余合并成「其他」）。
   */
  const palette = [
    '#4d9dff', // 蓝
    '#3ecf8e', // 绿
    '#f5a623', // 橙
    '#e8674f', // 红
    '#a78bfa', // 紫
    '#22c3d6', // 青
    '#e5c07b', // 沙
    '#8b93a7', // 灰（给「其他」用）
  ];

  /** 画布尺寸。宽度用 viewBox 自适应，高度固定。 */
  const W = 900;
  const H = 260;
  /** 左侧给 y 轴标签留位置 */
  const PAD_L = 64;
  const PAD_R = 16;
  const PAD_T = 16;
  const PAD_B = 36;

  const plotW = $derived(W - PAD_L - PAD_R);
  const plotH = $derived(H - PAD_T - PAD_B);

  /** 折线图按「模型」上色，颜色跟着模型名走，不随筛选变化。 */
  function colorOf(model: string): string {
    const i = models.indexOf(model);
    return palette[(i < 0 ? models.length : i) % palette.length];
  }

  /**
   * 每个模型在每天的量。缺的日期补 0 —— 不断线才是对的，
   * 断线会让人误以为那天没有数据源，其实是那天没用到这个模型。
   */
  const series = $derived.by(() => {
    const map = new Map<string, Map<string, number>>();
    for (const m of models) map.set(m, new Map());
    for (const p of trend) {
      const inner = map.get(p.model);
      if (inner) inner.set(p.date, (inner.get(p.date) ?? 0) + p.tokens);
    }
    return models.map((m) => ({
      model: m,
      values: dates.map((d) => map.get(m)?.get(d) ?? 0),
    }));
  });

  /** y 轴上界。取所有模型所有日期的最大值。 */
  const maxValue = $derived.by(() => {
    let max = 0;
    for (const s of series) for (const v of s.values) if (v > max) max = v;
    return max > 0 ? max : 1;
  });

  /** 横轴 x 坐标：日期均分。只有一天时放中间，避免除零。 */
  function xAt(i: number): number {
    if (dates.length <= 1) return PAD_L + plotW / 2;
    return PAD_L + (plotW * i) / (dates.length - 1);
  }

  /** 纵轴 y 坐标：值越大越靠上。 */
  function yAt(v: number): number {
    return PAD_T + plotH - (plotH * v) / maxValue;
  }

  /**
   * 把一串点连成平滑曲线。
   *
   * 用三次贝塞尔的 Catmull-Rom 近似：每个点两侧的切线取相邻点连线的斜率。
   * 关键是控制点的 y 被夹在相邻两点的取值区间内 —— 否则样条会在
   * 陡升/陡降处过冲，画出实际不存在的峰谷。
   */
  function smoothPath(values: number[]): string {
    if (values.length === 0) return '';
    if (values.length === 1) {
      // 只有一个点：画个小圆点，不能画成空路径
      const x = xAt(0);
      const y = yAt(values[0]);
      return `M ${x} ${y} L ${x + 0.01} ${y}`;
    }

    const pts = values.map((v, i) => ({ x: xAt(i), y: yAt(v) }));
    let d = `M ${pts[0].x} ${pts[0].y}`;

    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[i - 1] ?? pts[i];
      const p1 = pts[i];
      const p2 = pts[i + 1];
      const p3 = pts[i + 2] ?? p2;

      // 缩放 1/6 是 Catmull-Rom 转贝塞尔的系数
      const c1x = p1.x + (p2.x - p0.x) / 6;
      const c2x = p2.x - (p3.x - p1.x) / 6;

      // 限制纵向过冲：控制点的 y 不越出 p1..p2 的取值区间
      const lo = Math.min(p1.y, p2.y);
      const hi = Math.max(p1.y, p2.y);
      const c1y = Math.min(hi, Math.max(lo, p1.y + (p2.y - p0.y) / 6));
      const c2y = Math.min(hi, Math.max(lo, p2.y - (p3.y - p1.y) / 6));

      d += ` C ${c1x} ${c1y}, ${c2x} ${c2y}, ${p2.x} ${p2.y}`;
    }
    return d;
  }

  /** 横向网格线，4 等分。 */
  const gridLines = $derived(
    [0, 0.25, 0.5, 0.75, 1].map((f) => ({
      y: PAD_T + plotH - plotH * f,
      label: format(Math.round(maxValue * f)),
    })),
  );

  /**
   * 横轴标签。日期多时隔几个显示一个，否则标签会叠在一起。
   * 始终保留首尾两天，用户能立刻看出时间范围。
   */
  const xLabels = $derived.by(() => {
    const n = dates.length;
    if (n === 0) return [];
    const maxLabels = 10;
    const step = Math.max(1, Math.ceil(n / maxLabels));
    const out: { x: number; label: string; text: string; anchor: string }[] = [];
    for (let i = 0; i < n; i++) {
      if (i % step !== 0 && i !== n - 1) continue;
      // 只显示「月日」，年份在一个窗口内不会变
      const [, mm, dd] = dates[i].split('-');
      // 首尾两个标签改成左/右对齐，否则居中的文字会超出 viewBox 被裁掉
      const anchor = i === 0 ? 'start' : i === n - 1 ? 'end' : 'middle';
      out.push({ x: xAt(i), label: dates[i], text: `${Number(mm)}月${Number(dd)}日`, anchor });
    }
    return out;
  });

  /** 悬停的那一天。null 表示没悬停。 */
  let hoverIndex = $state<number | null>(null);

  /** 图上任意位置对应的最近日期索引。 */
  function indexFromEvent(e: MouseEvent): number | null {
    if (dates.length === 0) return null;
    const svg = e.currentTarget as SVGSVGElement;
    const rect = svg.getBoundingClientRect();
    // 屏幕坐标换算回 viewBox 坐标
    const vx = ((e.clientX - rect.left) / rect.width) * W;
    const ratio = (vx - PAD_L) / plotW;
    if (dates.length === 1) return 0;
    const idx = Math.round(ratio * (dates.length - 1));
    return Math.min(dates.length - 1, Math.max(0, idx));
  }
</script>

<div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
  <h2 class="mb-1 text-[14px] font-semibold">每日 Token 趋势</h2>
  <p class="mb-3 text-[11px] text-[var(--color-text-faint)]">
    按模型分组。量级相差大的线会贴近底部，那是真实的量差异。
  </p>

  <!-- 图例：颜色 + 模型名 -->
  <div class="mb-2 flex flex-wrap gap-x-4 gap-y-1.5">
    {#each models as m (m)}
      <span class="flex items-center gap-1.5 text-[11px] text-[var(--color-text-muted)]">
        <span class="size-2.5 shrink-0 rounded-full" style="background:{colorOf(m)}"></span>
        <span class="font-[var(--font-mono)]">{m}</span>
      </span>
    {/each}
  </div>

  <div class="relative">
    <svg
      viewBox="0 0 {W} {H}"
      class="w-full"
      style="height:auto"
      role="img"
      aria-label="每日 token 趋势图"
      onmousemove={indexFromEvent.length ? (e) => (hoverIndex = indexFromEvent(e)) : undefined}
      onmouseleave={() => (hoverIndex = null)}
    >
      <!-- 横向网格 + y 轴刻度 -->
      {#each gridLines as g}
        <line
          x1={PAD_L}
          y1={g.y}
          x2={W - PAD_R}
          y2={g.y}
          stroke="var(--color-border)"
          stroke-width="1"
          stroke-dasharray={g.y === PAD_T + plotH ? '0' : '3 4'}
        />
        <text
          x={PAD_L - 8}
          y={g.y + 3.5}
          text-anchor="end"
          fill="var(--color-text-faint)"
          font-size="10"
        >{g.label}</text>
      {/each}

      <!-- 悬停竖线 -->
      {#if hoverIndex !== null}
        <line
          x1={xAt(hoverIndex)}
          y1={PAD_T}
          x2={xAt(hoverIndex)}
          y2={PAD_T + plotH}
          stroke="var(--color-border-strong)"
          stroke-width="1"
        />
      {/if}

      <!-- 每个模型一条曲线 -->
      {#each series as s (s.model)}
        <path
          d={smoothPath(s.values)}
          fill="none"
          stroke={colorOf(s.model)}
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <!-- 悬停时在曲线上标点 -->
        {#if hoverIndex !== null}
          <circle
            cx={xAt(hoverIndex)}
            cy={yAt(s.values[hoverIndex] ?? 0)}
            r="3.5"
            fill={colorOf(s.model)}
            stroke="var(--color-bg)"
            stroke-width="1.5"
          />
        {/if}
      {/each}

      <!-- 横轴日期 -->
      {#each xLabels as l}
        <text
          x={l.x}
          y={H - PAD_B + 20}
          text-anchor={l.anchor}
          fill="var(--color-text-faint)"
          font-size="10"
        >{l.text}</text>
      {/each}
    </svg>

    <!-- 悬停详情：列出当天所有模型的量，按大小排 -->
    {#if hoverIndex !== null}
      {@const hi = hoverIndex}
      <div
        class="pointer-events-none absolute top-0 right-0 rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-3 py-2 text-[11px] shadow-lg"
      >
        <div class="mb-1 font-medium">{dates[hi]}</div>
        {#each [...series].sort((a, b) => (b.values[hi] ?? 0) - (a.values[hi] ?? 0)) as s (s.model)}
          {#if (s.values[hi] ?? 0) > 0}
            <div class="flex items-center justify-between gap-3">
              <span class="flex items-center gap-1.5">
                <span class="size-2 rounded-full" style="background:{colorOf(s.model)}"></span>
                <span class="font-[var(--font-mono)]">{s.model}</span>
              </span>
              <span class="tabular-nums">{format(s.values[hi])}</span>
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
</div>
