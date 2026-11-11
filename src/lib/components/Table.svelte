<script lang="ts">
  /**
   * 表格外壳。
   *
   * 只负责边框、圆角、表头样式，内容由调用方用 snippet 填。
   * 抽出来是为了四个页面里的表格长得完全一致——
   * 否则每个页面各写一遍 th/td 的类名，迟早会出现细微差异。
   */
  import type { Snippet } from 'svelte';

  interface Props {
    head: Snippet;
    children: Snippet;
  }

  let { head, children }: Props = $props();
</script>

<div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
  <table class="w-full border-collapse text-[13px]">
    <thead>
      <tr
        class="border-b border-[var(--color-border)] text-left text-[11px] uppercase tracking-wider text-[var(--color-text-faint)]"
      >
        {@render head()}
      </tr>
    </thead>
    <tbody>
      {@render children()}
    </tbody>
  </table>
</div>
