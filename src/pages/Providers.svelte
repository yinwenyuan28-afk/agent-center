<script lang="ts">
  /**
   * Provider 管理页。
   *
   * 和 MCP 页同一套交互：上面是 provider 池，默认只列清单，
   * 点某一行的「分配」才展开该 provider 的 agent 选择面板。
   *
   * 分配关系直接写母版（`~/.agents/providers.toml` 的 `agents` 字段），
   * 点右上角「应用到各 agent」才真正改各家的配置文件。
   *
   * 安全设计：key 编辑框留空表示不修改，避免误清空；
   * 界面上只显示 key 长度和末尾 4 位，原文不跨进程边界。
   */
  import { onMount } from 'svelte';
  import {
    Plus,
    Trash2,
    Pencil,
    Check,
    ChevronDown,
    ChevronRight,
    RefreshCw,
    Play,
    KeyRound,
    TriangleAlert,
  } from '@lucide/svelte';
  import {
    deleteProvider,
    listProviders,
    providerMatrix,
    readProvidersToml,
    backfillModels,
    saveProvider,
    setProviderRow,
    syncProviders,
    writeProvidersToml,
  } from '../lib/ipc';
  import type { ModelSpec, ProviderMatrix, ProviderView } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';

  let matrix = $state<ProviderMatrix | null>(null);
  let providers = $state<ProviderView[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');

  /** 当前展开分配面板的 provider id。null 表示全部收起。 */
  let expanded = $state<string | null>(null);

  // 编辑表单
  let editing = $state<ProviderView | null>(null);
  let isNew = $state(false);
  let form = $state({
    id: '',
    name: '',
    baseUrl: '',
    kind: 'anthropic',
    apiKey: '',
    models: [] as ModelSpec[],
    /** 按 agent 覆盖的协议。只放和 kind 不同的那些 */
    agentKinds: {} as Record<string, string>,
    note: '',
  });

  /** 当前展开元数据编辑的那张模型卡片。键是 provider id + 模型 id。 */
  let openModel = $state<string | null>(null);

  /** 空模型：只填了 id 的状态。 */
  function blankModel(id = ''): ModelSpec {
    return {
      id,
      inputModalities: [],
      outputModalities: [],
      reasoningEfforts: [],
    };
  }

  /** 折叠态的一行能力摘要。 */
  function modelSummary(m: ModelSpec): string {
    const bits: string[] = [];
    if (m.contextWindow) bits.push(`${fmtK(m.contextWindow)} ctx`);
    if (m.maxOutputTokens) bits.push(`${fmtK(m.maxOutputTokens)} out`);
    if (m.inputModalities.length > 0) bits.push(m.inputModalities.join('+'));
    if (m.reasoning) bits.push('reasoning');
    return bits.length > 0 ? bits.join(' · ') : '未填元数据';
  }

  /** 把 token 数压成 1M / 128K 这种短写法。 */
  function fmtK(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
    if (n >= 1000) return `${Math.round(n / 1000)}K`;
    return String(n);
  }

  /** 切换模型上的一个模态。 */
  function toggleModality(m: ModelSpec, list: 'inputModalities' | 'outputModalities', value: string) {
    const arr = m[list];
    const i = arr.indexOf(value);
    if (i >= 0) arr.splice(i, 1);
    else arr.push(value);
  }

  /** 切换模型上的一个推理档位。 */
  function toggleEffort(m: ModelSpec, value: string) {
    const i = m.reasoningEfforts.indexOf(value);
    if (i >= 0) m.reasoningEfforts.splice(i, 1);
    else m.reasoningEfforts.push(value);
  }

  /** 从各 agent 现有配置回填模型元数据。 */
  async function doBackfill() {
    busy = true;
    try {
      const r = await backfillModels(false);
      if (r.lines.length === 0) {
        log = '没有可回填的字段（母版里的模型元数据都已齐全，或各 agent 配置里读不到）。';
      } else {
        const lines = r.lines.map(
          (l) => `${l.provider} / ${l.model}  ← ${l.source}\n    ${l.filled.join(', ')}`,
        );
        log = `${lines.join('\n')}\n\n共补 ${r.filledFields} 个字段${r.wrote ? '，已写入母版' : ''}。`;
      }
      await load();
    } catch (e) {
      log = `回填失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  // 高级：直接编辑 TOML
  let showRaw = $state(false);
  let rawToml = $state('');

  /** 协议选项。value 与后端 ProviderKind::parse 对应。 */
  const KIND_OPTIONS = [
    { value: 'anthropic', label: 'Anthropic Messages' },
    { value: 'openai-chat', label: 'OpenAI Chat Completions' },
    { value: 'openai-responses', label: 'OpenAI Responses' },
    { value: 'google-generative-ai', label: 'Google Generative AI' },
  ];

  async function load() {
    loading = true;
    try {
      // 加超时保护：IPC 万一挂住（后端死锁或命令没注册），
      // 界面会一直停在骨架屏而且没有任何提示。宁可报错也不要无限等待。
      const withTimeout = <T,>(p: Promise<T>, label: string, ms = 8000): Promise<T> =>
        Promise.race([
          p,
          new Promise<T>((_, reject) =>
            setTimeout(() => reject(new Error(`${label} 超时（${ms}ms 无响应）`)), ms),
          ),
        ]);

      matrix = await withTimeout(providerMatrix(), '读取分配矩阵');
      providers = await withTimeout(listProviders(), '读取 provider 列表');
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
      console.error('[Providers] load failed', e);
    } finally {
      loading = false;
    }
  }

  /** 某个 provider 当前的分配列表。 */
  function assignedOf(id: string): string[] {
    return matrix?.providers.find((p) => p.id === id)?.agents ?? [];
  }

  /** 把 agent id 换成展示名。查不到就原样返回。 */
  function labelOf(agent: string): string {
    return matrix?.columns.find((c) => c.agent === agent)?.label ?? agent;
  }

  /** 把协议标识换成展示名。 */
  function kindLabelOf(kind: string): string {
    return KIND_OPTIONS.find((k) => k.value === kind)?.label ?? kind;
  }

  /**
   * 某个 agent 能选的协议。
   *
   * 后端 `supportedKinds` 是空表示「全部支持」；不空就只列它真能表达的。
   * 不这么做的话，Codex 的下拉里会出现 anthropic —— 选了会被注入器
   * 强制改写成 responses，等于骗用户。
   */
  function kindsForAgent(agent: string) {
    const allowed = matrix?.columns.find((c) => c.agent === agent)?.supportedKinds ?? [];
    if (allowed.length === 0) return KIND_OPTIONS;
    return KIND_OPTIONS.filter((k) => allowed.includes(k.value));
  }

  function isAssigned(providerId: string, agent: string): boolean {
    return assignedOf(providerId).includes(agent);
  }

  /**
   * 切换某个 provider 在某个 agent 的分配。
   *
   * 乐观更新，失败回滚。Claude 是单槽位，勾上它会自动取消别的
   * provider 上的 claude —— 所以成功后重新拉一次矩阵，别自己猜。
   */
  async function toggleAgent(providerId: string, agent: string) {
    const row = matrix?.providers.find((p) => p.id === providerId);
    if (!row) return;

    const on = !row.agents.includes(agent);
    let next = on ? [...row.agents, agent] : row.agents.filter((a) => a !== agent);

    // 单槽位 agent（Claude）：本地先做互斥，界面不会出现两条同时高亮。
    // 哪些 agent 是单槽位由后端 `col.singleSlot` 声明，前端不写死名字。
    const slot = matrix?.columns.find((c) => c.agent === agent);
    if (on && slot?.singleSlot && matrix) {
      for (const other of matrix.providers) {
        if (other.id !== providerId) {
          other.agents = other.agents.filter((a) => a !== agent);
        }
      }
      providers = providers.map((p) =>
        p.id === providerId ? p : { ...p, agents: p.agents.filter((a) => a !== agent) },
      );
    }

    const prev = [...row.agents];
    row.agents = next;
    providers = providers.map((p) => (p.id === providerId ? { ...p, agents: next } : p));

    try {
      const r = await setProviderRow(providerId, next);
      if (!r.ok) throw new Error(r.output);
    } catch (e) {
      row.agents = prev;
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
      await load();
    }
  }

  /** 全选 / 全不选所有可用 agent。Claude 仍只保留这一个 provider。 */
  async function toggleAllAgents(providerId: string) {
    if (!matrix) return;
    const row = matrix.providers.find((p) => p.id === providerId);
    if (!row) return;

    const available = matrix.columns.filter((c) => c.available).map((c) => c.agent);
    const allOn = available.every((a) => row.agents.includes(a));
    const next = allOn ? [] : [...available];

    // 全选时如果包含单槽位 agent，其他 provider 上的同一个 agent 也要摘掉
    if (!allOn) {
      const slots = matrix.columns
        .filter((c) => c.singleSlot)
        .map((c) => c.agent)
        .filter((a) => next.includes(a));
      if (slots.length > 0) {
        for (const other of matrix.providers) {
          if (other.id !== providerId) {
            other.agents = other.agents.filter((a) => !slots.includes(a));
          }
        }
      }
    }

    const prev = [...row.agents];
    row.agents = next;
    providers = providers.map((p) => (p.id === providerId ? { ...p, agents: next } : p));

    try {
      const r = await setProviderRow(providerId, next);
      if (!r.ok) throw new Error(r.output);
    } catch (e) {
      row.agents = prev;
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
      await load();
    }
  }

  /** 把母版的分配关系注入到各 agent 的配置文件。 */
  async function apply() {
    busy = true;
    try {
      const r = await syncProviders(false);
      log = r.log.join('\n');
      await load();
    } catch (e) {
      log = `应用失败：${e instanceof Error ? e.message : String(e)}`;
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
      baseUrl: '',
      kind: 'anthropic',
      apiKey: '',
      models: [],
      agentKinds: {},
      note: '',
    };
  }

  function startEdit(id: string) {
    const p = providers.find((x) => x.id === id);
    if (!p) return;
    isNew = false;
    editing = p;
    form = {
      id: p.id,
      name: p.name,
      baseUrl: p.baseUrl,
      kind: p.kind,
      // 留空表示不修改
      apiKey: '',
      // 深拷贝：编辑时改动不应该提前影响列表里的对象
      models: p.models.map((m) => ({
        ...m,
        inputModalities: [...m.inputModalities],
        outputModalities: [...m.outputModalities],
        reasoningEfforts: [...m.reasoningEfforts],
      })),
      agentKinds: { ...p.agentKinds },
      note: p.note,
    };
  }

  /** 某个 agent 在表单里当前选的协议。没单独指定就跟随默认值。 */
  function kindForAgent(agent: string): string {
    return form.agentKinds[agent] ?? form.kind;
  }

  /** 设置某个 agent 的协议。等于默认值时删掉覆盖项，让母版保持简洁。 */
  function setKindForAgent(agent: string, kind: string) {
    if (kind === form.kind) {
      delete form.agentKinds[agent];
    } else {
      form.agentKinds[agent] = kind;
    }
  }

  /** 某个 agent 是否单独指定了协议。 */
  function hasOverride(agent: string): boolean {
    return agent in form.agentKinds;
  }

  function cancelEdit() {
    editing = null;
    isNew = false;
  }

  async function submit() {
    if (!form.id.trim() || !form.name.trim() || !form.baseUrl.trim()) {
      log = '请填写 id、名称和 base_url';
      return;
    }
    if (isNew && !form.apiKey.trim()) {
      log = '新增 provider 必须填写 API key';
      return;
    }
    busy = true;
    try {
      // agents 不从这里改：分配在展开面板里做。
      // 新增时留空，编辑时原样带回，避免把已有分配洗掉。
      const agents = editing ? editing.agents : [];
      // 丢掉空 id 的模型行，避免把半填的行存进母版
      const models = form.models.filter((m) => m.id.trim());
      const r = await saveProvider({
        id: form.id.trim(),
        name: form.name.trim(),
        baseUrl: form.baseUrl.trim(),
        kind: form.kind,
        apiKey: form.apiKey.trim(),
        models,
        agents,
        // 只提交和默认值不同的那些，母版才不会堆一堆同值覆盖项
        agentKinds: Object.fromEntries(
          Object.entries(form.agentKinds).filter(([, k]) => k !== form.kind),
        ),
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
    if (
      !confirm(
        `从母版删除 provider「${name}」？\n\n这只改母版。各 agent 里的配置要重新「应用到各 agent」才会变。`,
      )
    ) {
      return;
    }
    busy = true;
    try {
      const r = await deleteProvider(id);
      log = r.output;
      if (r.ok) await load();
    } catch (e) {
      log = `删除失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function openRaw() {
    try {
      rawToml = await readProvidersToml();
      showRaw = true;
    } catch (e) {
      log = `读取母版失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }

  async function saveRaw() {
    busy = true;
    try {
      const r = await writeProvidersToml(rawToml);
      log = r.output;
      if (r.ok) {
        showRaw = false;
        await load();
      }
    } catch (e) {
      log = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }

  onMount(load);
</script>

<PageHead title="Provider">
  所有 provider 在一个池子里，每个只保留一把 key。点某一行的
  <strong class="font-medium text-[var(--color-text)]">「分配」</strong>
  展开，选择这个 provider 要用在哪些 agent 上。
  <br />
  <span class="text-[var(--color-text-muted)]">
    改动先写母版（<code class="font-[var(--font-mono)] text-[11px]"
      >~/.agents/providers.toml</code
    >，含明文 key，已在 .gitignore 中隔离），点「应用到各 agent」才真正写入配置文件。
  </span>
</PageHead>

<div class="mb-4 flex flex-wrap items-center gap-2">
  <Button variant="primary" onclick={startNew} disabled={busy}>
    <Plus size={14} />
    新增 provider
  </Button>
  <span class="flex-1"></span>
  <Button variant="ghost" onclick={openRaw} disabled={busy}>编辑 TOML</Button>
  <Button variant="ghost" onclick={doBackfill} disabled={busy || loading}>
    从配置回填模型
  </Button>
  <Button variant="ghost" onclick={load} disabled={busy}>
    <RefreshCw size={14} />
    刷新
  </Button>
  <Button variant="primary" onclick={apply} disabled={busy || loading}>
    <Play size={14} />
    {busy ? '处理中…' : '应用到各 agent'}
  </Button>
</div>

<!-- 编辑表单 -->
{#if isNew || editing}
  <div
    class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-accent)] bg-[var(--color-bg-subtle)] p-4"
  >
    <h2 class="mb-3 text-[14px] font-semibold">
      {isNew ? '新增 provider' : `编辑 ${editing?.name}`}
    </h2>

    <div class="grid grid-cols-2 gap-3">
      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">标识 id</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px] disabled:opacity-60"
          bind:value={form.id}
          placeholder="command"
          disabled={!isNew}
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">展示名</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[13px]"
          bind:value={form.name}
          placeholder="CommandCode"
        />
      </label>

      <label class="col-span-2 block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">API 端点 base_url</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
          bind:value={form.baseUrl}
          placeholder="https://api.example.com/v1"
        />
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">
          默认协议
          <span class="text-[var(--color-text-faint)]">（未单独指定的 agent 用它）</span>
        </span>
        <select
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[13px]"
          bind:value={form.kind}
        >
          {#each KIND_OPTIONS as k (k.value)}
            <option value={k.value}>{k.label}</option>
          {/each}
        </select>
      </label>

      <!-- 按 agent 覆盖协议。
           这里列**所有支持 provider 的 agent**，不只是已分配的那些 ——
           新增 provider 时还没分配，但也应该能预设某个 agent 用别的协议。 -->
      {#if matrix && matrix.columns.length > 0}
        <div class="col-span-2">
          <span class="mb-1.5 block text-[11px] text-[var(--color-text-muted)]">
            各 agent 实际用的协议
            <span class="text-[var(--color-text-faint)]">
              （同一个端点在不同 agent 里可以不同，留默认值就不单独存）
            </span>
          </span>
          <div class="flex flex-col gap-1.5">
            {#each matrix.columns as col (col.agent)}
              {@const assigned = editing ? editing.agents.includes(col.agent) : false}
              {@const overridden = hasOverride(col.agent)}
              {@const options = kindsForAgent(col.agent)}
              <div class="flex items-center gap-2 {assigned ? '' : 'opacity-50'}">
                <span class="w-[110px] shrink-0 truncate text-[11px]" title={col.label}>
                  {col.label}
                </span>
                {#if options.length === 1}
                  <!-- 只支持一种协议：显示成文字，不给下拉，避免看着像能选 -->
                  <span
                    class="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg-subtle)] px-1.5 py-1 text-[11px] text-[var(--color-text-faint)]"
                    title="这个 agent 只支持这一种协议"
                  >
                    {options[0].label}
                  </span>
                {:else}
                  <select
                    class="min-w-0 flex-1 rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-1 text-[11px]"
                    value={kindForAgent(col.agent)}
                    onchange={(e) => setKindForAgent(col.agent, e.currentTarget.value)}
                  >
                    {#each options as k (k.value)}
                      <option value={k.value}>{k.label}</option>
                    {/each}
                  </select>
                {/if}
                {#if overridden && options.length > 1}
                  <button
                    class="w-[52px] shrink-0 text-[10px] text-[var(--color-text-faint)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
                    onclick={() => delete form.agentKinds[col.agent]}
                  >
                    恢复默认
                  </button>
                {:else}
                  <span class="w-[52px] shrink-0 text-[10px] text-[var(--color-text-faint)]">
                    {options.length === 1 ? '固定' : '默认'}
                  </span>
                {/if}
              </div>
            {/each}
          </div>
          <div class="mt-1 text-[10px] text-[var(--color-text-faint)]">
            灰色的 agent 当前没分配这个 provider。协议可以预先设好，分配后生效。
          </div>
        </div>
      {/if}

      <label class="block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">
          API key
          {#if editing}
            <span class="text-[var(--color-text-faint)]">（留空表示不修改）</span>
          {/if}
        </span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 font-[var(--font-mono)] text-[12px]"
          type="password"
          bind:value={form.apiKey}
          placeholder={editing ? `当前 ${editing.keyLen} 字符，末尾 ${editing.keyTail}` : 'sk-...'}
        />
      </label>

      <div class="col-span-2">
        <div class="mb-1.5 flex items-center gap-2">
          <span class="text-[11px] text-[var(--color-text-muted)]">
            模型（{form.models.length}）
          </span>
          <span class="flex-1"></span>
          <button
            class="text-[11px] text-[var(--color-text-muted)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
            onclick={() => form.models.push(blankModel())}
          >
            + 添加模型
          </button>
        </div>

        {#if form.models.length === 0}
          <div
            class="rounded-[var(--radius-md)] border border-dashed border-[var(--color-border-strong)] py-3 text-center text-[11px] text-[var(--color-text-faint)]"
          >
            还没加模型。注入时会给一个占位模型，实际用不了，建议至少填一个。
          </div>
        {:else}
          <div class="flex flex-col gap-1.5">
            {#each form.models as m, i (i)}
              {@const key = `${form.id}:${m.id}:${i}`}
              {@const open = openModel === key}
              <div class="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)]">
                <!-- 折叠态：id + 能力摘要 -->
                <div class="flex items-center gap-2 px-2.5 py-1.5">
                  <button
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[var(--color-text-faint)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
                    aria-label={open ? '收起模型元数据' : '展开模型元数据'}
                    onclick={() => (openModel = open ? null : key)}
                  >
                    {#if open}▾{:else}▸{/if}
                  </button>
                  <input
                    class="w-[220px] shrink-0 rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-0.5 font-[var(--font-mono)] text-[11px]"
                    bind:value={m.id}
                    placeholder="model-id"
                  />
                  <span class="min-w-0 flex-1 truncate text-[11px] text-[var(--color-text-faint)]">
                    {modelSummary(m)}
                  </span>
                  <button
                    class="shrink-0 rounded p-1 text-[var(--color-text-faint)] hover:bg-[var(--color-danger-bg)] hover:text-[var(--color-danger)]"
                    title="移除这个模型"
                    aria-label={`移除模型 ${m.id || i}`}
                    onclick={() => form.models.splice(i, 1)}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>

                <!-- 展开态：元数据表单 -->
                {#if open}
                  <div class="border-t border-[var(--color-border)] bg-[var(--color-bg-subtle)] px-3 py-2.5">
                    <div class="grid grid-cols-2 gap-2">
                      <label class="block">
                        <span class="mb-0.5 block text-[10px] text-[var(--color-text-muted)]">
                          展示名
                        </span>
                        <input
                          class="w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-1 text-[11px]"
                          bind:value={m.name}
                          placeholder="留空用 id"
                        />
                      </label>
                      <label class="block">
                        <span class="mb-0.5 block text-[10px] text-[var(--color-text-muted)]">
                          默认推理档位
                        </span>
                        <input
                          class="w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-1 text-[11px]"
                          bind:value={m.defaultEffort}
                          placeholder="如 high"
                        />
                      </label>
                      <label class="block">
                        <span class="mb-0.5 block text-[10px] text-[var(--color-text-muted)]">
                          上下文窗口
                        </span>
                        <input
                          class="w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-1 font-[var(--font-mono)] text-[11px]"
                          type="number"
                          bind:value={m.contextWindow}
                          placeholder="留空沿用现有值"
                        />
                      </label>
                      <label class="block">
                        <span class="mb-0.5 block text-[10px] text-[var(--color-text-muted)]">
                          输出上限
                        </span>
                        <input
                          class="w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-1.5 py-1 font-[var(--font-mono)] text-[11px]"
                          type="number"
                          bind:value={m.maxOutputTokens}
                          placeholder="留空沿用现有值"
                        />
                      </label>
                    </div>

                    <!-- 输入模态 -->
                    <div class="mt-2">
                      <span class="mb-1 block text-[10px] text-[var(--color-text-muted)]">
                        输入模态
                      </span>
                      <div class="flex flex-wrap gap-1">
                        {#each ['text', 'image', 'video', 'pdf'] as mod (mod)}
                          {@const on = m.inputModalities.includes(mod)}
                          <button
                            class="rounded-full px-2 py-0.5 text-[10px] transition-colors {on
                              ? 'bg-[var(--color-accent)] text-white'
                              : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]'}"
                            aria-pressed={on}
                            onclick={() => toggleModality(m, 'inputModalities', mod)}
                          >
                            {mod}
                          </button>
                        {/each}
                      </div>
                    </div>

                    <!-- 推理 -->
                    <div class="mt-2 flex flex-wrap items-center gap-3">
                      <label class="flex items-center gap-1.5 text-[10px] text-[var(--color-text-muted)]">
                        <input
                          type="checkbox"
                          class="accent-[var(--color-accent)]"
                          bind:checked={m.reasoning}
                        />
                        支持推理
                      </label>
                      <div class="flex flex-wrap items-center gap-1">
                        <span class="text-[10px] text-[var(--color-text-muted)]">档位</span>
                        {#each ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as lv (lv)}
                          {@const on = m.reasoningEfforts.includes(lv)}
                          <button
                            class="rounded-full px-2 py-0.5 text-[10px] transition-colors {on
                              ? 'bg-[var(--color-accent)] text-white'
                              : 'bg-[var(--color-bg-muted)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)]'}"
                            aria-pressed={on}
                            onclick={() => toggleEffort(m, lv)}
                          >
                            {lv}
                          </button>
                        {/each}
                      </div>
                    </div>

                    <div class="mt-2 text-[10px] text-[var(--color-text-faint)]">
                      留空的字段表示「不指定」，注入时沿用该 agent 配置里的现有值或内置默认。
                    </div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <label class="col-span-2 block">
        <span class="mb-1 block text-[11px] text-[var(--color-text-muted)]">备注</span>
        <input
          class="w-full rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2.5 py-1.5 text-[12px]"
          bind:value={form.note}
          placeholder="2026-11-10 从控制台获取"
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

<!-- 原始 TOML 编辑 -->
{#if showRaw}
  <div class="mb-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
    <h2 class="mb-1 text-[14px] font-semibold">直接编辑母版 TOML</h2>
    <p class="mb-2 text-[12px] text-[var(--color-text-muted)]">
      保存前会校验格式，写坏会被拒绝。
    </p>
    <textarea
      class="h-[280px] w-full resize-y rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-3 py-2 font-[var(--font-mono)] text-[12px]"
      bind:value={rawToml}
      spellcheck="false"
    ></textarea>
    <div class="mt-2 flex gap-2">
      <Button variant="primary" onclick={saveRaw} disabled={busy}>保存</Button>
      <Button variant="ghost" onclick={() => (showRaw = false)} disabled={busy}>取消</Button>
    </div>
  </div>
{/if}

<!-- provider 列表 -->
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
    池子里还没有 provider。<br />
    点「新增 provider」填一条，或「编辑 TOML」直接写。
  </div>
{:else}
  <div class="flex flex-col gap-2">
    {#each matrix.providers as row (row.id)}
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
            <KeyRound size={15} class="text-[var(--color-text-muted)]" />
          </div>

          <!-- 主体 -->
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="font-[var(--font-mono)] text-[13px] font-medium">{row.id}</span>
              <Badge tone={row.kind === 'anthropic' ? 'info' : 'plain'}>{row.kindLabel}</Badge>
              {#if Object.keys(row.agentKinds).length > 0}
                <span
                  class="rounded-full bg-[var(--color-warn-bg)] px-2 py-0.5 text-[10px] text-[var(--color-warn)]"
                  title={Object.entries(row.agentKinds)
                    .map(([a, k]) => `${labelOf(a)} → ${kindLabelOf(k)}`)
                    .join('\n')}
                >
                  {Object.keys(row.agentKinds).length} 个 agent 用别的协议
                </span>
              {/if}
              {#if row.keyLen === 0}
                <Badge tone="warn">无 key</Badge>
              {/if}
            </div>
            <div
              class="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
              title={row.baseUrl}
            >
              {row.baseUrl}
            </div>
            <!-- 模型列表：折叠态也要看得到，否则不知道这个 provider 有哪些模型 -->
            <div class="mt-0.5 flex flex-wrap items-center gap-1">
              {#if row.models.length === 0}
                <span class="text-[10px] text-[var(--color-text-faint)]">没有模型</span>
              {:else}
                {#each row.models.slice(0, 4) as m (m.id)}
                  <span
                    class="rounded bg-[var(--color-bg-muted)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-muted)]"
                    title={modelSummary(m)}
                  >
                    {m.id}
                  </span>
                {/each}
                {#if row.models.length > 4}
                  <span
                    class="text-[10px] text-[var(--color-text-faint)]"
                    title={row.models
                      .slice(4)
                      .map((m) => m.id)
                      .join('\n')}
                  >
                    +{row.models.length - 4}
                  </span>
                {/if}
              {/if}
            </div>
          </div>

          <!-- 模型数 -->
          <div class="hidden shrink-0 text-[11px] text-[var(--color-text-faint)] lg:block">
            {row.models.length} 个模型
          </div>

          <!-- 已分配标签。用 label 而不是裸 id，随 agent 实现变化自动跟随 -->
          <div class="hidden shrink-0 items-center gap-1 md:flex">
            {#if row.agents.length === 0}
              <span class="text-[11px] text-[var(--color-text-faint)]">未分配</span>
            {:else}
              {#each row.agents as a (a)}
                <span
                  class="rounded-full bg-[var(--color-accent)]/10 px-2 py-0.5 text-[10px] text-[var(--color-accent)]"
                >
                  {labelOf(a)}
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
                把这个 provider 注入到
              </span>
              <span class="flex-1"></span>
              <button
                class="text-[11px] text-[var(--color-text-muted)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
                onclick={() => toggleAllAgents(row.id)}
              >
                {matrix.columns.filter((c) => c.available).every((c) => row.agents.includes(c.agent))
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
                  title={col.available
                    ? `切换「${row.id}」在 ${col.label}`
                    : `${col.label} 不可用：${col.note}`}
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
    共 {matrix.providers.length} 个 provider，{matrix.columns.filter((c) => c.available).length}
    个可用 agent。
  </div>
{/if}

<!-- 边界说明 -->
<div class="mt-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-3">
  <div class="text-[11px] leading-relaxed text-[var(--color-text-muted)]">
    <strong class="text-[var(--color-text)]">单槽位的 agent 只有一个生效。</strong>
    这类 agent（如 Claude Code）同一时刻只能有一个 provider，勾上它会自动取消其他 provider 上的同一个 agent。
    <br />
    <span class="flex items-start gap-1 text-[var(--color-text-faint)]">
      <TriangleAlert size={11} class="mt-0.5 shrink-0" />
      应用前请退出正在运行的 agent，它们可能在退出时把内存里的配置写回磁盘，覆盖这次改动。
    </span>
  </div>
</div>

{#if log}
  <div class="mt-4">
    <Log text={log} maxHeight="320px" />
  </div>
{/if}
