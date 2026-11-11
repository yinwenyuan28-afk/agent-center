<script lang="ts">
  /**
   * MCP 管理页。
   *
   * 默认只展示 server 列表，每行末尾用小标签表示已分配到哪些 agent。
   * 点某一行的「分配」才展开该 server 的 agent 选择面板——
   * 不做成常驻矩阵，避免满屏勾选框。
   *
   * 分配面板用卡片式按钮代替 checkbox：点整块切换，选中是实心高亮 + 对勾。
   * 比小方框更容易点，视觉上也更像「选人」而不是「填表」。
   */
  import { onMount } from 'svelte';
  import {
    RefreshCw,
    Download,
    Play,
    Plus,
    Pencil,
    Trash2,
    Check,
    ChevronDown,
    ChevronRight,
    Server,
    Link2,
  } from '@lucide/svelte';
  import {
    deleteMcpServer,
    importMcpFromAgents,
    listMcpServers,
    mcpMatrix,
    saveMcpServer,
    setMcpRow,
    syncMcp,
  } from '../lib/ipc';
  import type { McpMatrix, McpServerView } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';

  let matrix = $state<McpMatrix | null>(null);
  let servers = $state<McpServerView[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');

  /** 当前展开分配面板的 server id。null 表示全部收起。 */
  let expanded = $state<string | null>(null);

  // 编辑表单
  let editing = $state<McpServerView | null>(null);
  let isNew = $state(false);
  let form = $state({
    id: '',
    name: '',
    transport: 'stdio',
    command: '',
    args: '',
    cwd: '',
    url: '',
    env: '',
    headers: '',
    enabled: true,
    agents: '',
    note: '',
  });

  async function load() {
    loading = true;
    try {
      matrix = await mcpMatrix();
      servers = await listMcpServers();
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  /** 某个 server 当前的分配列表。 */
  function assignedOf(id: string): string[] {
    return matrix?.rows.find((r) => r.id === id)?.assigned ?? [];
  }

  /** 切换某个 server 在某个 agent 的分配。乐观更新，失败回滚。 */
  async function toggleAgent(serverId: string, agent: string) {
    const row = matrix?.rows.find((r) => r.id === serverId);
    if (!row) return;

    const on = !row.assigned.includes(agent);
    const prev = [...row.assigned];
    const next = on ? [...row.assigned, agent] : row.assigned.filter((a) => a !== agent);

    row.assigned = next;
    // 同步给列表，让行内标签立即更新
    servers = servers.map((s) => (s.id === serverId ? { ...s, agents: next } : s));

    try {
      const r = await setMcpRow(serverId, next);
      if (!r.ok) throw new Error(r.output);
    } catch (e) {
      row.assigned = prev;
      servers = servers.map((s) => (s.id === serverId ? { ...s, agents: prev } : s));
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** 全选 / 全不选某个 server 的所有可用 agent。 */
  async function toggleAllAgents(serverId: string) {
    if (!matrix) return;
    const row = matrix.rows.find((r) => r.id === serverId);
    if (!row) return;

    const available = matrix.columns.filter((c) => c.available).map((c) => c.agent);
    const allOn = available.every((a) => row.assigned.includes(a));
    const next = allOn ? [] : [...available];

    const prev = [...row.assigned];
    row.assigned = next;
    servers = servers.map((s) => (s.id === serverId ? { ...s, agents: next } : s));

    try {
      const r = await setMcpRow(serverId, next);
      if (!r.ok) throw new Error(r.output);
    } catch (e) {
      row.assigned = prev;
      servers = servers.map((s) => (s.id === serverId ? { ...s, agents: prev } : s));
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function apply() {
    busy = true;
    try {
      const r = await syncMcp(false);
      log = r.log.join('\n');
    } catch (e) {
      log = `应用失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function doImport() {
    busy = true;
    try {
      const r = await importMcpFromAgents();
      log = r.output;
      await load();
    } catch (e) {
      log = `导入失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  // ---- 编辑表单 ----
  function startNew() {
    isNew = true;
    editing = null;
    form = {
      id: '',
      name: '',
      transport: 'stdio',
      command: '',
      args: '',
      cwd: '',
      url: '',
      env: '',
      headers: '',
      enabled: true,
      agents: '',
      note: '',
    };
  }

  function startEdit(id: string) {
    const s = servers.find((x) => x.id === id);
    if (!s) return;
    isNew = false;
    editing = s;
    form = {
      id: s.id,
      name: s.name,
      transport: s.transport,
      command: s.command,
      args: s.args.join(' '),
      cwd: s.cwd,
      url: s.url,
      env: s.envKeys.map((k) => `${k}=`).join('\n'),
      headers: s.headerKeys.map((k) => `${k}=`).join('\n'),
      enabled: s.enabled,
      agents: s.agents.join(', '),
      note: s.note,
    };
  }

  function cancelEdit() {
    editing = null;
    isNew = false;
  }

  function parseKV(text: string): Record<string, string> {
    const out: Record<string, string> = {};
    for (const line of text.split('\n')) {
      const t = line.trim();
      if (!t || !t.includes('=')) continue;
      const idx = t.indexOf('=');
      out[t.slice(0, idx).trim()] = t.slice(idx + 1).trim();
    }
    return out;
  }

  async function submit() {
    if (!form.id.trim()) {
      log = 'id 不能为空';
      return;
    }
    if (form.transport === 'stdio' && !form.command.trim()) {
      log = 'stdio 型必须填 command';
      return;
    }
    if (form.transport !== 'stdio' && !form.url.trim()) {
      log = '远程型必须填 url';
      return;
    }
    busy = true;
    try {
      const r = await saveMcpServer({
        id: form.id.trim(),
        name: form.name.trim(),
        transport: form.transport,
        command: form.command.trim(),
        args: form.args.trim() ? form.args.trim().split(/\s+/) : [],
        cwd: form.cwd.trim(),
        url: form.url.trim(),
        env: parseKV(form.env),
        headers: parseKV(form.headers),
        enabled: form.enabled,
        agents: form.agents
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        note: form.note.trim(),
      });
      log = r.output;
      if (r.ok) {
        editing = null;
        isNew = false;
        await load();
      }
    } catch (e) {
      log = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function remove(id: string, name: string) {
    if (!confirm(`从池子里删除「${name}」？\n\n这只删母版。各 agent 里的配置要重新「应用」才会消失。`)) return;
    busy = true;
    try {
      const r = await deleteMcpServer(id);
      log = r.output;
      if (r.ok) await load();
    } catch (e) {
      log = `删除失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  function isAssigned(serverId: string, agent: string): boolean {
    return assignedOf(serverId).includes(agent);
  }

  onMount(load);
</script>

<PageHead title="MCP">
  所有 MCP server 在一个池子里。点某一行的
  <strong class="font-medium text-[var(--color-text)]">「分配」</strong>
  展开，选择这个 server 要用在哪些 agent 上。
  <br />
  <span class="text-[var(--color-text-muted)]">
    改动先写母版，点「应用到各 agent」才真正写入配置文件。取消勾选的会从该 agent 里移除。
  </span>
</PageHead>

<div class="mb-4 flex flex-wrap items-center gap-2">
  <Button variant="primary" onclick={startNew} disabled={busy}>
    <Plus size={14} />
    新增 server
  </Button>
  <Button onclick={doImport} disabled={busy}>
    <Download size={14} />
    从现有配置导入
  </Button>
  <span class="flex-1"></span>
  <Button variant="ghost" onclick={load} disabled={busy}>
    <RefreshCw size={14} />
    刷新
  </Button>
  <Button variant="primary" onclick={apply} disabled={busy || loading}>
    <Play size={14} />
    {busy ? '处理中…' : '应用到各 agent'}
  </Button>
</div>

<!-- 表单 -->
{#if isNew || editing}
  <div class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-accent)] bg-[var(--color-bg-subtle)] p-4">
    <h2 class="mb-3 text-[14px] font-semibold">
      {isNew ? '新增 MCP server' : `编辑 ${editing?.name}`}
    </h2>

    <div class="grid grid-cols-2 gap-3">
      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">标识 id</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px] disabled:opacity-60"
          bind:value={form.id}
          disabled={!isNew}
          placeholder="context7"
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">展示名</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[13px]"
          bind:value={form.name}
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">传输协议</span>
        <select
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[13px]"
          bind:value={form.transport}
        >
          <option value="stdio">stdio（本地进程）</option>
          <option value="http">http（Streamable HTTP）</option>
          <option value="sse">sse（Server-Sent Events）</option>
        </select>
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">全局启用</span>
        <div class="flex h-[34px] items-center">
          <input type="checkbox" class="accent-[var(--color-accent)]" bind:checked={form.enabled} />
          <span class="ml-2 text-[12px] text-[var(--color-text-muted)]">
            {form.enabled ? '启用' : '停用'}
          </span>
        </div>
      </label>

      {#if form.transport === 'stdio'}
        <label class="block">
          <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">命令</span>
          <input
            class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
            bind:value={form.command}
            placeholder="cmd"
          />
        </label>

        <label class="block">
          <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">参数（空格分隔）</span>
          <input
            class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
            bind:value={form.args}
            placeholder="/c npx -y @upstash/context7-mcp"
          />
        </label>

        <label class="col-span-2 block">
          <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">工作目录（可选）</span>
          <input
            class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
            bind:value={form.cwd}
          />
        </label>
      {:else}
        <label class="col-span-2 block">
          <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">端点 url</span>
          <input
            class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
            bind:value={form.url}
            placeholder="https://example.com/mcp"
          />
        </label>
      {/if}

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">环境变量（每行 KEY=VALUE）</span>
        <textarea
          class="h-[64px] w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[11px]"
          bind:value={form.env}
          spellcheck="false"
        ></textarea>
        {#if editing && editing.envKeys.length > 0}
          <div class="mt-0.5 text-[10px] text-[var(--color-warn)]">
            已有 {editing.envKeys.length} 个变量，值不回显，保存会被覆盖
          </div>
        {/if}
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">请求头（每行 Name=Value）</span>
        <textarea
          class="h-[64px] w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[11px]"
          bind:value={form.headers}
          spellcheck="false"
        ></textarea>
      </label>

      <label class="col-span-2 block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">备注</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[12px]"
          bind:value={form.note}
        />
      </label>
    </div>

    <div class="mt-3 flex gap-2">
      <Button variant="primary" onclick={submit} disabled={busy}>
        {busy ? '保存中…' : '保存'}
      </Button>
      <Button variant="ghost" onclick={cancelEdit} disabled={busy}>取消</Button>
    </div>
  </div>
{/if}

<!-- server 列表 -->
{#if loading}
  <div class="flex flex-col gap-2">
    {#each Array(4) as _}
      <div class="h-16 animate-pulse rounded-[var(--radius-lg)] bg-[var(--color-bg-muted)]"></div>
    {/each}
  </div>
{:else if !matrix || matrix.empty}
  <div
    class="rounded-[var(--radius-lg)] border border-dashed border-[var(--color-border-strong)] py-12 text-center text-[13px] text-[var(--color-text-faint)]"
  >
    池子里还没有 MCP server。<br />
    点「从现有配置导入」把你各 agent 里已有的 MCP 拉进来，或「新增 server」手工添加。
  </div>
{:else}
  <div class="flex flex-col gap-2">
    {#each matrix.rows as row (row.id)}
      {@const isOpen = expanded === row.id}
      <div
        class="overflow-hidden rounded-[var(--radius-lg)] border transition-colors {isOpen
          ? 'border-[var(--color-accent)]'
          : 'border-[var(--color-border)]'}"
      >
        <!-- 行主体 -->
        <div class="flex items-center gap-3 px-4 py-3">
          <!-- 展开箭头 -->
          <button
            class="flex h-6 w-6 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--color-text-faint)] transition-colors hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
            aria-label={isOpen ? '收起分配' : '展开分配'}
            onclick={() => (expanded = isOpen ? null : row.id)}
          >
            {#if isOpen}
              <ChevronDown size={14} />
            {:else}
              <ChevronRight size={14} />
            {/if}
          </button>

          <!-- 类型图标 -->
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-bg-muted)]"
          >
            {#if row.transport === 'stdio'}
              <Server size={15} class="text-[var(--color-text-muted)]" />
            {:else}
              <Link2 size={15} class="text-[var(--color-text-muted)]" />
            {/if}
          </div>

          <!-- 主体 -->
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="font-[var(--font-mono)] text-[13px] font-medium">{row.id}</span>
              <Badge tone={row.transport === 'stdio' ? 'plain' : 'info'}>{row.transport}</Badge>
              {#if !row.enabled}
                <Badge tone="warn">全局停用</Badge>
              {/if}
            </div>
            <div
              class="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
              title={row.target}
            >
              {row.target}
            </div>
          </div>

          <!-- 已分配标签 -->
          <div class="hidden shrink-0 items-center gap-1 md:flex">
            {#if row.assigned.length === 0}
              <span class="text-[11px] text-[var(--color-text-faint)]">未分配</span>
            {:else}
              {#each row.assigned as a (a)}
                <span
                  class="rounded-full bg-[var(--color-accent)]/10 px-2 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-accent)]"
                >
                  {a}
                </span>
              {/each}
            {/if}
          </div>

          <!-- 操作 -->
          <div class="flex shrink-0 items-center gap-1">
            <button
              class="rounded-[var(--radius-sm)] px-2.5 py-1 text-[11px] font-medium transition-colors {isOpen
                ? 'bg-[var(--color-accent)] text-white'
                : 'border border-[var(--color-border-strong)] text-[var(--color-text-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]'}"
              onclick={() => (expanded = isOpen ? null : row.id)}
            >
              {isOpen ? '收起' : '分配'}
            </button>
            <button
              class="rounded p-1 text-[var(--color-text-faint)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
              title="编辑"
              aria-label={`编辑 ${row.id}`}
              onclick={() => startEdit(row.id)}
            >
              <Pencil size={13} />
            </button>
            <button
              class="rounded p-1 text-[var(--color-text-faint)] hover:bg-[var(--color-danger-bg)] hover:text-[var(--color-danger)]"
              title="删除"
              aria-label={`删除 ${row.id}`}
              onclick={() => remove(row.id, row.name)}
            >
              <Trash2 size={13} />
            </button>
          </div>
        </div>

        <!-- 分配面板 -->
        {#if isOpen && matrix}
          <div class="border-t border-[var(--color-border)] bg-[var(--color-bg-subtle)] px-4 py-4">
            <div class="mb-3 flex items-center gap-2">
              <span class="text-[12px] font-medium text-[var(--color-text)]">
                把这个 server 注入到
              </span>
              <span class="flex-1"></span>
              <button
                class="text-[11px] text-[var(--color-text-muted)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
                onclick={() => toggleAllAgents(row.id)}
              >
                {matrix.columns
                  .filter((c) => c.available)
                  .every((c) => row.assigned.includes(c.agent))
                  ? '全不选'
                  : '全选'}
              </button>
            </div>

            <!-- agent 卡片：点整块切换 -->
            <div class="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
              {#each matrix.columns as col (col.agent)}
                {@const on = isAssigned(row.id, col.agent)}
                <button
                  class="flex items-center gap-2.5 rounded-[var(--radius-md)] border px-3 py-2.5 text-left transition-all {!col.available
                    ? 'cursor-not-allowed border-[var(--color-border)] opacity-40'
                    : on
                      ? 'border-[var(--color-accent)] bg-[var(--color-accent)] text-white shadow-sm'
                      : 'border-[var(--color-border-strong)] bg-[var(--color-bg)] text-[var(--color-text)] hover:border-[var(--color-accent)] hover:bg-[var(--color-bg-hover)]'}"
                  disabled={!col.available || busy}
                  title={col.available ? `切换「${row.id}」在 ${col.label}` : `${col.label} 不可用`}
                  aria-pressed={on}
                  aria-label={`${row.id} 在 ${col.label} ${on ? '已启用' : '未启用'}`}
                  onclick={() => toggleAgent(row.id, col.agent)}
                >
                  <span
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border transition-colors {on
                      ? 'border-white/60 bg-white/20'
                      : 'border-[var(--color-border-strong)]'}"
                  >
                    {#if on}
                      <Check size={11} class="text-white" />
                    {/if}
                  </span>

                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[12px] font-medium">{col.label}</span>
                    {#if !col.available}
                      <span class="block text-[10px] opacity-70">未安装</span>
                    {:else if on}
                      <span class="block text-[10px] opacity-80">已启用</span>
                    {:else}
                      <span class="block text-[10px] text-[var(--color-text-faint)]">未启用</span>
                    {/if}
                  </span>
                </button>
              {/each}
            </div>

            <div class="mt-3 text-[10px] text-[var(--color-text-faint)]">
              改动已写入母版。点右上角「应用到各 agent」才会真正写进配置文件。
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <div class="mt-2 text-[11px] text-[var(--color-text-faint)]">
    共 {matrix.rows.length} 个 server，{matrix.columns.filter((c) => c.available).length} 个可用 agent。
  </div>
{/if}

<!-- 边界说明 -->
<div class="mt-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-3">
  <div class="text-[11px] leading-relaxed text-[var(--color-text-muted)]">
    <strong class="text-[var(--color-text)]">只移除池子里的 server。</strong>
    各 agent 里用户手动配的、WorkBuddy 的市场连接器，都不在池子里，应用时不会被碰。
    <br />
    <span class="text-[var(--color-text-faint)]">
      这里的列由各 agent 的实现决定。某个 agent 没有出现，说明它没有声明 MCP 能力。
    </span>
  </div>
</div>

{#if log}
  <div class="mt-4">
    <Log text={log} maxHeight="320px" />
  </div>
{/if}
