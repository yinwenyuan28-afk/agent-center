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
  import Git from './pages/Git.svelte';
  import TitleBar from './lib/components/TitleBar.svelte';
  import { isNative } from './lib/ipc';
  import { cn } from './lib/utils';

  type Page = 'mappings' | 'skills' | 'store' | 'providers' | 'claude-codex' | 'mcp' | 'git';

  const pages: { id: Page; label: string; hint: string }[] = [
    { id: 'mappings', label: 'AGENTS文档映射', hint: 'AGENTS.md' },
    { id: 'skills', label: 'Skills', hint: '中心库' },
    { id: 'store', label: 'Skill 更新', hint: '第三方' },
    // Provider 和 MCP 同一种模型：一个池子 + 逐条分配
    { id: 'providers', label: 'Provider', hint: '池 + 分配' },
    // 这两家有配置文件里有、但工具本身没提供界面的开关
    { id: 'claude-codex', label: 'Claude & Codex', hint: '专属设置' },
    { id: 'mcp', label: 'MCP', hint: '池 + 分配' },
    { id: 'git', label: 'Git', hint: '版本管理' },
  ];

  let current = $state<Page>('mappings');
</script>

<div class="flex h-full flex-col">
  <!-- 自绘标题栏。浏览器预览时组件内部判断 isNative，不渲染 -->
  <TitleBar />

  <div class="flex min-h-0 flex-1">
    <!-- 侧栏 -->
    <nav
      class="flex w-[210px] shrink-0 flex-col gap-1 border-r border-[var(--color-border)] bg-[var(--color-bg-subtle)] p-3"
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
        <button
          aria-current={current === page.id ? 'page' : undefined}
          onclick={() => (current = page.id)}
          class={cn(
            'flex w-full items-center justify-between gap-2 rounded-[var(--radius-md)] px-2.5 py-1.5 text-left text-[13px] transition-colors',
            current === page.id
              ? 'bg-[var(--color-bg-muted)] font-medium text-[var(--color-text)]'
              : 'text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]',
          )}
        >
          <span class="whitespace-nowrap">{page.label}</span>
          <span class="text-[10px] text-[var(--color-text-faint)]">{page.hint}</span>
        </button>
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
        <Providers />
      {:else if current === 'claude-codex'}
        <ClaudeCodex />
      {:else if current === 'mcp'}
        <Mcp />
      {:else}
        <Git />
      {/if}
    </main>
  </div>
</div>
