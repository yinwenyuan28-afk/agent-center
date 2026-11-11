<script lang="ts">
  /**
   * 日志输出区。
   *
   * 等宽字体、可滚动、保留换行。用于展示命令的原始输出，
   * 所以不做任何格式化处理——原样显示才能看到工具到底说了什么。
   */
  interface Props {
    text: string;
    maxHeight?: string;
  }

  let { text, maxHeight = '300px' }: Props = $props();
</script>

<pre
  class="overflow-y-auto m-0 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg-subtle)] p-3 font-[var(--font-mono)] text-[12px] leading-relaxed whitespace-pre-wrap break-all text-[var(--color-text-muted)]"
  style="max-height: {maxHeight}"
>{text}</pre>
