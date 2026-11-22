<script lang="ts">
  /**
   * 折叠说明（tip）。
   *
   * 页头只留一句「这页是干什么的」。像实现细节、边界条件、注意事项这类
   * 看一遍就够的内容收进这里，默认收起，点开才展开，避免常驻大段文字。
   *
   * 用法：
   *   <Tip label="关于指纹">
   *     这里记录的是来源仓库和内容指纹……
   *   </Tip>
   */
  import type { Snippet } from 'svelte';
  import { Info, ChevronDown } from '@lucide/svelte';
  import { cn } from '../utils';

  interface Props {
    /** 收起时按钮上的文字。写清展开后能读到什么，别只写「说明」 */
    label?: string;
    class?: string;
    children: Snippet;
  }

  let { label = '说明', class: className = '', children }: Props = $props();

  /** 展开状态只活在这个组件里，换页即重置，不需要持久化 */
  let open = $state(false);
</script>

<div class={cn('text-[12px]', className)}>
  <button
    type="button"
    class="inline-flex items-center gap-1 rounded-[var(--radius-sm)] px-1.5 py-0.5 text-[11px] text-[var(--color-text-faint)] transition-colors hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text-muted)]"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <Info size={12} />
    {label}
    <ChevronDown size={12} class="transition-transform {open ? 'rotate-180' : ''}" />
  </button>

  {#if open}
    <div
      class="mt-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-bg-subtle)] px-3 py-2 leading-relaxed text-[var(--color-text-muted)]"
    >
      {@render children()}
    </div>
  {/if}
</div>
