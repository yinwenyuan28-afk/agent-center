<script lang="ts">
  /**
   * 徽标：表达状态。
   *
   * 和 ChatGPT 的做法一致——状态用低饱和的底色 + 同色系深色文字，
   * 不用鲜艳填充，避免在表格里一片眼花。
   */
  import type { Snippet } from 'svelte';
  import { cn } from '../utils';

  type Tone = 'ok' | 'warn' | 'danger' | 'info' | 'plain';

  interface Props {
    tone?: Tone;
    class?: string;
    title?: string;
    children: Snippet;
  }

  let { tone = 'plain', class: className = '', title, children }: Props = $props();

  const tones: Record<Tone, string> = {
    ok: 'bg-[var(--color-ok-bg)] text-[var(--color-ok)]',
    warn: 'bg-[var(--color-warn-bg)] text-[var(--color-warn)]',
    danger: 'bg-[var(--color-danger-bg)] text-[var(--color-danger)]',
    info: 'bg-[var(--color-info-bg)] text-[var(--color-info)]',
    plain: 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)]',
  };
</script>

<span
  {title}
  class={cn(
    'inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium leading-relaxed whitespace-nowrap',
    tones[tone],
    className,
  )}
>
  {@render children()}
</span>
