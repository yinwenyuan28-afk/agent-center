<script lang="ts">
  /**
   * 开关（switch）。
   *
   * 为什么不用 `<input type="checkbox">`：勾选框在视觉上表达「选中了某一项」，
   * 适合列表里的多选；而这里表达的是「一个功能开着还是关着」，开关更贴切。
   *
   * 无障碍上用 `role="switch"` + `aria-checked`，而不是 checkbox 的 `checked`。
   * 屏幕阅读器会读成「开关，已开启」而不是「复选框，已勾选」。
   *
   * 用法：
   *   <Switch checked={on} onchange={(v) => (on = v)} label="跳过首次引导" />
   * 带文字标签时用周围的文字，不给 label 就只渲染开关本身。
   */
  interface Props {
    checked: boolean;
    onchange: (value: boolean) => void;
    /** 无障碍名称。周围没有文字标签时必须给 */
    label?: string;
    disabled?: boolean;
    /** 小一号，用于紧凑的行内场景 */
    small?: boolean;
  }

  let { checked, onchange, label, disabled = false, small = false }: Props = $props();

  /** 轨道尺寸随 small 变。 */
  const track = $derived(small ? 'h-4 w-7' : 'h-5 w-9');
  const knob = $derived(small ? 'size-3' : 'size-4');
  const shift = $derived(small ? 'translate-x-3' : 'translate-x-4');

  function toggle() {
    if (disabled) return;
    onchange(!checked);
  }
</script>

<!--
  用 button 而不是 label+input：整块可点，且不需要处理 label 关联。
  aria-checked 表状态，disabled 时 aria-disabled 让读屏也能感知。
-->
<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  aria-disabled={disabled}
  {disabled}
  onclick={toggle}
  class="relative inline-flex shrink-0 cursor-pointer items-center rounded-full border-0 p-0 transition-colors
    {track}
    {checked ? 'bg-[var(--color-accent)]' : 'bg-[var(--color-border-strong)]'}
    {disabled ? 'cursor-not-allowed opacity-50' : 'hover:opacity-90'}"
>
  <span
    class="pointer-events-none block rounded-full bg-white shadow-sm transition-transform
      {knob}
      {checked ? shift : 'translate-x-0.5'}"
  ></span>
</button>
