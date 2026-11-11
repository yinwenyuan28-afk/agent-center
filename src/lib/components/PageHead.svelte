<script lang="ts">
  /**
   * 页面标题区。
   *
   * 标题 + 一句说明。说明限制最大宽度，避免长句在宽屏上拉成一整行难读。
   */
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    children?: Snippet;
  }

  let { title, children }: Props = $props();
</script>

<header class="mb-6">
  <h1 class="text-xl font-semibold tracking-tight text-[var(--color-text)]">{title}</h1>
  {#if children}
    <div class="mt-1 max-w-[68ch] text-[13px] text-[var(--color-text-muted)]">
      {@render children()}
    </div>
  {/if}
</header>
