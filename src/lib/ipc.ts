/**
 * 与 Rust 后端通信的统一入口。
 *
 * 两种运行环境：
 * 1. 在 Tauri 窗口里 —— 走 `invoke` 调用真实后端
 * 2. 在普通浏览器里（`npm run dev`）—— 走 `mock.ts` 的假数据
 *
 * 这样前端可以脱离 Rust 工具链独立开发：本机还没装 Rust 和 MSVC 时，
 * 光靠 `npm run dev` 就能把界面调完。
 */
import type {
  AgentDoc,
  BackfillReport,
  ClaudeModelMap,
  ClaudeOnboardingStatus,
  CollectPlan,
  CredentialOverview,
  CursorByokPlan,
  CursorByokState,
  GitStatus,
  ManagedSkill,
  Mapping,
  McpDiffReportView,
  McpMatrix,
  McpServerView,
  McpSyncReportView,
  ModelSpec,
  OrphanAction,
  OrphanActionResult,
  OrphanSkill,
  ProviderDiffReport,
  ProviderMatrix,
  ProviderSyncReport,
  ProviderView,
  RunResult,
  Skill,
  SkillMappingView,
  UsageReport,
} from './types';

/** Tauri 注入的全局对象存在与否，是判断当前运行环境的唯一可靠依据。 */
const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 动态加载 mock，避免 Tauri 环境下把假数据打进产物。 */
async function mock() {
  return await import('./mock');
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<T>(command, args);
  }
  const m = await mock();
  return (await m.handle(command, args)) as T;
}

/** 当前是否运行在 Tauri 窗口里。界面用这个决定是否灰掉需要真后端的按钮。 */
export const isNative = inTauri;

// ---- 指令映射 ----

/** 读取全部映射及其健康状态。 */
export const listMappings = () => call<Mapping[]>('list_mappings');

/** 执行同步。dryRun 为 true 时只返回计划不写盘。 */
export const syncMappings = (dryRun: boolean) =>
  call<RunResult>('sync_mappings', { dryRun });

/** 只修复指定的一个映射。 */
export const repairMapping = (agent: string) =>
  call<RunResult>('repair_mapping', { agent });

/**
 * 取消某个 agent 的映射：删掉目标位置的链接或副本，并记下「已取消」。
 *
 * 必须记标记，否则下一次「同步全部」会立刻把链接建回来。
 * 目标内容与母版不一致时会被移进 `~/.agents/backup/` 而不是直接删。
 */
export const unmap = (agent: string) => call<RunResult>('unmap', { agent });

/** 恢复被取消的映射：清掉标记并立刻重建链接。 */
export const remap = (agent: string) => call<RunResult>('remap', { agent });

// ---- 母版文档 ----

/** 母版文件的读取结果。exists 为 false 时 content 是空串。 */
export interface DocContent {
  exists: boolean;
  content: string;
}

/** 读取母版 AGENTS.md 的内容。 */
export const readDoc = () => call<DocContent>('read_doc');

/** 覆盖写入母版 AGENTS.md。 */
export const saveDoc = (content: string) => call<RunResult>('save_doc', { content });

/** 创建母版文件，已存在时拒绝覆盖。 */
export const createDoc = (content: string) => call<RunResult>('create_doc', { content });

// ---- 每个 agent 的专用指令片段 ----
//
// 有专用片段的 agent 读的是「通用 + 专用」的合成产物；没有的仍直接读通用母版。

/** 列出每个 agent 的片段与合成状态。纯只读。 */
export const listAgentDocs = () => call<AgentDoc[]>('list_agent_docs');

/** 读某个 agent 的专用片段。没有片段时 exists 为 false。 */
export const readAgentDoc = (agent: string) => call<DocContent>('read_agent_doc', { agent });

/**
 * 写某个 agent 的专用片段。
 *
 * 内容为空（trim 后）表示取消这个 agent 的专用内容：删掉片段文件，
 * 之后它回到直接读通用母版。写之前旧版会备份到 ~/.agents/backup/。
 */
export const saveAgentDoc = (agent: string, content: string) =>
  call<RunResult>('save_agent_doc', { agent, content });

/**
 * 重建所有 agent 的合成指令文档。
 *
 * dryRun 为真时只报告哪些落后了。`syncMappings` 内部也会先跑一遍这个，
 * 所以一般不需要单独调用。
 */
export const composeAgentDocs = (dryRun = false) =>
  call<RunResult>('compose_agent_docs', { dryRun });

// ---- skills ----

/** 列出中心库里的所有 skill。 */
export const listSkills = () => call<Skill[]>('list_skills');

/** 覆盖某个 skill 的标签。 */
export const setSkillTags = (name: string, tags: string[]) =>
  call<RunResult>('set_skill_tags', { name, tags });

/** 扫描散落在各 agent 目录里的 skill，返回处理计划。 */
export const scanOrphans = () => call<OrphanSkill[]>('scan_orphans');

/** 执行收集。apply 为 false 时只返回计划。 */
export const collectSkills = (apply: boolean) =>
  call<CollectPlan>('collect_skills', { apply });

/**
 * 对一条散落 skill 执行用户选定的处置。
 *
 * 与 collectSkills 的区别：那个按关系套固定策略批量处理，这个由用户在
 * 散落清单里逐条点。可能移动或删除磁盘上的目录。
 *
 * 选 `privatize` 时，后端会从路径反推所属 agent，把本体移进
 * `~/.agents/skills-mapping-<agent>/` 并写一条归属记录。
 */
export const applyOrphanAction = (path: string, action: OrphanAction) =>
  call<OrphanActionResult>('apply_orphan_action', { path, action });

// ---- 专用 skill ----

/** 列出全部专用 skill 记录及每条链接的现场状态。纯只读。 */
export const listSkillMappings = () => call<SkillMappingView[]>('list_skill_mappings');

/**
 * 重建专用 skill 的链接。幂等，已经是对的链接直接跳过。
 *
 * dryRun 为真时只返回会做什么，不动磁盘。
 */
export const syncSkillMappings = (dryRun = false) =>
  call<string[]>('sync_skill_mappings', { dryRun });

/**
 * 取消一条专用记录。
 *
 * toShared 为真时本体移回公共库 `~/.agents/skills`，变成所有 agent 都能用；
 * 为假时先备份到 `~/.agents/backup/` 再删。两种都会删掉各 agent 目录里的链接。
 */
export const removeSkillMapping = (skill: string, toShared: boolean) =>
  call<RunResult>('remove_skill_mapping', { skill, toShared });

// ---- npx skills ----

/** 读取 lock 文件里的受管 skill 清单。 */
export const listManaged = () => call<ManagedSkill[]>('list_managed');

/** 检查更新。返回原始输出，退出码 1 表示有更新可用。 */
export const checkUpdates = () => call<RunResult>('check_updates');

/** 更新指定 skill，不传则更新全部。 */
export const updateSkills = (names: string[]) =>
  call<RunResult>('update_skills', { names });

// ---- git ----

/** 读取 ~/.agents 的 git 状态。 */
export const gitStatus = () => call<GitStatus>('git_status');

/** 按指定路径提交。绝不使用 git add -A。 */
export const gitCommit = (paths: string[], message: string) =>
  call<RunResult>('git_commit', { paths, message });

/** 推送当前分支。 */
export const gitPush = () => call<RunResult>('git_push');

// ---- API key 体检 ----

/**
 * 体检所有 agent 的 API key 分布。
 *
 * 纯只读：不写任何文件，返回值里只有指纹和末尾 4 位，没有密钥原文。
 */
export const inspectCredentials = () => call<CredentialOverview>('inspect_credentials');

// ---- provider 统一管理 ----

/** 列出母版里的 provider。不含密钥原文。 */
export const listProviders = () => call<ProviderView[]>('list_providers');

/** 保存一条 provider。apiKey 传空串表示不修改现有 key。 */
export const saveProvider = (p: {
  id: string;
  name: string;
  baseUrl: string;
  kind: string;
  apiKey: string;
  models: ModelSpec[];
  agents: string[];
  /** 按 agent 覆盖的协议。只放和 kind 不同的那些 */
  agentKinds: Record<string, string>;
  /** Codex 的 requires_openai_auth 覆盖值；null 表示自动判断 */
  codexRequiresOpenaiAuth: boolean | null;
  /** Codex 用哪个模型；null 表示回退到 models 的第一个 */
  codexModel: string | null;
  note: string;
}) =>
  call<RunResult>('save_provider', {
    id: p.id,
    name: p.name,
    baseUrl: p.baseUrl,
    kind: p.kind,
    apiKey: p.apiKey,
    models: p.models,
    agents: p.agents,
    agentKinds: p.agentKinds,
    codexRequiresOpenaiAuth: p.codexRequiresOpenaiAuth,
    codexModel: p.codexModel,
    note: p.note,
  });

/** 从各 agent 现有配置回填模型元数据。dryRun 时只算报告不写盘。 */
export const backfillModels = (dryRun: boolean) =>
  call<BackfillReport>('backfill_models', { dryRun });

/** 从母版删除一条 provider。 */
export const deleteProvider = (id: string) => call<RunResult>('delete_provider', { id });

/** 对比母版与各 agent 现状。只读。 */
export const diffProviders = () => call<ProviderDiffReport>('diff_providers');

/** 读取 provider 分配矩阵：provider 池 × agent 勾选关系。 */
export const providerMatrix = () => call<ProviderMatrix>('provider_matrix');

/** 整行替换某个 provider 的分配列表。Claude 是单槽位，会做互斥。 */
export const setProviderRow = (
  id: string,
  agents: string[],
  claudeModelMap?: ClaudeModelMap | null,
) => call<RunResult>('set_provider_row', { id, agents, claudeModelMap: claudeModelMap ?? null });

/** 切换某个 provider 在某个 agent 的启用状态。 */
export const setProviderAssignment = (providerId: string, agent: string, on: boolean) =>
  call<RunResult>('set_provider_assignment', { providerId, agent, on });

/** 注入 provider。dryRun 时只显示计划。 */
export const syncProviders = (dryRun: boolean) =>
  call<ProviderSyncReport>('sync_providers', { dryRun });

/** 读取母版原始 TOML。 */
export const readProvidersToml = () => call<string>('read_providers_toml');

/** 直接写入母版 TOML（会先校验格式）。 */
export const writeProvidersToml = (content: string) =>
  call<RunResult>('write_providers_toml', { content });

/** 读 Claude Code 的首次引导状态（`~/.claude.json` 的 hasCompletedOnboarding）。 */
export const claudeOnboardingStatus = () =>
  call<ClaudeOnboardingStatus>('claude_onboarding_status');

/** 设置或清除 Claude Code 的首次引导跳过标记。 */
export const setClaudeOnboardingSkip = (skip: boolean) =>
  call<RunResult>('set_claude_onboarding_skip', { skip });

/**
 * 设置某个 provider 的 Codex 后端开关，并立刻只重写 Codex 的配置。
 *
 * value 为 null 表示交回自动判断。
 */
export const setCodexRequiresOpenaiAuth = (providerId: string, value: boolean | null) =>
  call<RunResult>('set_codex_requires_openai_auth', { providerId, value });

/**
 * 设置某个 provider 分配给 Codex 时用哪个模型，并立刻只重写 Codex 的配置。
 *
 * Codex 的模型选择器只列官方目录，自定义 provider 的模型进不去，所以
 * 「用哪个模型」只能靠 `config.toml` 顶层 `model`。value 为 null 表示
 * 取消指定，回退到 models 的第一个。
 */
export const setCodexModel = (providerId: string, model: string | null) =>
  call<RunResult>('set_codex_model', { providerId, model });

// ---- Cursor 的自带密钥（BYOK） ----
//
// Cursor 的配置在 SQLite 库里、密钥经 safeStorage 加密、写入前必须退出
// Cursor，所以不走 syncProviders，单独三个命令。见 providers/cursor_db.rs。

/** 读 Cursor 当前的 BYOK 状态。不含密钥原文。 */
export const cursorByokStatus = () => call<CursorByokState>('cursor_byok_status');

/** 算要让某个 provider 在 Cursor 里生效还差哪些改动。只读。 */
export const cursorByokPlan = (providerId: string, models: string[] = []) =>
  call<CursorByokPlan>('cursor_byok_plan', { providerId, models });

/**
 * 把某个 provider 写进 Cursor。
 *
 * models 是要出现在 Cursor 模型选择器里的名字。传空数组表示不动模型列表。
 * Cursor 正在运行时会被后端拒绝 —— 它开着时会用内存状态覆盖这次写入。
 */
export const cursorByokApply = (providerId: string, models: string[] = []) =>
  call<RunResult>('cursor_byok_apply', { providerId, models });

/**
 * 从 Cursor 的模型选择器里删掉指定模型。
 *
 * 只改 userAddedModels，不动开关、地址、密钥。
 */
export const cursorByokRemoveModels = (models: string[]) =>
  call<RunResult>('cursor_byok_remove_models', { models });

/** 关掉 Cursor 的自带密钥、清空接口地址。密钥留在库里不清。 */
export const cursorByokReset = () => call<RunResult>('cursor_byok_reset');

// ---- MCP 统一管理 ----

/** 列出母版里的 MCP server。env/headers 只给键名。 */
export const listMcpServers = () => call<McpServerView[]>('list_mcp_servers');

/** 保存一条 MCP server。env/headers 是完整键值对，传空表示清空。 */
export const saveMcpServer = (s: {
  id: string;
  name: string;
  transport: string;
  command: string;
  args: string[];
  cwd: string;
  url: string;
  env: Record<string, string>;
  headers: Record<string, string>;
  enabled: boolean;
  agents: string[];
  note: string;
}) =>
  call<RunResult>('save_mcp_server', {
    id: s.id,
    name: s.name,
    transport: s.transport,
    command: s.command,
    args: s.args,
    cwd: s.cwd,
    url: s.url,
    env: s.env,
    headers: s.headers,
    enabled: s.enabled,
    agents: s.agents,
    note: s.note,
  });

/** 从母版删除一条 MCP server。 */
export const deleteMcpServer = (id: string) => call<RunResult>('delete_mcp_server', { id });

/** 对比母版与各 agent 现状。只读。 */
export const diffMcp = () => call<McpDiffReportView>('diff_mcp');

/** 注入 MCP server。dryRun 时只显示计划。 */
export const syncMcp = (dryRun: boolean) =>
  call<McpSyncReportView>('sync_mcp', { dryRun });

/** 读取 MCP 母版原始 TOML。 */
export const readMcpToml = () => call<string>('read_mcp_toml');

/** 写入 MCP 母版 TOML（会先校验格式）。 */
export const writeMcpToml = (content: string) =>
  call<RunResult>('write_mcp_toml', { content });

/** 读取 MCP 分配矩阵：server 池 × agent 勾选关系。 */
export const mcpMatrix = () => call<McpMatrix>('mcp_matrix');

/** 切换某个 server 在某个 agent 的启用状态。 */
export const setMcpAssignment = (serverId: string, agent: string, on: boolean) =>
  call<RunResult>('set_mcp_assignment', { serverId, agent, on });

/** 整行替换某个 server 的分配列表。 */
export const setMcpRow = (serverId: string, agents: string[]) =>
  call<RunResult>('set_mcp_row', { serverId, agents });

/** 从各 agent 现有配置导入 server 到池子。 */
export const importMcpFromAgents = () => call<RunResult>('import_mcp_from_agents');

// ---- token 用量统计 ----

/**
 * 统计各 agent 的 token 用量。
 *
 * days 是窗口天数，0 表示不限。只有已启用的 agent 才会被读盘，
 * 没启用的连数据源都不碰。
 *
 * refresh 为真时丢掉全部缓存从头重算。平时统计是增量的：只读会话文件
 * 新追加的字节、只查 Cursor 库里新插入的行，所以第二次打开几乎是瞬时的。
 *
 * 异步执行：全量重算要读 GB 级的库（pi 595 MB + Cursor 4.2 GB），
 * 耗时几十秒；增量通常不到半秒。
 */
export const usageReport = (days: number, refresh = false) =>
  call<UsageReport>('usage_report', { days, refresh });

/**
 * 打开或关闭某个 agent 的用量统计。
 *
 * 只写 `~/.agents/agentcenter.toml` 的 `[usage] enabled` 名单，
 * 不读任何用量数据。要看到数字还得再调一次 usageReport。
 */
export const setUsageEnabled = (agent: string, on: boolean) =>
  call<RunResult>('set_usage_enabled', { agent, on });
