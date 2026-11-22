/**
 * 前后端共享的数据契约。
 *
 * 这里的每个类型都对应 Rust 侧 serde 的序列化结构。
 * 改这里的时候必须同步改 `src-tauri/src/types.rs`，否则界面会拿到 undefined。
 */

/** 链接的实际落地方式。auto 只出现在配置里，实际结果一定是后四种之一。 */
export type LinkKind = 'symlink' | 'junction' | 'hardlink' | 'copy';

/** 单条映射的健康状态。 */
export type HealthState = 'ok' | 'drifted' | 'broken' | 'missing';

/**
 * 一个 agent 的指令文档现状：通用片段 + 专用片段 + 合成产物。
 *
 * 有专用片段的 agent 读的是「通用 + 专用」拼出来的合成文件；
 * 没有的仍直接读通用母版。界面用它渲染片段选项卡和「待生成」提示。
 */
export interface AgentDoc {
  /** agent 标识，如 cursor */
  agent: string;
  /** 展示名 */
  label: string;
  /** 通用母版路径 */
  commonPath: string;
  /** 该 agent 的指令文件最终落点，如 ~/.cursor/AGENTS.md */
  target: string;
  /** 专用片段路径，无论存不存在都给出来，方便界面做「新建」 */
  specificPath: string;
  /** 专用片段是否存在。false 表示这个 agent 直接读通用母版 */
  specificExists: boolean;
  /** 合成产物路径。没有专用片段时为 null */
  composedPath: string | null;
  /** 合成产物与当前片段内容不一致，需要重建 */
  composedStale: boolean;
  /** 这个 agent 最终会读到的字节数 */
  bytes: number;
  /** 该 agent 的配置目录是否存在。false 表示工具未安装 */
  installed: boolean;
}

/** 一个 agent 的指令文件映射及其当前状态。 */
export interface Mapping {
  /** agent 标识，如 claude / zcode / cursor */
  agent: string;
  /** 展示用名称，如 "Claude Code" */
  label: string;
  /** 母版路径，如 ~/.agents/AGENTS.md */
  source: string;
  /** 目标路径，如 ~/.claude/CLAUDE.md */
  target: string;
  /** 配置里要求的方式 */
  mode: 'auto' | LinkKind;
  /** 实测落地的方式；目标不存在时为空 */
  actual: LinkKind | null;
  /** 健康状态 */
  health: HealthState;
  /** 该 agent 的配置目录是否存在。false 表示工具未安装，同步时会跳过 */
  installed: boolean;
  /** 用户取消了这个映射。为 true 时目标已删掉，同步也不会再建回来 */
  disabled: boolean;
  /** 补充说明，如 "内容与母版不同" / "链接目标不存在" */
  note: string | null;
}

/** 一个已收集的 skill。 */
export interface Skill {
  /** 目录名，也是唯一标识 */
  name: string;
  /** SKILL.md 里 frontmatter 的 description，没写则为空串 */
  description: string;
  /** 标签列表 */
  tags: string[];
  /** 是否由 npx skills 管理（在 .skill-lock.json 里有记录） */
  managed: boolean;
  /** 受管时的来源仓库，如 "MiniMax-AI/skills" */
  source: string | null;
  /** 内容指纹，取前 8 位展示 */
  hash: string;
  /** 目录内文件数 */
  fileCount: number;
  /** 目录字节数 */
  sizeBytes: number;
  /** 归属的 agent 列表。空数组 = 公共 skill，所有 agent 都能读 */
  agents: string[];
  /** 专用 skill 的本体目录，如 ~/.agents/skills-mapping-cursor/issue-scoper */
  storePath: string | null;
  /** 专用 skill 的链接健康状态。公共 skill 恒为 'ok' */
  linkState: PrivateLinkState;
}

/** 专用 skill 的链接健康状态。 */
export type PrivateLinkState = 'ok' | 'broken' | 'missing';

/** 专用 skill 在某个 agent 目录里的一处链接现场。 */
export interface PrivateLinkView {
  /** agent 标识 */
  agent: string;
  /** agent 展示名 */
  label: string;
  /** 该 agent 的 skills 目录里这个 skill 的位置 */
  path: string;
  /** 这一处的状态 */
  state: PrivateLinkState;
  /** 实际链接方式，不是链接时为空 */
  actual: LinkKind | null;
  /** 补充说明，如 "链接指向 xx，不是专用库那一份" */
  note: string | null;
}

/** 一条专用 skill 记录的完整视图。 */
export interface SkillMappingView {
  skill: string;
  agents: string[];
  /** 本体所在目录 */
  storePath: string;
  /** 本体是否存在 */
  storeExists: boolean;
  /** 各 agent 的链接状态 */
  links: PrivateLinkView[];
  /** 整体状态：全部 ok 才 ok，否则取最差的那个 */
  state: PrivateLinkState;
  /** 配置里要求的链接方式 */
  mode: 'auto' | LinkKind;
}

/** 散落在别的 agent 目录里、尚未收进 ~/.agents 的 skill。 */
export interface OrphanSkill {
  name: string;
  /** 出现在哪个 agent 目录，如 ".cursor/skills" */
  location: string;
  /** 哪个 agent 的目录，如 "cursor"。为 null 时不能标记专用 */
  agent: string | null;
  /** 完整路径 */
  path: string;
  /** 与中心库同名 skill 的比对结果 */
  relation: 'unique' | 'identical' | 'conflict' | 'broken-link';
  /** 散落这一份的 SKILL.md 描述。冲突时用来和中心库那份对照 */
  description: string;
  /** 目录内文件数 */
  fileCount: number;
  /** 目录字节数 */
  sizeBytes: number;
  /** 内容指纹前 8 位 */
  hash: string;
}

/**
 * 用户对一条散落 skill 选定的处置方式。
 *
 * 每个 relation 可用的动作：
 * - unique        → import（移入中心库）/ privatize（标记为当前 agent 专用）
 *                   / delete（删掉）
 * - identical     → dedupe（用中心库那份，原位置建链接）/ delete
 * - conflict      → take-orphan（采用散落版）/ take-shared（采用中心库版）/ delete
 * - broken-link   → remove-link
 *
 * privatize 只对 unique 开放：中心库已有同名 skill 时，那份是公共的，
 * 把它标成专用等于拿走别人也在用的东西。
 */
export type OrphanAction =
  | 'import'
  | 'privatize'
  | 'dedupe'
  | 'take-orphan'
  | 'take-shared'
  | 'delete'
  | 'remove-link';

/** 一条处置的执行结果。 */
export interface OrphanActionResult {
  name: string;
  path: string;
  action: OrphanAction;
  ok: boolean;
  /** 人话说明，成功或失败都写在这里 */
  message: string;
}

/** 一次 collect 操作的完整计划，dry-run 时就是这个结构。 */
export interface CollectPlan {
  /** 会移入中心库的 */
  toImport: OrphanSkill[];
  /** 内容相同、只删副本的 */
  toDedupe: OrphanSkill[];
  /** 内容冲突、需要人工决定的 */
  conflicts: OrphanSkill[];
  /** 坏链接，只报告 */
  broken: OrphanSkill[];
}

/** git 状态快照。 */
export interface GitStatus {
  /** 是否为 git 仓库 */
  isRepo: boolean;
  branch: string;
  /** 相对上游领先的提交数 */
  ahead: number;
  /** 落后上游的提交数 */
  behind: number;
  /** 有未提交改动的路径 */
  changed: GitChange[];
  /** 远端地址，没有则为空串 */
  remote: string;
}

export interface GitChange {
  path: string;
  /** M 修改 / A 新增 / D 删除 / ?? 未跟踪 */
  status: string;
}

/** npx skills 的受管 skill 快照。 */
export interface ManagedSkill {
  name: string;
  source: string;
  sourceType: string;
  /** 分支/标签/SHA，没有则为空 */
  ref: string | null;
  /** 内容指纹前 8 位 */
  hashShort: string;
  installedAt: string;
  updatedAt: string;
  /** 是否检测到可用更新；null 表示未检查 */
  hasUpdate: boolean | null;
}

/** 命令执行结果，用于把 CLI 的原始输出透传给界面日志区。 */
export interface RunResult {
  ok: boolean;
  exitCode: number;
  /** 合并后的 stdout + stderr，保留原始顺序不可得时先 stdout 后 stderr */
  output: string;
}

// ---- API key 体检 ----
//
// 注意：这组类型里没有任何密钥字段。密钥原文从不跨进程边界传输。

/** 一条凭证的体检信息。 */
export interface CredentialEntry {
  /** 哪个 agent 在用 */
  agent: string;
  /** 归一化后的服务商标识 */
  provider: string;
  /** api_key 或 oauth */
  kind: string;
  /** 具体落在哪个文件、哪个字段 */
  location: string;
  /** 密钥指纹前 8 位 */
  fingerprint: string;
  /** 密钥字符数 */
  length: number;
  /** 密钥末尾 4 位，帮助人辨认 */
  tail: string;
  /** 同一个指纹还出现在哪些别的 agent 上 */
  sharedWith: string[];
  /** 是否在多个 agent 上重复 */
  duplicated: boolean;
}

/** 读不到明文、需要用户自己登录的 agent。 */
export interface UnsupportedAgent {
  agent: string;
  reason: string;
}

/** 一次完整体检的结果。 */
export interface CredentialOverview {
  /** 按 provider 分组后的全部凭证 */
  providers: CredentialEntry[][];
  /** 读不到明文的 agent */
  unsupported: UnsupportedAgent[];
  /** 凭证总条数 */
  totalEntries: number;
  /** 不同的 key 数量 */
  uniqueKeys: number;
  /** 重复的 key 组数 */
  duplicatedKeys: number;
  /** 配了多把不同 key 的 provider 数量 */
  providersWithMultipleKeys: number;
}

// ---- provider 统一管理 ----

/** 模型价格（每百万 token）。目前只有 Pi 的配置里有。 */
export interface ModelCost {
  input?: number;
  output?: number;
  cacheRead?: number;
  cacheWrite?: number;
}

/**
 * 一个模型的能力描述。
 *
 * 除 id 外全部可选。留空表示「不指定，沿用 agent 配置里的现有值或内置默认」。
 * 这样只写了 id 的存量母版升级后行为不变。
 */
export interface ModelSpec {
  id: string;
  /** 展示名。留空时用 id */
  name?: string;
  /** 上下文窗口 */
  contextWindow?: number;
  /** 单次最大输出 token */
  maxOutputTokens?: number;
  /** 能接受的输入类型：text / image / video / pdf */
  inputModalities: string[];
  /** 能产出的输出类型 */
  outputModalities: string[];
  /** 是否支持推理 */
  reasoning?: boolean;
  /** 可选推理档位：low / medium / high / max … */
  reasoningEfforts: string[];
  /** 默认推理档位 */
  defaultEffort?: string;
  /** 是否支持函数调用 */
  toolCall?: boolean;
  cost?: ModelCost;
}

/**
 * Claude Code 档位 → 本 provider 模型 id。
 *
 * 对应 settings.json 里的 ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL
 * 与 ANTHROPIC_MODEL。字段名与母版一致：default 不是 defaultModel。
 */
export interface ClaudeModelMap {
  opus?: string;
  sonnet?: string;
  haiku?: string;
  /** 对应 ANTHROPIC_MODEL */
  default?: string;
}

/** 母版里的一条 provider。不含密钥原文，只有长度和末尾。 */
export interface ProviderView {
  id: string;
  name: string;
  baseUrl: string;
  /** 默认协议标识：anthropic / openai-chat / openai-responses / google-generative-ai */
  kind: string;
  /** 协议展示名，如 "OpenAI Responses" */
  kindLabel: string;
  /**
   * 按 agent 覆盖的协议。key 是 agent id，value 是协议标识。
   *
   * 协议不是 provider 的属性而是 provider × agent 的：同一个端点
   * 在 ZCode 里可能是 anthropic、在 Pi 里是 openai-responses。
   * 这里只列和默认值不同的那些。
   */
  agentKinds: Record<string, string>;
  /** 模型列表，带能力元数据 */
  models: ModelSpec[];
  agents: string[];
  /** Claude Code 档位映射；未配置时为空对象 */
  claudeModelMap: ClaudeModelMap;
  /**
   * Codex 是否绕过 ChatGPT 后端（写 `requires_openai_auth`）。
   *
   * null 表示交给注入器按盘上登录态自动判断；true / false 是用户强制指定。
   */
  codexRequiresOpenaiAuth: boolean | null;
  /**
   * 分配给 Codex 时写进 `config.toml` 顶层 `model` 的模型 id。
   *
   * Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型进不去，
   * 所以只能靠这个字段决定用哪个。null 表示回退到 `models` 的第一个。
   */
  codexModel: string | null;
  note: string;
  /**
   * key 明文原文。
   *
   * 母版文件本身就是明文，界面又是本机 WebView，所以直接给原文 ——
   * 编辑框能显示当前 key，不用凭末尾 4 位猜。
   */
  apiKey: string;
  /** key 长度，0 表示没配 */
  keyLen: number;
  /** key 末尾 4 位 */
  keyTail: string;
}

/**
 * Claude Code 首次启动引导（onboarding）开关的状态。
 *
 * 对应 `~/.claude.json` 根上的 `hasCompletedOnboarding`。这个标记和
 * provider 无关，是「这台机器上的 Claude Code 是否已完成引导」。
 */
export interface ClaudeOnboardingStatus {
  /** 实际读写的文件路径 */
  path: string;
  /** Claude Code 装没装 */
  available: boolean;
  /** 当前是否已跳过引导 */
  skipped: boolean;
}

/** 一条差异。 */
export interface ProviderDiffLine {
  agent: string;
  path: string;
  action: 'add' | 'update' | 'remove' | 'unchanged' | 'conflict';
  target: string;
  detail: string;
}

/** provider 差异报告。 */
export interface ProviderDiffReport {
  lines: ProviderDiffLine[];
  conflicts: string[];
  agentCount: number;
  changedFiles: number;
}

/** provider 同步结果。 */
export interface ProviderSyncReport {
  ok: boolean;
  written: string[];
  skipped: string[];
  backups: string[];
  log: string[];
}

/** provider 分配矩阵：provider 池 × agent，和 MCP 那份同形。 */
export interface ProviderMatrix {
  columns: AgentColumn[];
  providers: ProviderView[];
  /** 池子是空的 */
  empty: boolean;
}

// ---- Cursor 的自带密钥（BYOK） ----
//
// Cursor 的配置在 SQLite 库 `state.vscdb` 里，不在任何文本文件，也不参与
// `providers sync`。所以单独一组类型和命令。同样不含密钥原文。

/** Cursor 当前的 BYOK 配置。 */
export interface CursorByokState {
  /** Cursor 库文件存在 */
  installed: boolean;
  /** `Use OpenAI API Key` 开关。false 时自定义模型一定用不了 */
  useOpenaiKey: boolean;
  /** 当前接口地址；null 表示没覆盖，走 OpenAI 官方 */
  baseUrl: string | null;
  /** 库里存着的密钥长度。0 表示没存过 */
  keyLen: number;
  /** 密钥末尾 4 位 */
  keyTail: string;
  /** 已添加的自定义模型名 */
  userAddedModels: string[];
  /** 库文件路径 */
  dbPath: string;
  /** Cursor 进程是否在运行。运行中不允许写入 */
  running: boolean;
}

/** 要让某个 provider 在 Cursor 里生效，还差哪些改动。 */
export interface CursorByokPlan {
  /** 要开关总开关时给出目标值 */
  setUseOpenaiKey: boolean | null;
  /** 要改接口地址时给出目标值 */
  setBaseUrl: string | null;
  /** 要不要写密钥 */
  writeKey: boolean;
  /** 要加进模型选择器的名字 */
  addModels: string[];
  /** 要从模型选择器里删掉的名字 */
  removeModels: string[];
  /** 面向人的说明，一行一条 */
  summary: string[];
  /** 什么都不用做 */
  noop: boolean;
}

/** 一条模型回填记录。 */
export interface BackfillLine {
  provider: string;
  model: string;
  /** 从哪个 agent 读到的 */
  source: string;
  /** 填了哪些字段，如 "context_window=1000000" */
  filled: string[];
}

/** 协议不一致：母版里的协议和某个 agent 配置里实际写的不一样。 */
export interface KindMismatch {
  provider: string;
  agent: string;
  /** 母版要求的协议 */
  master: string;
  /** agent 配置里实际的协议 */
  actual: string;
}

/** 模型元数据回填报告。 */
export interface BackfillReport {
  /** 一共补了多少个字段 */
  filledFields: number;
  lines: BackfillLine[];
  /** 母版里没有对应端点的 provider（不导入，只报告） */
  skippedProviders: string[];
  /** 协议不一致的条目。只报告，不自动改 */
  kindMismatches: KindMismatch[];
  /** 是否真的写了母版 */
  wrote: boolean;
}

// ---- MCP 统一管理 ----

/** 母版里的一条 MCP server。env/headers 只返回值名，不返回值。 */
export interface McpServerView {
  id: string;
  name: string;
  /** stdio / sse / http */
  transport: string;
  command: string;
  args: string[];
  cwd: string;
  url: string;
  /** 环境变量名列表 */
  envKeys: string[];
  /** 请求头名列表 */
  headerKeys: string[];
  enabled: boolean;
  agents: string[];
  note: string;
}

/** MCP 差异行。 */
export interface McpDiffLineView {
  agent: string;
  path: string;
  /** add / update / unchanged */
  action: string;
  target: string;
  detail: string;
}

/** MCP 差异报告。 */
export interface McpDiffReportView {
  lines: McpDiffLineView[];
  agentCount: number;
  changedFiles: number;
}

/** MCP 同步结果。 */
export interface McpSyncReportView {
  ok: boolean;
  written: string[];
  skipped: string[];
  backups: string[];
  log: string[];
}

/** 矩阵的列：一个 agent。 */
export interface AgentColumn {
  agent: string;
  label: string;
  /** 配置文件是否存在 */
  available: boolean;
  /** 读不到的原因 */
  note: string;
  /** 单槽位：同一时刻只能有一个条目在它上面生效。界面点选时要自动摘掉其他的 */
  singleSlot: boolean;
  /**
   * 这个 agent 支持哪些协议。空数组 = 全部四种。
   *
   * 界面给某个 agent 选协议时只列这些 —— Codex 只有 responses、
   * Claude 只有 anthropic，列别的等于骗人。
   */
  supportedKinds: string[];
}

/** 矩阵的行：一个 MCP server。 */
export interface MatrixRow {
  id: string;
  name: string;
  transport: string;
  /** 命令或 URL */
  target: string;
  enabled: boolean;
  /** 已分配给哪些 agent */
  assigned: string[];
  note: string;
}

/** 完整的分配矩阵。 */
export interface McpMatrix {
  columns: AgentColumn[];
  rows: MatrixRow[];
  /** 池子是空的 */
  empty: boolean;
}

// ---- token 用量统计 ----

/**
 * 一条按 天 × agent × 模型 聚合的用量。
 *
 * `input` 是归一后的输入量，**含缓存命中**。四个 agent 的原始口径不一致
 * （pi 与 claude 的 input 不含缓存，zcode 与 codex 的含），
 * 后端已经统一折算，界面直接相加不会算错。
 */
export interface UsageRow {
  /** 本地日期，形如 2026-11-12 */
  date: string;
  agent: string;
  model: string;
  /** 输入 token，含缓存命中 */
  input: number;
  output: number;
  /** 模型调用次数，不含失败和空回包 */
  calls: number;
}

/** 一个 agent 的统计状态。 */
export interface UsageAgentStatus {
  agent: string;
  label: string;
  /** 有没有实现用量采集。false 时开关不可点 */
  supported: boolean;
  /** 用户是否勾选了纳入统计 */
  enabled: boolean;
  /** 数据源在不在 */
  available: boolean;
  /** 数据源位置 */
  source: string;
  /** 口径说明或已知限制 */
  note: string;
  /** 本次读到的记录条数 */
  records: number;
  /** 读取失败原因。空串表示正常 */
  error: string;
}

/** 用量统计结果。 */
export interface UsageReport {
  /** 统计窗口天数，0 = 不限 */
  days: number;
  agents: UsageAgentStatus[];
  /** 按天聚合的明细，日期倒序 */
  rows: UsageRow[];
  /** 折线图与饼图的数据 */
  charts: UsageCharts;
}

/** 折线图的一个点：某天某模型的总量。 */
export interface TrendPoint {
  /** 本地日期 2026-11-12 */
  date: string;
  model: string;
  /** 输入 + 输出 */
  tokens: number;
}

/** 饼图的一块。 */
export interface ModelShare {
  model: string;
  /** 输入 + 输出 */
  tokens: number;
  /** 占总量的百分比，0~100 */
  percent: number;
  calls: number;
}

/** 图表数据。 */
export interface UsageCharts {
  /** 折线图每条线的名称，按总量降序 */
  models: string[];
  trend: TrendPoint[];
  /** 饼图数据，按总量降序 */
  shares: ModelShare[];
  /** 折线图横轴日期，升序 */
  dates: string[];
}
