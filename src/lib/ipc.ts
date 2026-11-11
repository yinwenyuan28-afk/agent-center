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
  BackfillReport,
  CollectPlan,
  CredentialOverview,
  GitStatus,
  ManagedSkill,
  Mapping,
  McpDiffReportView,
  McpMatrix,
  McpServerView,
  McpSyncReportView,
  ModelSpec,
  OrphanSkill,
  ProviderDiffReport,
  ProviderMatrix,
  ProviderSyncReport,
  ProviderView,
  RunResult,
  Skill,
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
export const setProviderRow = (id: string, agents: string[]) =>
  call<RunResult>('set_provider_row', { id, agents });

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
