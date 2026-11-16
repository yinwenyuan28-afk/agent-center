<script lang="ts">
  /**
   * 应用外壳：左侧导航 + 右侧页面。
   *
   * 视觉向 ChatGPT 靠：侧栏用略深的底色，选中项是圆角高亮块而不是左侧竖条。
   * 导航不用路由库，一个 $state 记当前页面就够——四个页面、无深层链接需求。
   */
  import Mappings from './pages/Mappings.svelte';
  import Skills from './pages/Skills.svelte';
  import Store from './pages/Store.svelte';
  import Providers from './pages/Providers.svelte';
  import ClaudeCodex from './pages/ClaudeCodex.svelte';
  import Mcp from './pages/Mcp.svelte';
  import Usage from './pages/Usage.svelte';
  import Git from './pages/Git.svelte';
  import TitleBar from './lib/components/TitleBar.svelte';
  import { ChevronRight } from '@lucide/svelte';
  import { isNative } from './lib/ipc';
  import { cn } from './lib/utils';

  type Page =
    | 'mappings'
    | 'skills'
    | 'store'
    | 'providers'
    | 'providers-enable'
    | 'claude-codex'
    | 'mcp'
    | 'usage'
    | 'git';

  /**
   * 侧栏条目。带 `children` 的是可展开的二级菜单。
   *
   * Provider 拆成两页而不是页内切换：两件事的分工本来就不一样
   * （管池子 vs 管分配），放进侧栏比藏在页面里更容易发现。
   */
  const pages: {
    id: Page;
    label: string;
    hint: string;
    children?: { id: Page; label: string }[];
  }[] = [
    { id: 'mappings', label: 'AGENTS文档映射', hint: 'AGENTS.md' },
    { id: 'skills', label: 'Skills', hint: '中心库' },
    { id: 'store', label: 'Skills 更新', hint: '第三方' },
    // Provider 一项两页：管池子、管分配
    {
      id: 'providers',
      label: 'Provider',
      hint: '',
      children: [
        { id: 'providers', label: 'provider 管理' },
        { id: 'providers-enable', label: '启用 provider' },
      ],
    },
    // 这两家有配置文件里有、但工具本身没提供界面的开关
    { id: 'claude-codex', label: 'Claude & Codex', hint: '专属设置' },
    { id: 'mcp', label: 'MCP', hint: '池 + 分配' },
    // 用量统计：读各 agent 自己写的会话文件，默认不读
    { id: 'usage', label: 'Token 用量', hint: '按需读取' },
    { id: 'git', label: 'Git', hint: '版本管理' },
  ];

  let current = $state<Page>('mappings');

  /** 哪些分组是展开的。Provider 默认展开，因为它是常用入口。 */
  let expanded = $state<Set<string>>(new Set(['providers']));

  /** 某个分组下有没有子项被选中（决定父项是否高亮）。 */
  function groupActive(children: { id: Page }[] | undefined): boolean {
    return !!children?.some((c) => c.id === current);
  }

  function toggleGroup(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
      // 展开时如果这个分组下没有任何子项被选中，直接落到第一个子项上，
      // 否则用户点了展开却看不到对应内容，还得再点一次
      const group = pages.find((p) => p.id === id);
      const kids = group?.children;
      if (kids && kids.length > 0 && !kids.some((c) => c.id === current)) {
        current = kids[0].id;
      }
    }
    expanded = next;
  }
</script>

<div class="flex h-full flex-col">
  <!-- 自绘标题栏。浏览器预览时组件内部判断 isNative，不渲染 -->
  <TitleBar />

  <div class="flex min-h-0 flex-1">
    <!-- 侧栏 -->
    <nav
      class="flex w-[224px] shrink-0 flex-col gap-1 border-r border-[var(--color-border)] bg-[var(--color-bg-subtle)] p-3"
    >
      <!-- 标题栏里已经有 agentcenter 了，这里不再重复，
           只在浏览器预览时保留一个标识 -->
      {#if !isNative}
        <div class="px-2 pb-3 pt-1">
          <div class="text-[15px] font-semibold tracking-tight text-[var(--color-text)]">
            agentcenter
          </div>
          <div
            class="mt-1 inline-flex items-center rounded-[var(--radius-sm)] bg-[var(--color-warn-bg)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-warn)]"
            title="当前是浏览器预览，所有数据都是假的。要看真实数据请运行 exe"
          >
            假数据预览
          </div>
        </div>
      {:else}
        <div class="pb-2"></div>
      {/if}

      {#each pages as page (page.id)}
        {#if page.children}
          {@const active = groupActive(page.children)}
          {@const open = expanded.has(page.id)}
          <!-- 有子项的分组：点这一行只展开/收起，不切页。
               箭头在左侧，和子项各自的指示符对齐 -->
          <button
            aria-expanded={open}
            onclick={() => toggleGroup(page.id)}
            class={cn(
              'flex w-full items-center gap-1.5 rounded-[var(--radius-sm)] px-1.5 py-1.5 text-left text-[13px] transition-colors',
              active
                ? 'font-medium text-[var(--color-text)]'
                : 'text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]',
            )}
          >
            <!-- 图标宽度锁 13px，和叶子节点的圆点同宽，文字起点才对得齐 -->
            <span class="flex w-[13px] shrink-0 items-center justify-center">
              <ChevronRight
                size={13}
                class="text-[var(--color-text-faint)] transition-transform {open ? 'rotate-90' : ''}"
              />
            </span>
            <span class="whitespace-nowrap">{page.label}</span>
          </button>

          {#if open}
            <!-- 子项区：左边一条竖线引导。
                 图标按「是不是叶子」定，不看选中状态：
                 这里的子项都是叶子，所以一律圆点。
                 子项用圆角块 + 互相留缝，避免选中态变成一条直角长条、
                 也避免上下两项的背景粘在一起。 -->
            <div class="relative flex flex-col gap-0.5">
              <div class="absolute bottom-1 left-[13px] top-0 w-px bg-[var(--color-border)]"></div>
              {#each page.children as child (child.id)}
                {@const on = current === child.id}
                <button
                  aria-current={on ? 'page' : undefined}
                  onclick={() => (current = child.id)}
                  class={cn(
                    'ml-[12px] flex items-center gap-1.5 rounded-[var(--radius-sm)] py-1.5 pl-2 pr-2 text-left text-[12.5px] transition-colors',
                    on
                      ? 'bg-[var(--color-bg-muted)] font-medium text-[var(--color-text)]'
                      : 'text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]',
                  )}
                >
                  <span
                    class="w-[13px] shrink-0 text-center text-[15px] leading-none {on
                      ? 'text-[var(--color-text)]'
                      : 'text-[var(--color-text-faint)]'}"
                    aria-hidden="true"
                  >
                    ·
                  </span>
                  <span class="whitespace-nowrap">{child.label}</span>
                </button>
              {/each}
            </div>
          {/if}
        {:else}
          <!-- 顶级叶子节点：也带圆点，和上面的分组箭头占同样宽度，
               文字起点才能对齐 -->
          <button
            aria-current={current === page.id ? 'page' : undefined}
            onclick={() => (current = page.id)}
            class={cn(
              'flex w-full items-center justify-between gap-2 rounded-[var(--radius-sm)] px-1.5 py-1.5 text-left text-[13px] transition-colors',
              current === page.id
                ? 'bg-[var(--color-bg-muted)] font-medium text-[var(--color-text)]'
                : 'text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]',
            )}
          >
            <span class="flex min-w-0 items-center gap-1.5">
              <span
                class="w-[13px] shrink-0 text-center text-[15px] leading-none {current === page.id
                  ? 'text-[var(--color-text)]'
                  : 'text-[var(--color-text-faint)]'}"
                aria-hidden="true"
              >
                ·
              </span>
              <span class="truncate">{page.label}</span>
            </span>
            <span class="shrink-0 text-[10px] text-[var(--color-text-faint)]">{page.hint}</span>
          </button>
        {/if}
      {/each}

      <div class="mt-auto px-2 pt-4 text-[10px] leading-relaxed text-[var(--color-text-faint)]">
        ~/.agents 为中心库
      </div>
    </nav>

    <!-- 内容区 -->
    <main class="min-w-0 flex-1 overflow-y-auto px-8 py-7">
      {#if current === 'mappings'}
        <Mappings />
      {:else if current === 'skills'}
        <Skills />
      {:else if current === 'store'}
        <Store />
      {:else if current === 'providers'}
        <!-- 给 Claude / Codex 的特殊配置指路：那些开关在专属设置页 -->
        <Providers view="provider" onGoClaudeCodex={() => (current = 'claude-codex')} />
      {:else if current === 'providers-enable'}
        <Providers
          view="agent"
          onGoProviderPool={() => (current = 'providers')}
          onGoClaudeCodex={() => (current = 'claude-codex')}
        />
      {:else if current === 'claude-codex'}
        <ClaudeCodex />
      {:else if current === 'mcp'}
        <Mcp />
      {:else if current === 'usage'}
        <Usage />
      {:else}
        <Git />
      {/if}
    </main>
  </div>
</div>
