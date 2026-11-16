<script lang="ts">
  /**
   * 环形图：各模型用量占比。
   *
   * 用手写 SVG 的 `stroke-dasharray` 画环，而不是 `path` 画扇形：
   * 环形图的每段本质上就是一段圆弧，dasharray 控制「画多少、空多少」
   * 比手算扇形路径简单得多，也不用处理大弧标志位。
   *
   * ## 为什么中间要写总量
   *
   * 环形图中间是空的，不写字就浪费了最好的位置。写上总量让用户
   * 一眼看到这块饼代表多大 —— 只看百分比不知道是 1 亿还是 10 亿。
   *
   * ## 为什么按占比排序并配文字列表
   *
   * 环形图本身读不出精确数值，只能看大致比例。旁边配一份
   * 「模型 / 百分比 / 绝对量」的列表，两者互补：图看分布，表看数字。
   */
  import type { ModelShare } from '../types';

  interface Props {
    shares: ModelShare[];
    /** 数字格式化，与折线图保持一致 */
    format: (n: number) => string;
  }

  let { shares, format }: Props = $props();

  /** 与折线图同一套配色，保持模型颜色在两个图里一致。 */
  const palette = [
    '#4d9dff',
    '#3ecf8e',
    '#f5a623',
    '#e8674f',
    '#a78bfa',
    '#22c3d6',
    '#e5c07b',
    '#8b93a7',
  ];

  function colorAt(i: number): string {
    return palette[i % palette.length];
  }

  /** 环形参数。半径取 70 让中间的两位数字放得下。 */
  const SIZE = 200;
  const R = 70;
  const STROKE = 34;
  const CIRC = 2 * Math.PI * R;
  const CENTER = SIZE / 2;

  /** 总量。中间那个数字。 */
  const total = $derived(shares.reduce((n, s) => n + s.tokens, 0));

  /**
   * 每段弧的 dasharray。
   *
   * 用一个累积偏移而不是 `stroke-dashoffset` 逐段设置：
   * dasharray 写成「画多长、空多长」，配合每段自己的 offset 就能接上。
   * 留 1px 间隙让相邻段在视觉上分得开。
   */
  const segments = $derived.by(() => {
    let acc = 0;
    return shares.map((s, i) => {
      const frac = total > 0 ? s.tokens / total : 0;
      const len = Math.max(0, frac * CIRC - 1);
      const offset = -acc;
      acc += frac * CIRC;
      return { share: s, color: colorAt(i), len, offset };
    });
  });
</script>

<div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
  <h2 class="mb-3 text-[14px] font-semibold">模型用量</h2>

  <div class="flex flex-wrap items-center gap-8">
    <svg
      viewBox="0 0 {SIZE} {SIZE}"
      class="shrink-0"
      style="width:{SIZE}px;height:{SIZE}px"
      role="img"
      aria-label="模型用量占比环形图"
    >
      <!-- 底环。用量为 0 时也有一圈淡色底，避免整个图空掉 -->
      <circle
        cx={CENTER}
        cy={CENTER}
        r={R}
        fill="none"
        stroke="var(--color-bg-muted)"
        stroke-width={STROKE}
      />

      <!-- 各模型的弧段。绕圆心旋转 -90° 让第一段从 12 点开始 -->
      {#each segments as seg (seg.share.model)}
        <circle
          cx={CENTER}
          cy={CENTER}
          r={R}
          fill="none"
          stroke={seg.color}
          stroke-width={STROKE}
          stroke-dasharray="{seg.len} {CIRC - seg.len}"
          stroke-dashoffset={seg.offset}
          transform="rotate(-90 {CENTER} {CENTER})"
        />
      {/each}

      <!-- 中间的总量 -->
      <text
        x={CENTER}
        y={CENTER - 4}
        text-anchor="middle"
        fill="var(--color-text)"
        font-size="19"
        font-weight="600"
      >{format(total)}</text>
      <text
        x={CENTER}
        y={CENTER + 13}
        text-anchor="middle"
        fill="var(--color-text-faint)"
        font-size="10"
      >tokens</text>
    </svg>

    <!-- 明细列表：图看比例，表看数字 -->
    <div class="min-w-0 flex-1">
      {#each segments as seg (seg.share.model)}
        <div class="flex items-center gap-2 py-1.5">
          <span class="size-2.5 shrink-0 rounded-full" style="background:{seg.color}"></span>
          <span class="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px]">
            {seg.share.model}
          </span>
          <span class="shrink-0 text-right text-[11px] tabular-nums text-[var(--color-text-muted)]">
            {seg.share.percent.toFixed(1)}%
          </span>
        </div>
        <div class="mb-1 pl-4.5 text-[11px] tabular-nums text-[var(--color-text-faint)]">
          {format(seg.share.tokens)} tokens · {seg.share.calls.toLocaleString('zh-CN')} 次调用
        </div>
      {/each}
    </div>
  </div>
</div>
