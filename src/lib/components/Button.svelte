<script lang="ts">
  /**
   * 按钮。
   *
   * 变体使用项目的黑白灰配色：主按钮通过主题变量自动适配浅色和深色背景。
   * 没有花哨的渐变和阴影，靠边框和底色区分层级。
   */
  import type { Snippet } from 'svelte';
  import { cn } from '../utils';

  type Variant = 'default' | 'primary' | 'ghost' | 'outline' | 'danger';
  type Size = 'sm' | 'md';

  interface Props {
    variant?: Variant;
    size?: Size;
    disabled?: boolean;
    type?: 'button' | 'submit';
    class?: string;
    /** 悬停提示。用来说明点下去会发生什么，尤其是会动磁盘的操作 */
    title?: string;
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  }

  let {
    variant = 'outline',
    size = 'md',
    disabled = false,
    type = 'button',
    class: className = '',
    title,
    onclick,
    children,
  }: Props = $props();

  // 每个变体一组类名。用对象而不是长三元表达式，
  // 加新变体时只改一处。
  const variants: Record<Variant, string> = {
    default: 'bg-[var(--color-bg)] text-[var(--color-text)] border border-[var(--color-border-strong)] hover:bg-[var(--color-bg-hover)]',
    primary: 'bg-[var(--color-accent)] text-[var(--color-accent-text)] border border-transparent hover:bg-[var(--color-accent-hover)]',
    ghost: 'bg-transparent text-[var(--color-text-muted)] border border-transparent hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]',
    outline: 'bg-transparent text-[var(--color-text)] border border-[var(--color-border-strong)] hover:bg-[var(--color-bg-hover)]',
    danger: 'bg-transparent text-[var(--color-danger)] border border-[var(--color-danger)] hover:bg-[var(--color-danger-bg)]',
  };

  const sizes: Record<Size, string> = {
    sm: 'h-7 px-2.5 text-xs rounded-[var(--radius-sm)]',
    md: 'h-9 px-3.5 text-sm rounded-[var(--radius-md)]',
  };
</script>

<button
  {type}
  {disabled}
  {title}
  onclick={onclick}
  class={cn(
    'inline-flex items-center justify-center gap-1.5 whitespace-nowrap',
    'font-medium transition-colors select-none',
    'disabled:opacity-40 disabled:pointer-events-none',
    variants[variant],
    sizes[size],
    className,
  )}
>
  {@render children()}
</button>
