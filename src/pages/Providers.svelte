<script lang="ts">
  /**
   * Provider 管理页。
   *
   * 两个视角共用同一份分配数据（母版 `~/.agents/providers.toml` 里每条
   * provider 的 `agents` 字段），只是分组方向相反：
   *
   * - 「provider 管理」：一行一个 provider，只管池子的增删改
   * - 「启用 provider」：一行一个 agent，勾选它启用哪几个 provider
   *
   * 两边的勾选都只写母版；点「应用到各 agent」（在启用视角的工具栏里）
   * 才真正改各家的配置文件。
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
    RefreshCw,
    Play,
    KeyRound,
    Terminal,
    GripVertical,
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
    cursorByokStatus,
    setCodexModel,
  } from '../lib/ipc';
  import type {
    ClaudeModelMap,
    ModelSpec,
    ProviderMatrix,
    ProviderView,
  } from '../lib/types';
  import PageHead from '../lib/components/PageHead.svelte';
  import Button from '../lib/components/Button.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Log from '../lib/components/Log.svelte';
  import CursorByok from '../lib/components/CursorByok.svelte';

  /**
   * 组件参数。
   *
   * `view` 由 App.svelte 根据侧栏选中项传入 —— Provider 拆成两个侧栏子页，
   * 页内不再有自己的切换器，同一个组件按 view 渲染不同内容。
   *
   * 两个跳转回调也是 App 注入的：它们要改的是侧栏的当前页，组件自己
   * 改不了。跨到别的页（Claude & Codex）用 onGoClaudeCodex，
   * 在两个 Provider 子页之间跳用 onGoProviderPool。
   */
  let {
    view = 'provider',
    onGoClaudeCodex = () => {},
    onGoProviderPool = () => {},
  }: {
    view?: 'provider' | 'agent';
    onGoClaudeCodex?: () => void;
    onGoProviderPool?: () => void;
  } = $props();

  let matrix = $state<ProviderMatrix | null>(null);
  let providers = $state<ProviderView[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let log = $state('');

  /** 正在分配页里操作的 provider id。null 表示停在列表。 */
  let assigningId = $state<string | null>(null);

  /**
   * 「启用 provider」视角里那个 Cursor 卡片要不要显示。
   *
   * Cursor 不在 provider 矩阵里（`supports_provider()` 是 false），所以
   * 判断它装没装得单独读一次它自己的配置库。读失败就当没装。
   */
  let cursorAvailable = $state(false);
  /** 要写进 Cursor 的那个 provider。由卡片里的 CursorByok 双向绑定。 */
  let cursorProviderId = $state('');

  // ---- 启用视角的折叠与排序 ----
  //
  // 卡片默认全部折叠：一台机器上不一定每个 agent 都用，全展开页面太长。
  // 顺序存在 localStorage 里，跨刷新保留；放前端是因为它纯粹是显示偏好，
  // 写进母版会污染「配置」这个语义。

  /** 哪些 agent 卡片当前是展开的。默认空 = 全折叠。 */
  let agentOpen = $state<Set<string>>(new Set());

  /** 用户自定义的 agent 顺序。空数组表示用后端的默认顺序。 */
  let agentOrder = $state<string[]>([]);

  /** 正在被拖动的那一个，仅用于高亮。 */
  let dragging = $state<string | null>(null);

  const ORDER_KEY = 'agentcenter.providers.agentOrder';

  /** 读回上次存的顺序。 */
  function loadAgentOrder() {
    try {
      const raw = localStorage.getItem(ORDER_KEY);
      if (!raw) return;
      const arr = JSON.parse(raw);
      if (Array.isArray(arr)) {
        agentOrder = arr.filter((x): x is string => typeof x === 'string');
      }
    } catch {
      // 存坏了就当没存过，不值得打断页面
      agentOrder = [];
    }
  }

  function saveAgentOrder() {
    try {
      localStorage.setItem(ORDER_KEY, JSON.stringify(agentOrder));
    } catch {
      // 隐私模式或被禁写时会抛，忽略即可
    }
  }

  /**
   * 按用户顺序排好的列。
   *
   * 顺序表里没提到的 agent 追加在末尾，这样后端新增 agent 时不用手动补，
   * 也不会因为顺序表过期把它们藏起来。
   */
  let orderedColumns = $derived.by(() => {
    const cols = matrix?.columns ?? [];
    if (agentOrder.length === 0) return cols;
    const rank = new Map(agentOrder.map((id, i) => [id, i]));
    return [...cols].sort((a, b) => {
      const ra = rank.get(a.agent) ?? Number.MAX_SAFE_INTEGER;
      const rb = rank.get(b.agent) ?? Number.MAX_SAFE_INTEGER;
      return ra - rb;
    });
  });

  /** 展开 / 收起一张卡片。 */
  function toggleAgentOpen(agent: string) {
    const next = new Set(agentOpen);
    if (next.has(agent)) next.delete(agent);
    else next.add(agent);
    agentOpen = next;
  }

  /** 把被拖动的那个放到目标位置。 */
  function dropAgent(target: string) {
    const from = dragging;
    dragging = null;
    if (!from || from === target) return;

    // 以当前显示顺序为基准重排，这样用户看到的位置和结果一致
    const order = orderedColumns.map((c) => c.agent);
    const fi = order.indexOf(from);
    const ti = order.indexOf(target);
    if (fi < 0 || ti < 0) return;

    order.splice(fi, 1);
    order.splice(ti, 0, from);
    agentOrder = order;
    saveAgentOrder();
  }

  /** 恢复后端给的默认顺序。 */
  function resetAgentOrder() {
    agentOrder = [];
    try {
      localStorage.removeItem(ORDER_KEY);
    } catch {
      // 同上，忽略
    }
  }

  /**
   * 滚到某个 agent 的卡片，并展开它。
   *
   * 跳过去却不展开等于没跳 —— 用户还是看不到内容。
   */
  function jumpToAgent(agent: string) {
    if (!agentOpen.has(agent)) {
      const next = new Set(agentOpen);
      next.add(agent);
      agentOpen = next;
    }
    // 等展开后的 DOM 落地再滚，否则量到的是折叠时的高度
    requestAnimationFrame(() => {
      document
        .querySelector(`[data-agent-card="${agent}"]`)
        ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    });
  }

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

  /**
   * Codex 选模型弹窗。
   *
   * Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型进不去，
   * 所以「用哪个模型」只能在这里选，写进母版 `codex_model`，再由注入器
   * 落到 `~/.codex/config.toml` 顶层的 `model`。
   */
  let codexModelDialog = $state<null | {
    providerId: string;
    /** 当前选中的模型 id；空串表示不指定、沿用第一个 */
    model: string;
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
          // 旧后端可能完全不返回这个字段；补成 null（= 自动）
          codexRequiresOpenaiAuth: p.codexRequiresOpenaiAuth ?? null,
        })),
      };
      providers = (await withTimeout(listProviders(), '读取 provider 列表')).map((p) => ({
        ...p,
        agentKinds: p.agentKinds ?? {},
        models: (p.models ?? []).map(normalizeModel),
        agents: p.agents ?? [],
        claudeModelMap: p.claudeModelMap ?? {},
        codexRequiresOpenaiAuth: p.codexRequiresOpenaiAuth ?? null,
      }));
    } catch (e) {
      log = `读取失败：${e instanceof Error ? e.message : String(e)}`;
      console.error('[Providers] load failed', e);
    } finally {
      loading = false;
    }

    // Cursor 单独探一次：它不在 provider 矩阵里，装没装要看它自己的配置库。
    // 读失败当没装，不让它影响整页。
    try {
      cursorAvailable = (await cursorByokStatus()).installed;
    } catch {
      cursorAvailable = false;
    }
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

  // ---- agent 视角 ----
  //
  // 同一份数据反过来看：某个 agent 现在启用了哪几个 provider。
  // 勾选动作仍然是写母版里的 `agents` 列表，只是入口从「provider 找 agent」
  // 变成「agent 找 provider」。

  /**
   * 把母版的分配关系注入到各 agent 的配置文件。
   *
   * 只在「启用 provider」视角的工具栏出现 —— 那是用户刚勾完分配的地方，
   * 按完立刻生效符合预期。
   */
  async function applyAll() {
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

  /** 某个 agent 上已启用的 provider（按母版顺序）。 */
  function providersForAgent(agent: string): ProviderView[] {
    return (matrix?.providers ?? []).filter((p) => p.agents.includes(agent));
  }

  /** 这个 agent 是不是单槽位（同一时刻只能有一个 provider 生效）。 */
  function agentIsSingleSlot(agent: string): boolean {
    return matrix?.columns.find((c) => c.agent === agent)?.singleSlot ?? false;
  }

  /**
   * 某个 provider 在某个 agent 下实际走哪种协议。
   *
   * 优先级：provider 对这个 agent 的覆盖 > provider 的默认值。
   * 但还要受 agent 能力的约束 —— Codex 只认 openai-responses，
   * 就算 provider 默认是 anthropic，注入时也会被改写成 responses。
   * 界面必须显示**实际会发生**的那个，否则用户以为填的 anthropic 生效了。
   */
  function effectiveKind(p: ProviderView, agent: string): string {
    const declared = p.agentKinds?.[agent] ?? p.kind;
    const allowed = matrix?.columns.find((c) => c.agent === agent)?.supportedKinds ?? [];
    if (allowed.length === 0) return declared;
    // 声明的不被支持时，注入器会落到该 agent 支持的第一种
    return allowed.includes(declared) ? declared : allowed[0];
  }

  /**
   * agent 视角下勾选一个 provider。
   *
   * 单槽位 agent 走「整行替换」语义：勾上它就等于把它设成唯一的那个，
   * 自动摘掉其他 provider 上的这个 agent。多槽位的就是普通加减。
   *
   * 不复用 toggleAgent：那个是 provider 视角的「provider × agent」单元格，
   * 这里的主语是 agent，语义方向相反，混用会让以后的改动互相绊。
   */
  async function toggleProviderForAgent(providerId: string, agent: string) {
    const row = matrix?.providers.find((p) => p.id === providerId);
    if (!row) return;

    const wasOn = row.agents.includes(agent);
    const single = agentIsSingleSlot(agent);

    // 记下所有要被改动的行，失败时整体回滚
    const snapshot = (matrix?.providers ?? []).map((p) => ({ id: p.id, agents: [...p.agents] }));

    // 先算好每个 provider 的新 agents 列表，统一应用
    const nextById = new Map<string, string[]>();
    if (single && !wasOn) {
      // 单槽位：把这个 agent 从所有 provider 上摘掉，再挂到目标上
      for (const p of matrix?.providers ?? []) {
        nextById.set(
          p.id,
          p.id === providerId ? [...p.agents.filter((a) => a !== agent), agent] : p.agents.filter((a) => a !== agent),
        );
      }
    } else {
      nextById.set(
        providerId,
        wasOn ? row.agents.filter((a) => a !== agent) : [...row.agents, agent],
      );
    }

    // 乐观更新
    for (const p of matrix?.providers ?? []) {
      const next = nextById.get(p.id);
      if (next) p.agents = next;
    }
    providers = providers.map((p) => {
      const next = nextById.get(p.id);
      return next ? { ...p, agents: next } : p;
    });

    busy = true;
    try {
      // 单槽位会同时改多行，逐行提交；多槽位只有一行
      for (const [id, agents] of nextById) {
        // 单槽位的互斥由后端 set_row 里的 exclusive 逻辑保证，
        // 这里显式把每次的目标状态传过去即可
        const r = await setProviderRow(id, agents);
        if (!r.ok) throw new Error(r.output);
      }
    } catch (e) {
      // 整体回滚到操作前的快照
      for (const p of matrix?.providers ?? []) {
        const old = snapshot.find((s) => s.id === p.id);
        if (old) p.agents = old.agents;
      }
      providers = providers.map((p) => {
        const old = snapshot.find((s) => s.id === p.id);
        return old ? { ...p, agents: old.agents } : p;
      });
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
      await load();
    } finally {
      busy = false;
    }
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

  function closeClaudeMapDialog() {
    claudeMapDialog = null;
  }

  /** 确认 Codex 用哪个模型：写母版 `codex_model`，立刻重写 codex 配置。 */
  async function confirmCodexModel() {
    const dlg = codexModelDialog;
    if (!dlg) return;
    const providerId = dlg.providerId;
    const picked = dlg.model.trim();

    busy = true;
    try {
      const r = await setCodexModel(providerId, picked || null);
      log = r.output;
      if (!r.ok) throw new Error(r.output);
      codexModelDialog = null;
      await load();
    } catch (e) {
      log = `写入失败：${e instanceof Error ? e.message : String(e)}`;
      await load();
    } finally {
      busy = false;
    }
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
        // 原样带回：编辑其他字段不能把 Codex 的这两个设置洗掉。
        // 它们只在启用页的 Codex 卡片里改。
        codexRequiresOpenaiAuth: editing ? (editing.codexRequiresOpenaiAuth ?? null) : null,
        codexModel: editing ? (editing.codexModel ?? null) : null,
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

  // 顺序是纯显示偏好，放 localStorage，页面一打开就恢复
  onMount(loadAgentOrder);
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key !== 'Escape' || busy) return;
    if (isNew || editing) cancelEdit();
    else if (claudeMapDialog) closeClaudeMapDialog();
    else if (assigningId) assigningId = null;
  }}
/>

{#if view === 'provider'}
  <PageHead title="provider 管理">
    维护 provider 池：每个 provider 只保留一把 key，编辑在对话框里完成。
    <br />
    <span class="text-[var(--color-text-muted)]">
      母版在 <code class="font-[var(--font-mono)] text-[11px]"
        >~/.agents/providers.toml</code
      >，含明文 key，已在 .gitignore 中隔离。要用在哪些 agent 上，看左侧的「启用 provider」。
    </span>
  </PageHead>
{:else}
  <PageHead title="启用 provider">
    按 agent 分组，勾选每个 agent 启用哪几个 provider。勾选只写母版，
    点「应用到各 agent」才真正写进各家配置文件。
    <br />
    <span class="text-[var(--color-text-muted)]">
      单槽位的 agent（Claude Code、Codex 等）同一时刻只能有一个 provider 生效。
    </span>
  </PageHead>
{/if}

<div class="mb-4 flex flex-wrap items-center gap-2">
  <!--
    第一个位置按视角换：provider 管理放「新增」，启用 provider 放「应用到各 agent」。
    两者的语义都是「这一屏的主要动作」，所以共用一个位置。
    视角由侧栏决定，这里不再有页内切换器。
  -->
  {#if view === 'provider'}
    <Button variant="primary" onclick={startNew} disabled={busy}>
      <Plus size={14} />
      新增 provider
    </Button>
  {:else}
    <Button variant="primary" onclick={applyAll} disabled={busy || loading}>
      <Play size={14} />
      {busy ? '处理中…' : '应用到各 agent'}
    </Button>
  {/if}

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
            ? '填写后保存到 provider 池。要用在哪些 agent 上，保存后切到「启用 provider」。'
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
                  <div class="grid grid-cols-[7.5rem_minmax(0,1fr)_4.5rem] items-center gap-x-4 {assigned ? '' : 'opacity-50'}">
                    <span class="truncate text-[13px] text-[var(--color-text)]" title={col.label}>
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
                        type="button"
                        class="whitespace-nowrap text-right text-[12px] text-[var(--color-text-faint)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
                        onclick={() => delete form.agentKinds[col.agent]}
                      >
                        恢复默认
                      </button>
                    {:else}
                      <span class="text-right text-[12px] text-[var(--color-text-faint)]">
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
    {#if view === 'agent'}
      <!-- 这个视角只能分配，新增要去另一个子页 -->
      先去
      <button
        type="button"
        class="underline underline-offset-2 hover:text-[var(--color-accent)]"
        onclick={() => onGoProviderPool()}
      >
        provider 管理
      </button>
      页新增一条。
    {:else}
      点「新增 provider」填一条，或「编辑 TOML」直接写。
    {/if}
  </div>
{:else if view === 'agent'}
  <!-- ===== agent 视角 =====
       一行一个 agent，默认折叠。展开后列出它能用的 provider。
       单槽位的 agent 是单选，多槽位的是多选。
       改的还是母版里每个 provider 的 agents 列表。 -->

  <!-- 顺序与跳转条：拖动排这个页面的卡片顺序，点名字滚到那一张。
       不单独留标题行 —— 名字块本身加 title 提示就够了，多一行纯属占地方。
       容器和名字块都不加边框，靠底色分层，和页面其它区块一致。 -->
  <div
    class="mb-3 flex flex-wrap items-center gap-1.5 rounded-[var(--radius-lg)] bg-[var(--color-bg-subtle)] px-3 py-2.5"
  >
    {#each orderedColumns as col (col.agent)}
      <button
        type="button"
        class="flex cursor-grab items-center gap-1 rounded-[var(--radius-sm)] px-2 py-1 text-[11px] transition-colors active:cursor-grabbing {dragging === col.agent
          ? 'bg-[var(--color-accent)]/15 text-[var(--color-text)]'
          : 'bg-[var(--color-bg)] text-[var(--color-text-muted)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text)]'}"
        draggable="true"
        title={`拖动排序，点击跳到 ${col.label}`}
        ondragstart={() => (dragging = col.agent)}
        ondragover={(e) => e.preventDefault()}
        ondrop={() => dropAgent(col.agent)}
        ondragend={() => (dragging = null)}
        onclick={() => jumpToAgent(col.agent)}
      >
        <GripVertical size={11} class="text-[var(--color-text-faint)]" />
        {col.label}
      </button>
    {/each}

    <!-- 两个顺序按钮跟在名字块后面，不单独占一行 -->
    <span class="flex-1"></span>
    <button
      type="button"
      class="text-[11px] text-[var(--color-text-faint)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
      title="按名称的字母序排列"
      onclick={() => {
        agentOrder = [...matrix!.columns].sort((a, b) => a.label.localeCompare(b.label)).map((c) => c.agent);
        saveAgentOrder();
      }}
    >
      按名称
    </button>
    <button
      type="button"
      class="text-[11px] text-[var(--color-text-faint)] underline-offset-2 hover:text-[var(--color-accent)] hover:underline"
      title="回到后端给的默认顺序"
      onclick={resetAgentOrder}
    >
      恢复默认
    </button>
  </div>

  <div class="flex flex-col gap-2">
    {#each orderedColumns as col (col.agent)}
      {@const chosen = providersForAgent(col.agent)}
      {@const single = col.singleSlot}
      {@const open = agentOpen.has(col.agent)}
      <div
        class="scroll-mt-4 overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]"
        data-agent-card={col.agent}
      >
        <!-- 卡片头：整块可点，展开/收起。右侧常显已启用的 provider -->
        <button
          type="button"
          class="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-[var(--color-bg-hover)]"
          aria-expanded={open}
          aria-controls={`agent-body-${col.agent}`}
          onclick={() => toggleAgentOpen(col.agent)}
        >
          <span class="text-[13px] text-[var(--color-text-faint)]">{open ? '▾' : '▸'}</span>
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-bg-muted)]"
          >
            <Terminal size={15} class="text-[var(--color-text-muted)]" />
          </div>

          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-[13px] font-medium text-[var(--color-text)]">{col.label}</span>
              {#if single}
                <Badge tone="info">单槽位</Badge>
              {/if}
              {#if !col.available}
                <Badge tone="plain">{col.note || '未安装'}</Badge>
              {/if}
            </div>
            <div class="text-[11px] text-[var(--color-text-faint)]">
              {#if !col.available}
                {col.note || '配置目录不存在，无法注入'}
              {:else if chosen.length === 0}
                还没有启用任何 provider
              {:else}
                {single ? '当前生效' : `已启用 ${chosen.length} 个`}：{chosen.map((p) => p.id).join(', ')}
              {/if}
            </div>
          </div>
        </button>

        {#if open}
          <div id={`agent-body-${col.agent}`} class="border-t border-[var(--color-border)] px-4 py-3">
            <!-- provider 勾选列表。每个占满一行，名称和协议在同一行的左右两端 -->
            <div class="flex flex-col gap-2">
              {#each matrix.providers as p (p.id)}
                {@const on = chosen.some((c) => c.id === p.id)}
                {@const kindFor = effectiveKind(p, col.agent)}
                <button
                  type="button"
                  class="flex items-center gap-2 rounded-[var(--radius-md)] border px-3 py-2 text-left transition-colors {on
                    ? 'border-[var(--color-accent)] bg-[var(--color-accent)]/10'
                    : 'border-[var(--color-border-strong)] hover:border-[var(--color-accent)]'}"
                  disabled={!col.available || busy}
                  aria-pressed={on}
                  title={on
                    ? `取消「${p.id}」在 ${col.label}`
                    : `把「${p.id}」用到 ${col.label}`}
                  onclick={() => toggleProviderForAgent(p.id, col.agent)}
                >
                  <span
                    class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border {on
                      ? 'border-[var(--color-accent)] bg-[var(--color-accent)]'
                      : 'border-[var(--color-border-strong)]'}"
                  >
                    {#if on}
                      <Check size={10} class="text-white" />
                    {/if}
                  </span>
                  <span class="min-w-0 flex-1 truncate text-[12px] font-medium text-[var(--color-text)]">
                    {p.id}
                  </span>
                  <span class="shrink-0 text-[11px] text-[var(--color-text-faint)]">
                    {kindFor}{#if p.keyLen === 0} · 无 key{/if}
                  </span>
                </button>
              {/each}
            </div>

            <!-- 这一家的特殊入口 + 单槽位说明。都放在勾选列表下面。
                 按钮在前、说明文字在后 —— 操作是主体，说明是补充；
                 按钮也做成带边框的，侧栏那种纯文字链接在内容区太不显眼 -->
            <div class="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-2">
              <!-- 特殊操作统一放在这个位置，不再塞进标题行 -->
              {#if col.agent === 'claude'}
                <button
                  type="button"
                  class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                  title="Claude Code 的首次引导跳过标记"
                  onclick={() => onGoClaudeCodex()}
                >
                  首次引导设置
                </button>
                {#if chosen.length > 0}
                  <button
                    type="button"
                    class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                    title="为 opus / sonnet / haiku / 默认指定模型"
                    onclick={() => {
                      const cur = chosen[0];
                      if (!cur) return;
                      claudeMapDialog = {
                        providerId: cur.id,
                        pendingAgents: [...(matrix?.providers.find((p) => p.id === cur.id)?.agents ?? [])],
                        map: suggestClaudeMap(cur),
                        mode: 'edit',
                      };
                    }}
                  >
                    配置档位映射
                  </button>
                {/if}
              {:else if col.agent === 'codex'}
                {#if chosen.length > 0}
                  <button
                    type="button"
                    class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                    title="Codex 的模型选择器只列官方模型，自定义 provider 的模型只能在这里选"
                    onclick={() => {
                      const cur = chosen[0];
                      if (!cur) return;
                      codexModelDialog = { providerId: cur.id, model: cur.codexModel ?? '' };
                    }}
                  >
                    配置模型
                  </button>
                {/if}
                <button
                  type="button"
                  class="rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
                  title="requires_openai_auth：是否绕过 ChatGPT 后端"
                  onclick={() => onGoClaudeCodex()}
                >
                  后端开关
                </button>
              {/if}

              {#if single}
                <span class="text-[11px] text-[var(--color-text-faint)]">
                  单槽位：勾选另一个会自动取消当前的。
                  {#if col.agent === 'claude'}
                    Claude Code 只有一组 <code class="font-[var(--font-mono)]">ANTHROPIC_*</code> 环境变量。
                  {:else if col.agent === 'codex'}
                    Codex 一次只认一张 <code class="font-[var(--font-mono)]">model_providers</code> 表。
                  {:else}
                    这类 agent 同一时刻只能有一个 provider 生效。
                  {/if}
                </span>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/each}

    <!--
      Cursor 单独一张卡，和上面的 agent 卡片同形。
      它不在 provider 矩阵里（没有 provider 列表的概念，只有一个开关加一个地址），
      所以只能手写在最后，而不是从 matrix.columns 循环出来。
    -->
    {#if cursorAvailable}
      <div class="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)]">
        <div class="flex items-center gap-3 px-4 py-3">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-bg-muted)]"
          >
            <Terminal size={15} class="text-[var(--color-text-muted)]" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-[13px] font-medium text-[var(--color-text)]">Cursor</span>
              <Badge tone="info">自带密钥</Badge>
            </div>
            <div class="text-[11px] text-[var(--color-text-faint)]">
              配置在 SQLite 库里，不走「应用到各 agent」
            </div>
          </div>
        </div>
        <div class="border-t border-[var(--color-border)] px-4 py-3">
          <CursorByok {providers} bind:providerId={cursorProviderId} compact onDone={() => load()} />
        </div>
      </div>
    {/if}
  </div>

  <div class="mt-2 text-[11px] text-[var(--color-text-faint)]">
    共 {matrix.providers.length} 个 provider，{matrix.columns.filter((c) => c.available).length}
    个可用 agent。这里按 agent 分组，改的仍是同一份分配关系。
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

{#if codexModelDialog}
  {@const dlg = codexModelDialog}
  {@const p = matrix?.providers.find((x) => x.id === dlg.providerId)}
  {@const modelOptions = p?.models.map((m) => m.id).filter(Boolean) ?? []}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="codex-model-title"
    onclick={(e) => {
      if (e.target === e.currentTarget) codexModelDialog = null;
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape') codexModelDialog = null;
    }}
  >
    <div
      class="w-full max-w-md rounded-[var(--radius-lg)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] p-4 shadow-lg"
    >
      <h2 id="codex-model-title" class="text-[14px] font-medium text-[var(--color-text)]">
        配置 Codex 用哪个模型
      </h2>
      <p class="mt-1 text-[12px] leading-relaxed text-[var(--color-text-muted)]">
        Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型进不去。
        选中后会写进母版 <code class="text-[11px]">codex_model</code>，再落到
        <code class="text-[11px]">~/.codex/config.toml</code> 顶层的
        <code class="text-[11px]">model</code> —— Codex 不校验这个名字，原样发给上游。
      </p>

      {#if modelOptions.length === 0}
        <p class="mt-3 text-[12px] text-[var(--color-danger)]">
          这个 provider 还没有模型。请先在「provider 管理」里加上模型。
        </p>
      {:else}
        <label class="mt-4 block">
          <span class="field-label">模型</span>
          <select
            class="field-control w-full"
            value={dlg.model}
            onchange={(e) => {
              if (!codexModelDialog) return;
              codexModelDialog = {
                ...codexModelDialog,
                model: (e.currentTarget as HTMLSelectElement).value,
              };
            }}
          >
            <option value="">不指定（沿用第一个：{modelOptions[0]}）</option>
            {#each modelOptions as mid}
              <option value={mid}>{mid}</option>
            {/each}
          </select>
        </label>

        <p class="mt-2 text-[11px] text-[var(--color-text-faint)]">
          改完立即重写 <code class="font-[var(--font-mono)]">config.toml</code>。
          Codex 正在运行时改动会在下次启动生效。
        </p>
      {/if}

      <div class="mt-4 flex justify-end gap-2">
        <Button variant="ghost" onclick={() => (codexModelDialog = null)}>取消</Button>
        <Button
          variant="primary"
          disabled={busy || modelOptions.length === 0}
          onclick={confirmCodexModel}
        >
          保存
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
    line-height: 34px;
    color: var(--color-text);
  }

  /* 协议固定、不能下拉的那一格，文字要和选择框同一条中线。 */
  :global(span.field-control) {
    display: flex;
    align-items: center;
    line-height: 1.4;
  }

  /* 去掉系统自带的下拉按钮，否则在 WebView 里箭头会压到圆角边框上。 */
  :global(select.field-control) {
    appearance: none;
    -webkit-appearance: none;
    padding-right: 32px;
    background-color: var(--color-bg);
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' fill='none' viewBox='0 0 16 16'%3E%3Cpath stroke='%23999999' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M4 6l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    background-size: 16px 16px;
  }

  :global(.field-control:disabled) {
    opacity: 0.6;
  }
</style>
