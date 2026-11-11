<script lang="ts">
  /**
   * Provider 管理页。
   *
   * 列表只留 provider 本身（标识、协议、端点），不展开模型和已分配的 agent。
   * 编辑在对话框里完成。点「分配」进入单独一页，选择这个 provider 注入到哪些 agent。
   *
   * 分配关系直接写母版（`~/.agents/providers.toml` 的 `agents` 字段），
   * 点「应用到各 agent」才真正改各家的配置文件。
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
    ArrowLeft,
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
  import type { ClaudeModelMap, ModelSpec, ProviderMatrix, ProviderView } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';

  let matrix = $state<ProviderMatrix | null>(null);
  let providers = $state<ProviderView[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');

  /** 正在分配页里操作的 provider id。null 表示停在列表。 */
  let assigningId = $state<string | null>(null);

  /** 分配页当前这一条。加载后对不上就留空，页面会提示返回。 */
  let assigningRow = $derived(
    assigningId ? (matrix?.providers.find((p) => p.id === assigningId) ?? null) : null,
  );

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

  /** 「各 agent 实际用的协议」默认折叠，少占编辑区。 */
  let showAgentKinds = $state(false);

  /** 模型高级配置（模态 / 推理）默认折叠，对齐 zcode 编辑体验。 */
  let showModelAdvanced = $state(false);

  /**
   * Claude 档位映射弹窗。启用 Claude 时必填一次；已启用可再点「编辑映射」。
   * pendingAgents：确认后要写入的整行分配（启用场景会带上 claude）。
   */
  let claudeMapDialog = $state<null | {
    providerId: string;
    pendingAgents: string[];
    map: { opus: string; sonnet: string; haiku: string; default: string };
    mode: 'enable' | 'edit';
  }>(null);

  const MODALITY_OPTIONS = [
    { value: 'text', label: '文本' },
    { value: 'image', label: '图片' },
    { value: 'video', label: '视频' },
    { value: 'pdf', label: 'PDF' },
  ] as const;

  const EFFORT_OPTIONS = ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const;

  /** 空模型：只填了 id 的状态。 */
  function blankModel(id = ''): ModelSpec {
    return {
      id,
      inputModalities: [],
      outputModalities: [],
      reasoningEfforts: [],
    };
  }

  /** 把后端可能省略的数组补成空数组，避免渲染时读 .length 崩掉。 */
  function normalizeModel(m: ModelSpec): ModelSpec {
    return {
      ...m,
      inputModalities: m.inputModalities ?? [],
      outputModalities: m.outputModalities ?? [],
      reasoningEfforts: m.reasoningEfforts ?? [],
    };
  }

  /** 折叠态的一行能力摘要。 */
  function modelSummary(m: ModelSpec): string {
    const bits: string[] = [];
    if (m.contextWindow) bits.push(`${fmtK(m.contextWindow)} ctx`);
    if (m.maxOutputTokens) bits.push(`${fmtK(m.maxOutputTokens)} out`);
    const inputs = m.inputModalities ?? [];
    if (inputs.length > 0) bits.push(inputs.join('+'));
    if (m.reasoning) bits.push('reasoning');
    return bits.length > 0 ? bits.join(' · ') : '未填元数据';
  }

  /** 把 token 数压成 1M / 128K 这种短写法。 */
  function fmtK(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
    if (n >= 1000) return `${Math.round(n / 1000)}K`;
    return String(n);
  }

  /** 表单展示用 k token；母版里仍存原始 token 数。 */
  function tokensToK(n: number | undefined | null): string {
    if (n == null || Number.isNaN(n)) return '';
    if (n % 1000 === 0) return String(n / 1000);
    return String(Math.round((n / 1000) * 10) / 10);
  }

  function kToTokens(raw: string): number | undefined {
    const t = raw.trim();
    if (!t) return undefined;
    const v = Number(t);
    if (!Number.isFinite(v) || v < 0) return undefined;
    return Math.round(v * 1000);
  }

  function openModelEditor(key: string) {
    openModel = openModel === key ? null : key;
    showModelAdvanced = false;
  }

  function overrideCount(): number {
    return Object.keys(form.agentKinds).length;
  }

  /** 切换模型上的一个模态。 */
  function toggleModality(m: ModelSpec, list: 'inputModalities' | 'outputModalities', value: string) {
    if (!m[list]) m[list] = [];
    const arr = m[list];
    const i = arr.indexOf(value);
    if (i >= 0) arr.splice(i, 1);
    else arr.push(value);
  }

  /** 切换模型上的一个推理档位。 */
  function toggleEffort(m: ModelSpec, value: string) {
    if (!m.reasoningEfforts) m.reasoningEfforts = [];
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

      const rawMatrix = await withTimeout(providerMatrix(), '读取分配矩阵');
      // 兜底：旧后端可能省略空数组，这里统一补齐，避免列表渲染崩回骨架屏
      matrix = {
        ...rawMatrix,
        providers: (rawMatrix.providers ?? []).map((p) => ({
          ...p,
          agentKinds: p.agentKinds ?? {},
          models: (p.models ?? []).map(normalizeModel),
          agents: p.agents ?? [],
          claudeModelMap: p.claudeModelMap ?? {},
        })),
      };
      providers = (await withTimeout(listProviders(), '读取 provider 列表')).map((p) => ({
        ...p,
        agentKinds: p.agentKinds ?? {},
        models: (p.models ?? []).map(normalizeModel),
        agents: p.agents ?? [],
        claudeModelMap: p.claudeModelMap ?? {},
      }));
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

  /** 从 provider 模型列表推断档位默认值；已有映射优先。 */
  function suggestClaudeMap(p: ProviderView): {
    opus: string;
    sonnet: string;
    haiku: string;
    default: string;
  } {
    const ids = p.models.map((m) => m.id).filter(Boolean);
    const pick = (i: number) => (ids.length === 0 ? '' : ids[Math.min(i, ids.length - 1)]!);
    const existing = p.claudeModelMap ?? {};
    return {
      opus: existing.opus ?? pick(0),
      sonnet: existing.sonnet ?? pick(1),
      haiku: existing.haiku ?? pick(2),
      default: existing.default ?? pick(0),
    };
  }

  function openClaudeMapDialog(
    providerId: string,
    pendingAgents: string[],
    mode: 'enable' | 'edit',
  ) {
    const row = matrix?.providers.find((p) => p.id === providerId);
    if (!row) return;
    claudeMapDialog = {
      providerId,
      pendingAgents,
      map: suggestClaudeMap(row),
      mode,
    };
  }

  function closeClaudeMapDialog() {
    claudeMapDialog = null;
  }

  /** 确认映射：写入分配行 + claude_model_map。 */
  async function confirmClaudeMap() {
    const dlg = claudeMapDialog;
    if (!dlg) return;
    const map: ClaudeModelMap = {
      opus: dlg.map.opus || undefined,
      sonnet: dlg.map.sonnet || undefined,
      haiku: dlg.map.haiku || undefined,
      default: dlg.map.default || undefined,
    };

    const providerId = dlg.providerId;
    const next = dlg.pendingAgents;
    const row = matrix?.providers.find((p) => p.id === providerId);
    const prevAgents = row ? [...row.agents] : [];
    const prevMap = row ? { ...(row.claudeModelMap ?? {}) } : {};

    // 单槽位互斥：本地先清掉其他 provider 上的 claude
    if (matrix && next.includes('claude')) {
      for (const other of matrix.providers) {
        if (other.id !== providerId) {
          other.agents = other.agents.filter((a) => a !== 'claude');
        }
      }
      providers = providers.map((p) =>
        p.id === providerId ? p : { ...p, agents: p.agents.filter((a) => a !== 'claude') },
      );
    }

    if (row) {
      row.agents = next;
      row.claudeModelMap = map;
    }
    providers = providers.map((p) =>
      p.id === providerId ? { ...p, agents: next, claudeModelMap: map } : p,
    );
    claudeMapDialog = null;

    try {
      const r = await setProviderRow(providerId, next, map);
      if (!r.ok) throw new Error(r.output);
      await load();
    } catch (e) {
      if (row) {
        row.agents = prevAgents;
        row.claudeModelMap = prevMap;
      }
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
      await load();
    }
  }

  /**
   * 切换某个 provider 在某个 agent 的分配。
   *
   * 只由卡片左侧的勾选调用。乐观更新，失败回滚。
   * 单槽位 agent 勾上后会自动摘掉其他 provider 上的同一个 agent。
   */
  async function toggleAgent(providerId: string, agent: string) {
    const row = matrix?.providers.find((p) => p.id === providerId);
    if (!row) return;

    const on = !row.agents.includes(agent);
    const next = on ? [...row.agents, agent] : row.agents.filter((a) => a !== agent);

    // 单槽位 agent：本地先做互斥，界面不会出现两条同时高亮。
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

  /** 点卡片正文时要打开的额外配置。没有就返回 null，正文不可点。 */
  function agentDetail(agent: string): 'claude-map' | null {
    if (agent === 'claude') return 'claude-map';
    return null;
  }

  /** 打开这个 agent 的额外配置。不改启用状态。 */
  function openAgentDetail(providerId: string, agent: string) {
    const row = matrix?.providers.find((p) => p.id === providerId);
    if (!row || agentDetail(agent) !== 'claude-map') return;
    openClaudeMapDialog(providerId, [...row.agents], 'edit');
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
    openModel = null;
    showAgentKinds = false;
    showModelAdvanced = false;
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
    openModel = null;
    showAgentKinds = false;
    showModelAdvanced = false;
    form = {
      id: p.id,
      name: p.name,
      baseUrl: p.baseUrl,
      kind: p.kind,
      // 留空表示不修改
      apiKey: '',
      // 深拷贝：编辑时改动不应该提前影响列表里的对象
      models: p.models.map((raw) => {
        const m = normalizeModel(raw);
        return {
          ...m,
          inputModalities: [...m.inputModalities],
          outputModalities: [...m.outputModalities],
          reasoningEfforts: [...m.reasoningEfforts],
        };
      }),
      agentKinds: { ...(p.agentKinds ?? {}) },
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
    openModel = null;
    showAgentKinds = false;
    showModelAdvanced = false;
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

<svelte:window
  onkeydown={(e) => {
    if (e.key !== 'Escape' || busy) return;
    if (isNew || editing) cancelEdit();
    else if (claudeMapDialog) closeClaudeMapDialog();
    else if (assigningId) assigningId = null;
  }}
/>

{#if assigningId}
  <button
    type="button"
    class="mb-4 inline-flex items-center gap-1.5 text-[13px] text-[var(--color-text-muted)] transition-colors hover:text-[var(--color-text)]"
    onclick={() => (assigningId = null)}
  >
    <ArrowLeft size={14} />
    返回 provider 列表
  </button>

  {#if loading}
    <div class="flex flex-col gap-2">
      {#each Array(3) as _, i (i)}
        <div class="h-16 animate-pulse rounded-[var(--radius-lg)] bg-[var(--color-bg-muted)]"></div>
      {/each}
    </div>
  {:else if !assigningRow || !matrix}
    <PageHead title="分配">这条 provider 已经不在池里。</PageHead>
  {:else}
    {@const row = assigningRow}
    <PageHead title="分配 {row.name || row.id}">
      勾选左侧圆点启用或取消。Claude Code 这类要额外配置的，点卡片其余部分打开设置。勾选会记到母版，点「应用到各 agent」才写入各家配置。
    </PageHead>

    <div class="mb-4 flex flex-wrap items-center gap-3">
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <span class="font-[var(--font-mono)] text-[13px] font-medium">{row.id}</span>
          <Badge tone={row.kind === 'anthropic' ? 'info' : 'plain'}>{row.kindLabel}</Badge>
        </div>
        <div
          class="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-faint)]"
          title={row.baseUrl}
        >
          {row.baseUrl}
        </div>
      </div>
      <Button variant="primary" onclick={apply} disabled={busy}>
        <Play size={14} />
        {busy ? '处理中…' : '应用到各 agent'}
      </Button>
    </div>

    <div class="rounded-[var(--radius-lg)] border border-[var(--color-border)] p-4">
      <div class="mb-3 flex items-center gap-2">
        <span class="text-[13px] font-medium text-[var(--color-text)]">注入到这些 agent</span>
        <span class="flex-1"></span>
        <button
          type="button"
          class="text-[12px] text-[var(--color-text-muted)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
          onclick={() => toggleAllAgents(row.id)}
        >
          {matrix.columns.filter((c) => c.available).every((c) => row.agents.includes(c.agent))
            ? '全不选'
            : '全选'}
        </button>
      </div>

      <div class="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
        {#each matrix.columns as col (col.agent)}
          {@const on = isAssigned(row.id, col.agent)}
          {@const detail = col.available ? agentDetail(col.agent) : null}
          <div
            class="flex items-center gap-1 rounded-[var(--radius-md)] border py-1.5 pl-1.5 pr-3 transition-colors {!col.available
              ? 'border-[var(--color-border)] opacity-40'
              : on
                ? 'border-[var(--color-accent)] bg-[var(--color-accent)] text-white shadow-sm'
                : 'border-[var(--color-border-strong)] bg-[var(--color-bg)] text-[var(--color-text)]'}"
          >
            <button
              type="button"
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-transparent text-inherit disabled:cursor-not-allowed"
              disabled={!col.available || busy}
              title={col.available
                ? `${on ? '取消' : '启用'}「${row.id}」在 ${col.label}`
                : `${col.label} 不可用：${col.note}`}
              aria-pressed={on}
              aria-label={`${on ? '取消' : '启用'} ${col.label}`}
              onclick={() => toggleAgent(row.id, col.agent)}
            >
              <span
                class="flex h-5 w-5 items-center justify-center rounded-full border transition-colors {on
                  ? 'border-white/60 bg-white/20'
                  : 'border-[var(--color-border-strong)]'}"
              >
                {#if on}
                  <Check size={11} class={on ? 'text-white' : ''} />
                {/if}
              </span>
            </button>

            {#if detail}
              <button
                type="button"
                class="min-w-0 flex-1 rounded-[var(--radius-sm)] px-1.5 py-1 text-left text-inherit {on
                  ? 'hover:bg-white/10'
                  : 'hover:bg-[var(--color-bg-hover)]'}"
                disabled={busy}
                aria-label={`配置 ${col.label}`}
                onclick={() => openAgentDetail(row.id, col.agent)}
              >
                <span class="block truncate text-[12px] font-medium">{col.label}</span>
                <span class="block text-[10px] {on ? 'opacity-80' : 'text-[var(--color-text-faint)]'}">
                  {on ? '已启用' : '未启用'} · 配置档位
                </span>
              </button>
            {:else}
              <span class="min-w-0 flex-1 px-1.5 py-1">
                <span class="block truncate text-[12px] font-medium">{col.label}</span>
                {#if !col.available}
                  <span class="block text-[10px] opacity-70">未安装</span>
                {:else if on}
                  <span class="block text-[10px] opacity-80">已启用</span>
                {:else}
                  <span class="block text-[10px] text-[var(--color-text-faint)]">未启用</span>
                {/if}
              </span>
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <div class="mt-4 rounded-[var(--radius-lg)] border border-[var(--color-border)] p-3">
      <div class="text-[12px] leading-relaxed text-[var(--color-text-muted)]">
        <strong class="font-medium text-[var(--color-text)]">单槽位的 agent 只有一个生效。</strong>
        这类 agent（如 Claude Code）同一时刻只能有一个 provider，勾上它会自动取消其他 provider 上的同一个 agent。
        <span class="mt-1 flex items-start gap-1 text-[var(--color-text-faint)]">
          <TriangleAlert size={12} class="mt-0.5 shrink-0" />
          勾选会立刻写入母版。应用到各 agent 之前，请先退出正在运行的 agent，避免它们退出时用内存里的配置覆盖这次改动。
        </span>
      </div>
    </div>
  {/if}
{:else}
<PageHead title="Provider">
  每个 provider 只保留一把 key。编辑在对话框里完成；点「分配」进入单独页面，选择要用在哪些 agent 上。
  <br />
  <span class="text-[var(--color-text-muted)]">
    母版在 <code class="font-[var(--font-mono)] text-[11px]"
      >~/.agents/providers.toml</code
    >，含明文 key，已在 .gitignore 中隔离。写入各家配置在分配页完成。
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
</div>

<!-- 编辑对话框。点遮罩或取消关闭，不自动保存。 -->
{#if isNew || editing}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="provider-edit-title"
    onclick={(e) => {
      if (e.target === e.currentTarget && !busy) cancelEdit();
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape' && !busy) cancelEdit();
    }}
  >
    <div
      class="flex max-h-[min(880px,calc(100vh-2rem))] w-full max-w-3xl flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] shadow-lg"
    >
      <div class="border-b border-[var(--color-border)] px-5 py-4">
        <h2 id="provider-edit-title" class="text-[15px] font-semibold text-[var(--color-text)]">
          {isNew ? '新增 provider' : `编辑 ${editing?.name}`}
        </h2>
        <p class="mt-1 text-[12px] text-[var(--color-text-muted)]">
          {isNew
            ? '填写后保存到 provider 池。要用在哪些 agent 上，保存后再点「分配」。'
            : '改的是池里这一条。已有的分配不会在这里被清掉。'}
        </p>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div class="grid grid-cols-2 gap-3">
      <label class="block">
        <span class="field-label">标识 id</span>
        <input
          class="field-control w-full font-[var(--font-mono)]"
          bind:value={form.id}
          placeholder="command"
          disabled={!isNew}
        />
      </label>

      <label class="block">
        <span class="field-label">展示名</span>
        <input class="field-control w-full" bind:value={form.name} placeholder="CommandCode" />
      </label>

      <label class="col-span-2 block">
        <span class="field-label">API 端点 base_url</span>
        <input
          class="field-control w-full font-[var(--font-mono)]"
          bind:value={form.baseUrl}
          placeholder="https://api.example.com/v1"
        />
      </label>

      <label class="col-span-2 block">
        <span class="field-label">
          默认协议
          <span class="text-[var(--color-text-faint)]">（未单独指定的 agent 用它）</span>
        </span>
        <select class="field-control w-full" bind:value={form.kind}>
          {#each KIND_OPTIONS as k (k.value)}
            <option value={k.value}>{k.label}</option>
          {/each}
        </select>
      </label>

      <!-- 按 agent 覆盖协议。默认折叠，避免编辑区一上来就被长列表占满。 -->
      {#if matrix && matrix.columns.length > 0}
        <div class="col-span-2 rounded-[var(--radius-md)] border border-[var(--color-border)]">
          <button
            type="button"
            class="flex w-full items-center gap-2 px-3 py-2.5 text-left hover:bg-[var(--color-bg-hover)]"
            aria-expanded={showAgentKinds}
            onclick={() => (showAgentKinds = !showAgentKinds)}
          >
            <span class="text-[13px] text-[var(--color-text-faint)]">{showAgentKinds ? '▾' : '▸'}</span>
            <span class="min-w-0 flex-1">
              <span class="block text-[13px] font-medium text-[var(--color-text)]">
                各 agent 实际用的协议
              </span>
              <span class="block text-[12px] text-[var(--color-text-faint)]">
                {#if overrideCount() > 0}
                  已为 {overrideCount()} 个 agent 单独指定协议
                {:else}
                  默认折叠。同一端点在不同 agent 可不同，留默认就不单独存
                {/if}
              </span>
            </span>
          </button>

          {#if showAgentKinds}
            <div class="border-t border-[var(--color-border)] px-3 py-2.5">
              <div class="flex flex-col gap-2">
                {#each matrix.columns as col (col.agent)}
                  {@const assigned = editing ? editing.agents.includes(col.agent) : false}
                  {@const overridden = hasOverride(col.agent)}
                  {@const options = kindsForAgent(col.agent)}
                  <div class="flex items-center gap-2 {assigned ? '' : 'opacity-50'}">
                    <span
                      class="w-[120px] shrink-0 truncate text-[13px] text-[var(--color-text)]"
                      title={col.label}
                    >
                      {col.label}
                    </span>
                    {#if options.length === 1}
                      <span
                        class="field-control flex min-w-0 flex-1 items-center text-[var(--color-text-faint)]"
                        title="这个 agent 只支持这一种协议"
                      >
                        {options[0].label}
                      </span>
                    {:else}
                      <select
                        class="field-control min-w-0 flex-1"
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
                        class="w-[56px] shrink-0 text-[12px] text-[var(--color-text-faint)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
                        onclick={() => delete form.agentKinds[col.agent]}
                      >
                        恢复默认
                      </button>
                    {:else}
                      <span class="w-[56px] shrink-0 text-[12px] text-[var(--color-text-faint)]">
                        {options.length === 1 ? '固定' : '默认'}
                      </span>
                    {/if}
                  </div>
                {/each}
              </div>
              <div class="mt-2 text-[12px] text-[var(--color-text-faint)]">
                灰色的 agent 当前没分配这个 provider。协议可以预先设好，分配后生效。
              </div>
            </div>
          {/if}
        </div>
      {/if}

      <label class="col-span-2 block">
        <span class="field-label">
          API key
          {#if editing}
            <span class="text-[var(--color-text-faint)]">（留空表示不修改）</span>
          {/if}
        </span>
        <input
          class="field-control w-full font-[var(--font-mono)]"
          type="password"
          bind:value={form.apiKey}
          placeholder={editing ? `当前 ${editing.keyLen} 字符，末尾 ${editing.keyTail}` : 'sk-...'}
        />
      </label>

      <div class="col-span-2">
        <div class="mb-1.5 flex items-center gap-2">
          <span class="text-[13px] font-medium text-[var(--color-text)]">
            模型（{form.models.length}）
          </span>
          <span class="flex-1"></span>
          <button
            class="text-[13px] text-[var(--color-text-muted)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
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
          <div class="flex flex-col gap-2">
            {#each form.models as m, i (i)}
              {@const key = `${form.id}:${m.id}:${i}`}
              {@const open = openModel === key}
              <div class="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)]">
                <!-- 折叠态：id + 能力摘要 -->
                <div class="flex items-center gap-2 px-3 py-2">
                  <button
                    class="flex h-7 w-7 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[13px] text-[var(--color-text-faint)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]"
                    aria-label={open ? '收起模型元数据' : '展开模型元数据'}
                    onclick={() => openModelEditor(key)}
                  >
                    {#if open}▾{:else}▸{/if}
                  </button>
                  <input
                    class="field-control w-[240px] shrink-0 font-[var(--font-mono)]"
                    bind:value={m.id}
                    placeholder="model-id"
                  />
                  <span class="min-w-0 flex-1 truncate text-[12px] text-[var(--color-text-faint)]">
                    {modelSummary(m)}
                  </span>
                  <button
                    class="shrink-0 rounded-[var(--radius-sm)] p-1.5 text-[var(--color-text-faint)] hover:bg-[var(--color-danger-bg)] hover:text-[var(--color-danger)]"
                    title="移除这个模型"
                    aria-label={`移除模型 ${m.id || i}`}
                    onclick={() => form.models.splice(i, 1)}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>

                <!-- 展开态：基础字段常显，高级配置默认折叠（对齐 zcode） -->
                {#if open}
                  <div class="border-t border-[var(--color-border)] bg-[var(--color-bg)] px-3 py-3">
                    <div class="flex flex-col gap-3">
                      <label class="block">
                        <span class="field-label">展示名</span>
                        <input
                          class="field-control w-full"
                          bind:value={m.name}
                          placeholder="留空用 id"
                        />
                      </label>

                      <div class="grid grid-cols-2 gap-3">
                        <label class="block">
                          <span class="field-label">上下文窗口</span>
                          <div class="relative">
                            <input
                              class="field-control w-full pr-16 font-[var(--font-mono)]"
                              type="number"
                              min="0"
                              step="1"
                              value={tokensToK(m.contextWindow)}
                              placeholder="例如 1000"
                              oninput={(e) => (m.contextWindow = kToTokens(e.currentTarget.value))}
                            />
                            <span
                              class="pointer-events-none absolute inset-y-0 right-2.5 flex items-center text-[12px] text-[var(--color-text-faint)]"
                            >
                              k token
                            </span>
                          </div>
                        </label>
                        <label class="block">
                          <span class="field-label">最大输出 Token</span>
                          <div class="relative">
                            <input
                              class="field-control w-full pr-16 font-[var(--font-mono)]"
                              type="number"
                              min="0"
                              step="1"
                              value={tokensToK(m.maxOutputTokens)}
                              placeholder="例如 128"
                              oninput={(e) => (m.maxOutputTokens = kToTokens(e.currentTarget.value))}
                            />
                            <span
                              class="pointer-events-none absolute inset-y-0 right-2.5 flex items-center text-[12px] text-[var(--color-text-faint)]"
                            >
                              k token
                            </span>
                          </div>
                        </label>
                      </div>

                      <div class="rounded-[var(--radius-md)] border border-[var(--color-border)]">
                        <button
                          type="button"
                          class="flex w-full items-center gap-2 px-3 py-2.5 text-left hover:bg-[var(--color-bg-hover)]"
                          aria-expanded={showModelAdvanced}
                          onclick={() => (showModelAdvanced = !showModelAdvanced)}
                        >
                          <span class="text-[13px] text-[var(--color-text-faint)]">
                            {showModelAdvanced ? '▾' : '▸'}
                          </span>
                          <span class="text-[13px] font-medium text-[var(--color-text)]">高级配置</span>
                        </button>

                        {#if showModelAdvanced}
                          <div class="space-y-3 border-t border-[var(--color-border)] px-3 py-3">
                            <div>
                              <span class="field-label">输入类型</span>
                              <div class="flex flex-wrap gap-1.5">
                                {#each MODALITY_OPTIONS as mod (mod.value)}
                                  {@const on = m.inputModalities.includes(mod.value)}
                                  <button
                                    type="button"
                                    class="rounded-[var(--radius-sm)] border px-2.5 py-1.5 text-[12px] transition-colors {on
                                      ? 'border-[var(--color-accent)] bg-[var(--color-accent)] text-white'
                                      : 'border-[var(--color-border-strong)] bg-[var(--color-bg)] text-[var(--color-text-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-text)]'}"
                                    aria-pressed={on}
                                    onclick={() => toggleModality(m, 'inputModalities', mod.value)}
                                  >
                                    {mod.label}
                                  </button>
                                {/each}
                              </div>
                            </div>

                            <div>
                              <span class="field-label">推理等级（从低到高）</span>
                              <div class="mb-2 flex items-center gap-2">
                                <label class="flex items-center gap-1.5 text-[13px] text-[var(--color-text)]">
                                  <input
                                    type="checkbox"
                                    class="size-3.5 accent-[var(--color-accent)]"
                                    bind:checked={m.reasoning}
                                  />
                                  支持推理
                                </label>
                              </div>
                              <div class="flex flex-wrap gap-1.5">
                                {#each EFFORT_OPTIONS as lv (lv)}
                                  {@const on = m.reasoningEfforts.includes(lv)}
                                  <button
                                    type="button"
                                    class="rounded-[var(--radius-sm)] border px-2.5 py-1.5 font-[var(--font-mono)] text-[12px] transition-colors {on
                                      ? 'border-[var(--color-accent)] bg-[var(--color-accent)] text-white'
                                      : 'border-[var(--color-border-strong)] bg-[var(--color-bg)] text-[var(--color-text-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-text)]'}"
                                    aria-pressed={on}
                                    onclick={() => toggleEffort(m, lv)}
                                  >
                                    {lv}
                                  </button>
                                {/each}
                              </div>
                            </div>

                            <label class="block">
                              <span class="field-label">默认推理档位</span>
                              <input
                                class="field-control w-full"
                                bind:value={m.defaultEffort}
                                placeholder="如 high / max"
                              />
                            </label>
                          </div>
                        {/if}
                      </div>

                      <div class="text-[12px] text-[var(--color-text-faint)]">
                        上下文与输出按 k token 填写（1000 = 100 万 token）。留空表示不指定，注入时沿用现有值。
                      </div>
                    </div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <label class="col-span-2 block">
        <span class="field-label">备注</span>
        <input
          class="field-control w-full"
          bind:value={form.note}
          placeholder="2026-11-10 从控制台获取"
        />
      </label>
        </div>
      </div>

      <div class="flex justify-end gap-2 border-t border-[var(--color-border)] px-5 py-3">
        <Button variant="ghost" onclick={cancelEdit} disabled={busy}>取消</Button>
        <Button variant="primary" onclick={submit} disabled={busy}>
          {busy ? '保存中…' : '保存'}
        </Button>
      </div>
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
      <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
        <div class="flex items-center gap-3 px-4 py-3">
          <!-- 类型图标 -->
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-bg-muted)]"
          >
            <KeyRound size={15} class="text-[var(--color-text-muted)]" />
          </div>

          <div class="min-w-0 flex-1 overflow-hidden">
            <div class="flex flex-wrap items-center gap-2">
              <span class="font-[var(--font-mono)] text-[13px] font-medium">{row.id}</span>
              {#if row.name && row.name !== row.id}
                <span class="text-[13px] text-[var(--color-text-muted)]">{row.name}</span>
              {/if}
              <Badge tone={row.kind === 'anthropic' ? 'info' : 'plain'}>{row.kindLabel}</Badge>
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
          </div>

          <!-- 操作 -->
          <div class="flex shrink-0 items-center gap-1">
            <button
              class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
              onclick={() => (assigningId = row.id)}
            >
              分配
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
      </div>
    {/each}
  </div>

  <div class="mt-2 text-[11px] text-[var(--color-text-faint)]">
    共 {matrix.providers.length} 个 provider，{matrix.columns.filter((c) => c.available).length}
    个可用 agent。
  </div>
{/if}

{/if}

{#if log}
  <div class="mt-4">
    <Log text={log} maxHeight="320px" />
  </div>
{/if}

{#if claudeMapDialog}
  {@const dlg = claudeMapDialog}
  {@const p = matrix?.providers.find((x) => x.id === dlg.providerId)}
  {@const modelOptions = p?.models.map((m) => m.id).filter(Boolean) ?? []}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="claude-map-title"
    onclick={(e) => {
      if (e.target === e.currentTarget) closeClaudeMapDialog();
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape') closeClaudeMapDialog();
    }}
  >
    <div
      class="w-full max-w-md rounded-[var(--radius-lg)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] p-4 shadow-lg"
    >
      <h2 id="claude-map-title" class="text-[14px] font-medium text-[var(--color-text)]">
        {dlg.mode === 'enable' ? '启用 Claude：选择档位模型' : '编辑 Claude 档位映射'}
      </h2>
      <p class="mt-1 text-[12px] text-[var(--color-text-muted)]">
        为 opus / sonnet / haiku / 默认 指定本 provider 的模型 id，写入母版
        <code class="text-[11px]">claude_model_map</code>。
        Claude Code 走 Anthropic <code class="text-[11px]">/messages</code>，请选网关里支持该协议的模型（例如 CommandCode 的
        <code class="text-[11px]">claude-*</code>）；只开了 chat/completions 的 DeepSeek 等会报 model 不可用。
      </p>

      {#if modelOptions.length === 0}
        <p class="mt-3 text-[12px] text-[var(--color-danger)]">
          这个 provider 还没有模型。请先在编辑里加上模型，再设映射。
        </p>
      {:else}
        <div class="mt-4 grid gap-3">
          {#each [
            { key: 'opus' as const, label: 'Opus（ANTHROPIC_DEFAULT_OPUS_MODEL）' },
            { key: 'sonnet' as const, label: 'Sonnet（ANTHROPIC_DEFAULT_SONNET_MODEL）' },
            { key: 'haiku' as const, label: 'Haiku（ANTHROPIC_DEFAULT_HAIKU_MODEL）' },
            { key: 'default' as const, label: '默认（ANTHROPIC_MODEL）' },
          ] as slot}
            <label class="block">
              <span class="field-label">{slot.label}</span>
              <select
                class="field-control w-full"
                value={dlg.map[slot.key]}
                onchange={(e) => {
                  if (!claudeMapDialog) return;
                  claudeMapDialog = {
                    ...claudeMapDialog,
                    map: {
                      ...claudeMapDialog.map,
                      [slot.key]: (e.currentTarget as HTMLSelectElement).value,
                    },
                  };
                }}
              >
                {#each modelOptions as mid}
                  <option value={mid}>{mid}</option>
                {/each}
              </select>
            </label>
          {/each}
        </div>
      {/if}

      <div class="mt-4 flex justify-end gap-2">
        <Button variant="ghost" onclick={closeClaudeMapDialog}>取消</Button>
        <Button
          variant="primary"
          disabled={busy || modelOptions.length === 0}
          onclick={confirmClaudeMap}
        >
          {dlg.mode === 'enable' ? '确认启用' : '保存映射'}
        </Button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* 表单标签与控件成套：字号 12/13，控件高度统一，避免「字小框大」 */
  :global(.field-label) {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--color-text-muted);
  }

  :global(.field-control) {
    display: block;
    box-sizing: border-box;
    height: 36px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    padding: 0 10px;
    font-size: 13px;
    line-height: 1.4;
    color: var(--color-text);
  }

  :global(select.field-control) {
    padding-right: 28px;
  }

  :global(.field-control:disabled) {
    opacity: 0.6;
  }
</style>
