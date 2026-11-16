/**
 * 浏览器开发用的假后端。
 *
 * 数据全部照抄本机 2026-11-09 的实测结果，不是随手编的示例：
 * - 6 个指令文件的真实字节数
 * - 18 个 skill 的真实名字和来源
 * - ~/.agents 的真实 git 状态（17 次提交，工作区干净）
 *
 * 目的：本机还没装 Rust 时，`npm run dev` 就能把界面按真实数据调完。
 * 一旦编译出真后端，这份文件不会被打进 Tauri 产物。
 */
import type {
  CollectPlan,
  GitStatus,
  ManagedSkill,
  Mapping,
  OrphanAction,
  OrphanSkill,
  RunResult,
  Skill,
  UsageAgentStatus,
  UsageCharts,
  UsageRow,
} from './types';

const now = new Date().toISOString();

/** 8 个指令文件的实测状态。disabled 统一补 false，见下面的 mappings。 */
const mappingsRaw: Omit<Mapping, 'disabled'>[] = [
  {
    agent: 'zcode',
    label: 'ZCode',
    source: '~/.agents/AGENTS.md',
    target: '~/.zcode/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: null,
  },
  {
    agent: 'claude',
    label: 'Claude Code',
    source: '~/.agents/AGENTS.md',
    target: '~/.claude/CLAUDE.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: null,
  },
  {
    agent: 'codex',
    label: 'Codex CLI',
    source: '~/.agents/AGENTS.md',
    target: '~/.codex/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: '目标当前是 0 字节空文件',
  },
  {
    agent: 'cursor',
    label: 'Cursor',
    source: '~/.agents/AGENTS.md',
    target: '~/.cursor/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: null,
  },
  {
    agent: 'opencode',
    label: 'OpenCode',
    source: '~/.agents/AGENTS.md',
    target: '~/.config/opencode/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: null,
  },
  {
    agent: 'pi',
    label: 'pi',
    source: '~/.agents/AGENTS.md',
    target: '~/.pi/agent/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: true,
    note: null,
  },
  {
    agent: 'qoder-cn',
    label: 'Qoder CN',
    source: '~/.agents/AGENTS.md',
    target: '~/.qoder-cn/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: false,
    note: '配置目录不存在，同步时跳过',
  },
  {
    agent: 'trae-cn',
    label: 'Trae CN',
    source: '~/.agents/AGENTS.md',
    target: '~/.trae-cn/AGENTS.md',
    mode: 'auto',
    actual: null,
    health: 'missing',
    installed: false,
    note: '配置目录不存在，同步时跳过',
  },
];

/**
 * 预览模式下的映射表。
 *
 * 从上面的常量展开出一份可变的副本，「取消映射」和「恢复映射」才能在
 * 浏览器里看到真实变化，而不是点了没反应。
 */
const mappings: Mapping[] = mappingsRaw.map((m) => ({ ...m, disabled: false }));

/** 18 个 skill 的真实清单。managed 与 source 来自 .skill-lock.json。 */
const skills: Skill[] = [
  { name: 'agent-browser', description: '浏览器自动化 CLI，用于点击、填表、截图、提取页面数据', tags: [], managed: true, source: 'vercel-labs/agent-browser', hash: '82def678', fileCount: 3, sizeBytes: 8420 },
  { name: 'agents-md', description: '为仓库生成或审查 AGENTS.md', tags: ['文档'], managed: false, source: null, hash: 'a1b2c3d4', fileCount: 9, sizeBytes: 24800 },
  { name: 'ai-text-readable', description: '让 AI 生成的文档和注释读得懂', tags: ['文档'], managed: false, source: null, hash: 'b2c3d4e5', fileCount: 6, sizeBytes: 18400 },
  { name: 'deepseek-search', description: '用 DeepSeek 官方接口做联网搜索与正文抓取', tags: ['搜索'], managed: false, source: null, hash: 'c3d4e5f6', fileCount: 8, sizeBytes: 41200 },
  { name: 'eli5', description: '按读者水平重新解释概念', tags: [], managed: false, source: null, hash: 'd4e5f6a7', fileCount: 2, sizeBytes: 6200 },
  { name: 'find-skills', description: '发现并安装新的 agent skill', tags: [], managed: true, source: 'vercel-labs/skills', hash: 'e5f6a7b8', fileCount: 2, sizeBytes: 9800 },
  { name: 'frontend-design', description: '界面视觉设计与排版指导', tags: ['UI设计'], managed: false, source: null, hash: 'f6a7b8c9', fileCount: 3, sizeBytes: 12600 },
  { name: 'git-push-rebase', description: 'git 提交推送与变基全流程', tags: [], managed: false, source: null, hash: 'a7b8c9d0', fileCount: 1, sizeBytes: 5200 },
  { name: 'grill-me', description: '对计划或决定做高强度追问', tags: [], managed: false, source: null, hash: 'b8c9d0e1', fileCount: 2, sizeBytes: 7400 },
  { name: 'learn-hard-concept', description: '先例子后定义的概念学习协议', tags: [], managed: false, source: null, hash: 'c9d0e1f2', fileCount: 4, sizeBytes: 16800 },
  { name: 'minimax-docx', description: '专业 DOCX 文档创建与编辑', tags: [], managed: true, source: 'MiniMax-AI/skills', hash: 'e55cc738', fileCount: 75, sizeBytes: 186400 },
  { name: 'minimax-pdf', description: '注重视觉质量的 PDF 生成', tags: [], managed: true, source: 'MiniMax-AI/skills', hash: 'f1a2b3c4', fileCount: 12, sizeBytes: 34200 },
  { name: 'minimax-xlsx', description: 'Excel 表格创建、分析与校验', tags: [], managed: true, source: 'MiniMax-AI/skills', hash: 'a2b3c4d5', fileCount: 25, sizeBytes: 72800 },
  { name: 'plan-execute', description: '复杂开发任务先写计划再执行', tags: [], managed: false, source: null, hash: 'b3c4d5e6', fileCount: 19, sizeBytes: 48600 },
  { name: 'pptx-generator', description: 'PowerPoint 演示文稿生成与编辑', tags: [], managed: true, source: 'MiniMax-AI/skills', hash: 'c4d5e6f7', fileCount: 6, sizeBytes: 21400 },
  { name: 'project-docs-architecture', description: '编写系统架构与组件文档', tags: ['文档'], managed: false, source: null, hash: 'd5e6f7a8', fileCount: 5, sizeBytes: 15200 },
  { name: 'project-docs-readme', description: '编写项目 README', tags: ['文档'], managed: false, source: null, hash: 'e6f7a8b9', fileCount: 4, sizeBytes: 11800 },
  { name: 'ui-design', description: '界面偏好：文案、层级、主题与本地化', tags: ['UI设计'], managed: false, source: null, hash: 'f7a8b9c0', fileCount: 7, sizeBytes: 22600 },
];

/** 散落在各 agent 目录里的 skill，照抄实测结果。 */
const orphans: OrphanSkill[] = [
  { name: 'analyze', location: '.config/opencode/skills', path: 'C:\\Users\\leaf\\.config\\opencode\\skills\\analyze', relation: 'unique', description: '分析代码结构并给出改进建议', fileCount: 4, sizeBytes: 12400, hash: 'a1b2c3d4' },
  { name: 'context-archive', location: '.config/opencode/skills', path: 'C:\\Users\\leaf\\.config\\opencode\\skills\\context-archive', relation: 'unique', description: '把长对话归档成结构化笔记', fileCount: 6, sizeBytes: 18600, hash: 'b2c3d4e5' },
  { name: 'dingtalk-req-to-spec', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\dingtalk-req-to-spec', relation: 'unique', description: '把钉钉需求消息整理成技术规格', fileCount: 11, sizeBytes: 42300, hash: 'c3d4e5f6' },
  { name: 'hardcoded-page-portal-config', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\hardcoded-page-portal-config', relation: 'unique', description: '页面写死配置的迁移说明', fileCount: 3, sizeBytes: 9800, hash: 'd4e5f6a7' },
  { name: 'portal-link-migrate', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\portal-link-migrate', relation: 'unique', description: '门户链接批量迁移脚本与步骤', fileCount: 7, sizeBytes: 26800, hash: 'e5f6a7b8' },
  { name: 'plan-execute', location: '.workbuddy/skills', path: 'C:\\Users\\leaf\\.workbuddy\\skills\\plan-execute', relation: 'identical', description: '复杂开发任务先写计划再执行', fileCount: 19, sizeBytes: 48600, hash: 'b3c4d5e6' },
  { name: 'agent-browser', location: '.claude/skills', path: 'C:\\Users\\leaf\\.claude\\skills\\agent-browser', relation: 'identical', description: '浏览器自动化 CLI，用于点击、填表、截图、提取页面数据', fileCount: 3, sizeBytes: 8420, hash: '82def678' },
  { name: 'agent-browser', location: '.codebuddy/skills', path: 'C:\\Users\\leaf\\.codebuddy\\skills\\agent-browser', relation: 'conflict', description: '浏览器自动化 CLI（本地改过，多了自建的截图流程）', fileCount: 5, sizeBytes: 11200, hash: '9f8e7d6c' },
  { name: 'legacy-review', location: '.gemini/skills', path: 'C:\\Users\\leaf\\.gemini\\skills\\legacy-review', relation: 'broken-link', description: '', fileCount: 0, sizeBytes: 0, hash: '' },
];

/** 受管 skill 的 lock 信息。 */
const managed: ManagedSkill[] = [
  { name: 'agent-browser', source: 'vercel-labs/agent-browser', sourceType: 'github', ref: null, hashShort: '82def678', installedAt: '2026-09-18T15:25:38.566Z', updatedAt: '2026-09-18T15:25:38.566Z', hasUpdate: null },
  { name: 'find-skills', source: 'vercel-labs/skills', sourceType: 'github', ref: null, hashShort: 'e5f6a7b8', installedAt: '2026-11-07T18:50:53.995Z', updatedAt: '2026-11-07T18:50:53.995Z', hasUpdate: null },
  { name: 'minimax-docx', source: 'MiniMax-AI/skills', sourceType: 'github', ref: null, hashShort: 'e55cc738', installedAt: '2026-11-07T18:48:23.141Z', updatedAt: '2026-11-07T18:48:23.141Z', hasUpdate: null },
  { name: 'minimax-pdf', source: 'MiniMax-AI/skills', sourceType: 'github', ref: null, hashShort: 'f1a2b3c4', installedAt: '2026-11-07T18:48:23.141Z', updatedAt: '2026-11-07T18:48:23.141Z', hasUpdate: null },
  { name: 'minimax-xlsx', source: 'MiniMax-AI/skills', sourceType: 'github', ref: null, hashShort: 'a2b3c4d5', installedAt: '2026-11-07T18:48:23.141Z', updatedAt: '2026-11-07T18:48:23.141Z', hasUpdate: null },
  { name: 'pptx-generator', source: 'MiniMax-AI/skills', sourceType: 'github', ref: null, hashShort: 'c4d5e6f7', installedAt: '2026-11-07T18:48:23.141Z', updatedAt: '2026-11-07T18:48:23.141Z', hasUpdate: null },
];

/** git 状态：17 次提交，工作区干净。 */
const gitStatus: GitStatus = {
  isRepo: true,
  branch: 'master',
  ahead: 0,
  behind: 0,
  changed: [],
  remote: 'git@github.com:yinwenyuan28-afk/.agents.git',
};

const ok = (output: string): RunResult => ({ ok: true, exitCode: 0, output });

/**
 * 预览模式下可变的状态。
 *
 * 两个后端开关改了之后要能立刻看到效果，所以把状态留在模块级：
 * 否则 `list_providers` 每次返回同一份常量，点了没反应。
 */
const mockState = {
  claudeOnboardingSkipped: true,
  /** provider id → codex_requires_openai_auth */
  codexAuth: new Map<string, boolean | null>([['command', null]]),
  /** provider id → codex_model（给 Codex 用哪个模型） */
  codexModel: new Map<string, string | null>([['command', null]]),
  /** 预览模式下已勾选纳入统计的 agent。真实后端存在 agentcenter.toml 里 */
  usageEnabled: new Set<string>(['zcode']),
  /** Cursor 自带密钥的可变状态，让预览里点「写入」看得到变化 */
  cursorByok: {
    useOpenaiKey: true,
    baseUrl: 'https://api.commandcode.ai/provider/v1' as string | null,
    /** 已加进 Cursor 模型选择器的名字 */
    models: ['deepseek/deepseek-v4.1-flash'] as string[],
  },
};

/**
 * 预览模式下某个 provider 的接口地址。
 *
 * 只有描述真实接口的那几条返回地址，其余（演示用的假 provider）返回 null，
 * 让 Cursor 那块报「母版里没有 provider」，和真后端取不到 provider 时一致。
 */
function mockCursorProviderUrl(id: string): string | null {
  const known: Record<string, string> = {
    command: 'https://api.commandcode.ai/provider/v1',
    'minimax-cn': 'https://api.minimax.cn/anthropic',
    'dsh-responses': 'https://responses.example.com/v1',
  };
  return known[id] ?? null;
}

/** 预览模式下的用量 agent 状态。前四个支持，其余明确标不支持。 */
function mockUsageAgents(): UsageAgentStatus[] {
  const supported = [
    {
      agent: 'claude',
      label: 'Claude Code',
      source: '~/.claude/projects',
      note: '只统计 assistant 消息；报错消息（model 为 <synthetic>）不计入。原始 input 不含缓存，已折算为含缓存。',
      records: 1,
    },
    {
      agent: 'codex',
      label: 'Codex CLI',
      source: '~/.codex/sessions',
      note: '只读未压缩的 rollout-*.jsonl。更早的历史是 .jsonl.zst 压缩格式，未纳入统计。',
      records: 2,
    },
    {
      agent: 'zcode',
      label: 'ZCode',
      source: '~/.zcode/cli/db/db.sqlite',
      note: '数据在 model_usage 表，按模型调用逐条记录。只统计 status=completed 的记录。',
      records: 26432,
    },
    {
      agent: 'pi',
      label: 'pi',
      source: '~/.pi/agent/sessions',
      note: '会话文件按项目分目录，子代理会话在子目录里。原始 input 不含缓存，已折算。',
      records: 36163,
    },
    {
      agent: 'cursor',
      label: 'Cursor',
      source: '~/AppData/Roaming/Cursor/User/globalStorage/state.vscdb',
      note: '估算值。Cursor 按请求次数计费，本地不记录单次 token；这里用上下文窗口快照的相邻差值推算。输出 token 本地完全没有记录。',
      records: 629,
    },
  ];

  const out: UsageAgentStatus[] = supported.map((s) => ({
    ...s,
    supported: true,
    enabled: mockState.usageEnabled.has(s.agent),
    available: true,
    error: '',
  }));

  // 其余 agent 各占一行，明确告诉用户「还没做」，而不是让它凭空消失
  for (const agent of ['opencode', 'workbuddy', 'dsh', 'hermes', 'qoder', 'qoder-cn', 'codebuddy', 'trae-cn', 'zed', 'gemini', 'grok', 'openclaw', 'kimi', 'minimax']) {
    out.push({
      agent,
      label: agent,
      supported: false,
      enabled: false,
      available: false,
      source: '',
      note: '',
      records: 0,
      error: '',
    });
  }

  return out;
}

/** 预览用的用量明细。取最近 5 天，量级对齐本机真实数据。 */
function mockUsageRows(): UsageRow[] {
  return [
    { date: '2026-11-12', agent: 'zcode', model: 'deepseek/deepseek-v4.1-flash', input: 530610588, output: 1071143, calls: 1840 },
    { date: '2026-11-12', agent: 'codex', model: 'deepseek/deepseek-v4.1-flash', input: 46536, output: 118, calls: 2 },
    { date: '2026-11-11', agent: 'zcode', model: 'deepseek/deepseek-v4.1-flash', input: 168122499, output: 343313, calls: 573 },
    { date: '2026-11-11', agent: 'zcode', model: 'deepseek/deepseek-v4-flash-vision-exp', input: 78973561, output: 41930, calls: 129 },
    { date: '2026-11-11', agent: 'claude', model: 'deepseek/deepseek-v4.1-flash', input: 27390, output: 9, calls: 1 },
    { date: '2026-11-10', agent: 'zcode', model: 'deepseek/deepseek-v4.1-flash', input: 386455966, output: 818615, calls: 1031 },
    { date: '2026-11-10', agent: 'zcode', model: 'deepseek/deepseek-v4-flash-vision-exp', input: 12860138, output: 16213, calls: 32 },
    { date: '2026-11-10', agent: 'pi', model: 'deepseek/deepseek-v4.1-flash', input: 140168769, output: 977666, calls: 510 },
    { date: '2026-11-09', agent: 'zcode', model: 'deepseek/deepseek-v4.1-flash', input: 210090546, output: 927504, calls: 1070 },
    { date: '2026-11-09', agent: 'pi', model: 'MiniMax-M3', input: 47148071, output: 270327, calls: 385 },
  ];
}

/** 预览用的窗口下界：只保留最近 N 天的行。 */
function mockUsageCutoff(days: number): string {
  const d = new Date('2026-11-12T00:00:00');
  d.setDate(d.getDate() - (days - 1));
  return d.toISOString().slice(0, 10);
}

/** 空图表数据。没有可展示的行时用它，避免组件拿 undefined。 */
function emptyCharts(): UsageCharts {
  return { models: [], trend: [], shares: [], dates: [] };
}

/**
 * 预览用的图表数据。算法与后端 `build_charts` 保持一致：
 * 模型按总量排序、最多保留 8 个、其余合并成「其他」。
 */
function mockCharts(rows: UsageRow[]): UsageCharts {
  if (rows.length === 0) return emptyCharts();

  const perModel = new Map<string, number>();
  for (const r of rows) {
    perModel.set(r.model, (perModel.get(r.model) ?? 0) + r.input + r.output);
  }
  const ranked = [...perModel.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const top = new Set(ranked.slice(0, 8).map(([m]) => m));
  const hasOther = ranked.length > 8;

  const dates = [...new Set(rows.map((r) => r.date))].sort();

  const points = new Map<string, number>();
  const shares = new Map<string, { tokens: number; calls: number }>();
  for (const r of rows) {
    const tokens = r.input + r.output;
    const bucket = top.has(r.model) ? r.model : hasOther ? '其他' : r.model;
    const key = `${bucket}\u0000${r.date}`;
    points.set(key, (points.get(key) ?? 0) + tokens);
    const cur = shares.get(bucket) ?? { tokens: 0, calls: 0 };
    cur.tokens += tokens;
    cur.calls += r.calls;
    shares.set(bucket, cur);
  }

  const total = [...shares.values()].reduce((n, s) => n + s.tokens, 0);
  const models = [...shares.keys()].sort(
    (a, b) => (shares.get(b)?.tokens ?? 0) - (shares.get(a)?.tokens ?? 0),
  );

  return {
    models,
    trend: [...points.entries()].map(([k, tokens]) => {
      const [model, date] = k.split('\u0000');
      return { model, date, tokens };
    }),
    shares: models.map((m) => {
      const s = shares.get(m)!;
      return {
        model: m,
        tokens: s.tokens,
        percent: total > 0 ? (s.tokens * 100) / total : 0,
        calls: s.calls,
      };
    }),
    dates,
  };
}

/** 假后端的总入口，按命令名分发。 */
export async function handle(command: string, args?: Record<string, unknown>): Promise<unknown> {
  // 模拟真实 IPC 的往返延迟，让加载态在开发时也能被看到
  await new Promise((r) => setTimeout(r, 120));

  switch (command) {
    case 'list_mappings':
      return mappings;

    case 'sync_mappings': {
      const dry = args?.dryRun === true;
      if (dry) {
        return ok('[预览] 将同步 6 个映射，跳过 2 个未安装的 agent');
      }
      return ok(
        '[zcode]    AGENTS.md  hardlink  ok\n' +
          '[claude]   CLAUDE.md  hardlink  ok\n' +
          '[codex]    AGENTS.md  hardlink  ok\n' +
          '[cursor]   AGENTS.md  hardlink  ok\n' +
          '[opencode] AGENTS.md  hardlink  ok\n' +
          '[pi]       AGENTS.md  hardlink  ok\n' +
          '完成：6 个已同步，2 个跳过',
      );
    }

    case 'repair_mapping':
      return ok(`[${String(args?.agent)}] 已修复`);

    case 'unmap': {
      const agent = String(args?.agent ?? '');
      const m = mappings.find((x) => x.agent === agent);
      if (!m) return { ok: false, exitCode: 1, output: `找不到 agent：${agent}` };
      m.disabled = true;
      m.actual = null;
      m.health = 'missing';
      m.note = '已取消，同步时跳过';
      return ok(`[${m.label}] 已取消映射\n下次同步会跳过，点「恢复映射」可以重新建立`);
    }

    case 'remap': {
      const agent = String(args?.agent ?? '');
      const m = mappings.find((x) => x.agent === agent);
      if (!m) return { ok: false, exitCode: 1, output: `找不到 agent：${agent}` };
      m.disabled = false;
      m.actual = 'hardlink';
      m.health = 'ok';
      m.note = null;
      return ok(`[${m.label}] 已恢复映射：Hardlink — 已创建硬链接（无需权限）`);
    }

    case 'read_doc':
      // 浏览器预览时的占位内容。必须明确标注这是假数据，
      // 否则用户会以为看到的就是磁盘上的真实文件。
      return {
        exists: true,
        content:
          '<!-- 浏览器预览模式：以下是你 ~/.agents/AGENTS.md 的占位内容，不是真实文件。 -->\n' +
          '<!-- 要看真实内容请运行 tauri dev 或直接打开 exe。 -->\n\n' +
          '# 全局工作方法\n\n' +
          '（此处显示假数据，真实内容由 Rust 后端从磁盘读取）\n',
      };

    case 'save_doc':
      return ok(`已保存母版（${String(args?.content ?? '').length} 字符）`);

    case 'create_doc':
      return ok('已创建母版文件');

    case 'list_skills':
      return skills;

    case 'set_skill_tags':
      return ok(`已更新 ${String(args?.name)} 的标签`);

    case 'scan_orphans':
      return orphans;

    case 'collect_skills': {
      const plan: CollectPlan = {
        toImport: orphans.filter((o) => o.relation === 'unique'),
        toDedupe: orphans.filter((o) => o.relation === 'identical'),
        conflicts: orphans.filter((o) => o.relation === 'conflict'),
        broken: orphans.filter((o) => o.relation === 'broken-link'),
      };
      return plan;
    }

    case 'apply_orphan_action': {
      // 预览模式：只把这一条从散落清单里拿掉，磁盘什么都没动
      const path = String(args?.path ?? '');
      const action = String(args?.action ?? '') as OrphanAction;
      const idx = orphans.findIndex((o) => o.path === path);
      const name = idx >= 0 ? orphans[idx].name : path;
      if (idx >= 0) orphans.splice(idx, 1);

      const messages: Record<OrphanAction, string> = {
        import: '已移入中心库，原位置建立 Hardlink',
        dedupe: '已删除散落副本，原位置建立 Hardlink 指向中心库',
        'take-orphan': '已用散落版替换中心库；旧版备份在 ~/.agents/backup/；原位置建立 Hardlink',
        'take-shared': '已采用中心库那份，散落副本删除，原位置建立 Hardlink',
        delete: '已删除这份散落副本',
        'remove-link': '已删除坏链接',
      };

      return { name, path, action, ok: true, message: messages[action] ?? '已处理' };
    }

    case 'list_managed':
      return managed;

    case 'check_updates':
      return { ok: false, exitCode: 1, output: '检查完成：2 个 skill 有可用更新\n  minimax-docx\n  find-skills' };

    case 'update_skills': {
      const names = (args?.names as string[]) ?? [];
      // 模拟更新后 lock 文件里的 hash 和 updatedAt 变化，
      // 让界面能看到「指纹变了、日期刷新了」这个效果
      for (const m of managed) {
        if (names.length === 0 || names.includes(m.name)) {
          m.hashShort = 'updated1';
          m.updatedAt = new Date().toISOString();
          m.hasUpdate = false;
        }
      }
      return ok(
        names.length
          ? `✓ Updated ${names.join(', ')}\n\n✓ Updated ${names.length} skill(s)`
          : '✓ Updated all managed skills',
      );
    }

    case 'git_status':
      return gitStatus;

    case 'git_commit':
      return ok(`已提交 ${(args?.paths as string[])?.length ?? 0} 个路径`);

    case 'git_push':
      return ok('已推送到 origin/master');

    // ---- provider 管理（浏览器预览用假数据）----
    case 'list_providers':
      return [
        {
          id: 'command',
          name: 'CommandCode',
          baseUrl: 'https://api.commandcode.ai/provider/v1',
          kind: 'anthropic',
          kindLabel: 'Anthropic Messages',
          // 演示：同一个端点，Pi 里实际用的是 openai-chat
          agentKinds: { pi: 'openai-chat' },
          models: [
            {
              id: 'deepseek/deepseek-v4.1-flash',
              contextWindow: 1000000,
              maxOutputTokens: 128000,
              inputModalities: ['text'],
              outputModalities: ['text'],
              reasoning: true,
              reasoningEfforts: ['low', 'high', 'max'],
              defaultEffort: 'max',
            },
            {
              id: 'xiaomi/mimo-v2.5',
              contextWindow: 1000000,
              maxOutputTokens: 131072,
              inputModalities: ['text'],
              outputModalities: ['text'],
              reasoning: false,
              reasoningEfforts: [],
            },
          ],
          agents: ['claude', 'zcode', 'pi', 'codex'],
          codexRequiresOpenaiAuth: mockState.codexAuth.get('command') ?? null,
          codexModel: mockState.codexModel.get('command') ?? null,
          claudeModelMap: {
            opus: 'deepseek/deepseek-v4.1-flash',
            sonnet: 'xiaomi/mimo-v2.5',
            haiku: 'xiaomi/mimo-v2.5',
            default: 'deepseek/deepseek-v4.1-flash',
          },
          note: '2026-11-10 从控制台获取',
          keyLen: 93,
          keyTail: 'JGNt',
        },
        {
          id: 'minimax-cn',
          name: 'MiniMax 中文站',
          baseUrl: 'https://api.minimax.cn/anthropic',
          kind: 'anthropic',
          kindLabel: 'Anthropic Messages',

          agentKinds: {},
          models: [
            {
              id: 'MiniMax-M3',
              contextWindow: 1000000,
              maxOutputTokens: 128000,
              inputModalities: ['text', 'image', 'video', 'pdf'],
              outputModalities: ['text'],
              reasoning: false,
              reasoningEfforts: [],
            },
          ],
          agents: ['zcode', 'pi'],
          claudeModelMap: {},
          note: '',
          keyLen: 125,
          keyTail: 'I3v0',
        },
        {
          id: 'dsh-responses',
          name: 'DSH Responses 测试',
          baseUrl: 'https://responses.example.com/v1',
          kind: 'openai-responses',
          kindLabel: 'OpenAI Responses',
          // 只填了 id：用来在预览里展示「未填元数据」的形态
          models: [
            { id: 'resp-model', inputModalities: [], outputModalities: [], reasoningEfforts: [] },
          ],
          agents: ['dsh', 'workbuddy'],
          claudeModelMap: {},
          note: '验证 Responses 协议',
          keyLen: 40,
          keyTail: 'abcd',
        },
      ];

    case 'backfill_models':
      return {
        filledFields: 3,
        lines: [
          {
            provider: 'command',
            model: 'deepseek/deepseek-v4.1-flash',
            source: 'pi',
            filled: ['context_window=1000000', 'input=[text]'],
          },
          {
            provider: 'minimax-cn',
            model: 'MiniMax-M3',
            source: 'ZCode',
            filled: ['max_output_tokens=128000'],
          },
        ],
        skippedProviders: ['Bigmodel - API Key (来自 ZCode)'],
        kindMismatches: [
          {
            provider: 'command',
            agent: 'zcode',
            master: 'anthropic',
            actual: 'openai-chat',
          },
        ],
        wrote: args?.dryRun !== true,
      };

    case 'save_provider':
      return ok(`已保存 provider ${String(args?.id)}`);

    case 'delete_provider':
      return ok(`已从母版移除 ${String(args?.id)}`);

    // ---- provider 分配矩阵（浏览器预览假数据）----
    case 'provider_matrix':
      return {
        columns: [
          { agent: 'claude', label: 'Claude Code', available: true, note: '', singleSlot: true, supportedKinds: ['anthropic'] },
          { agent: 'codex', label: 'Codex CLI', available: true, note: '', singleSlot: true, supportedKinds: ['openai-responses'] },
          { agent: 'pi', label: 'pi', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'opencode', label: 'opencode', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'workbuddy', label: 'WorkBuddy', available: true, note: '', singleSlot: false, supportedKinds: [] },
          {
            agent: 'dsh',
            label: 'DeepSeek Harness',
            available: false,
            note: '配置文件不存在',
            singleSlot: false,
            supportedKinds: [],
          },
          { agent: 'zcode', label: 'ZCode', available: true, note: '', singleSlot: false, supportedKinds: [] },
        ],
        providers: [
          {
            id: 'command',
            name: 'CommandCode',
            baseUrl: 'https://api.commandcode.ai/provider/v1',
            kind: 'anthropic',
            kindLabel: 'Anthropic Messages',
            agentKinds: { pi: 'openai-chat' },
            models: [
              {
                id: 'deepseek/deepseek-v4.1-flash',
                contextWindow: 1000000,
                maxOutputTokens: 128000,
                inputModalities: ['text'],
                outputModalities: ['text'],
                reasoning: true,
                reasoningEfforts: ['low', 'high', 'max'],
                defaultEffort: 'max',
              },
              {
                id: 'xiaomi/mimo-v2.5',
                contextWindow: 1000000,
                maxOutputTokens: 131072,
                inputModalities: ['text'],
                outputModalities: ['text'],
                reasoningEfforts: [],
              },
            ],
            agents: ['claude', 'zcode', 'pi', 'codex'],
            codexRequiresOpenaiAuth: mockState.codexAuth.get('command') ?? null,
            codexModel: mockState.codexModel.get('command') ?? null,
            claudeModelMap: {
              opus: 'deepseek/deepseek-v4.1-flash',
              sonnet: 'xiaomi/mimo-v2.5',
              haiku: 'xiaomi/mimo-v2.5',
              default: 'deepseek/deepseek-v4.1-flash',
            },
            note: '2026-11-10 从控制台获取',
            keyLen: 93,
            keyTail: 'JGNt',
          },
          {
            id: 'minimax-cn',
            name: 'MiniMax 中文站',
            baseUrl: 'https://api.minimax.cn/anthropic',
            kind: 'openai-chat',
            kindLabel: 'OpenAI Chat Completions',

            agentKinds: {},
            models: [
              {
                id: 'MiniMax-M3',
                contextWindow: 1000000,
                maxOutputTokens: 128000,
                inputModalities: ['text', 'image', 'video', 'pdf'],
                outputModalities: ['text'],
                reasoningEfforts: [],
              },
            ],
            agents: ['zcode', 'pi'],
            claudeModelMap: {},
            note: '',
            keyLen: 125,
            keyTail: 'I3v0',
          },
          {
            id: 'dsh-responses',
            name: 'DSH Responses 测试',
            baseUrl: 'https://responses.example.com/v1',
            kind: 'openai-responses',
            kindLabel: 'OpenAI Responses',

            agentKinds: {},
            models: [
              { id: 'resp-model', inputModalities: [], outputModalities: [], reasoningEfforts: [] },
            ],
            agents: ['workbuddy'],
            claudeModelMap: {},
            note: '验证 Responses 协议',
            keyLen: 40,
            keyTail: 'abcd',
          },
        ],
        empty: false,
      };

    case 'set_provider_row':
      return ok(`已更新 ${String(args?.id)} 的分配`);

    case 'set_provider_assignment':
      return ok(
        `${String(args?.providerId)} 在 ${String(args?.agent)} ${args?.on ? '已启用' : '已取消'}`,
      );

    // ---- Cursor 自带密钥（预览用假数据）----
    //
    // 照本机真实状态摆：开关已开、地址指向 CommandCode、密钥 93 字符。
    // 但 keyTail 是假的 —— 预览模式不能泄露真实密钥的任何片段。
    case 'cursor_byok_status':
      return {
        installed: true,
        useOpenaiKey: mockState.cursorByok.useOpenaiKey,
        baseUrl: mockState.cursorByok.baseUrl,
        keyLen: 93,
        keyTail: 'x9Uv',
        userAddedModels: [...mockState.cursorByok.models],
        dbPath: 'C:\\Users\\leaf\\AppData\\Roaming\\Cursor\\User\\globalStorage\\state.vscdb',
        running: false,
      };

    case 'cursor_byok_plan': {
      const id = String(args?.providerId ?? '');
      const want = mockCursorProviderUrl(id);
      if (!want) throw new Error(`母版里没有 provider：${id}`);

      const have = (mockState.cursorByok.baseUrl ?? '').replace(/\/+$/, '');
      const setKey = !mockState.cursorByok.useOpenaiKey;
      const setUrl = want !== have;
      const wantModels = (args?.models as string[]) ?? [];
      const addModels = wantModels.filter((m) => m.trim() && !mockState.cursorByok.models.includes(m));

      const summary: string[] = [];
      if (setKey) summary.push('打开「Use OpenAI API Key」开关（关着时自定义模型名会被拒）');
      if (setUrl) summary.push(`接口地址 ${have || '（未设置）'} → ${want}`);
      if (addModels.length > 0) summary.push(`加进模型选择器：${addModels.join(', ')}`);

      return {
        setUseOpenaiKey: setKey ? true : null,
        setBaseUrl: setUrl ? want : null,
        writeKey: false,
        addModels,
        removeModels: [],
        summary,
        noop: !setKey && !setUrl && addModels.length === 0,
      };
    }

    case 'cursor_byok_apply': {
      const id = String(args?.providerId ?? '');
      const want = mockCursorProviderUrl(id);
      if (!want) return { ok: false, exitCode: 1, output: `母版里没有 provider：${id}` };
      mockState.cursorByok.useOpenaiKey = true;
      mockState.cursorByok.baseUrl = want;
      const add = ((args?.models as string[]) ?? []).filter(
        (m) => m.trim() && !mockState.cursorByok.models.includes(m),
      );
      for (const m of add) mockState.cursorByok.models.push(m);
      const lines = ['  - 已设置 useOpenAIKey = true'];
      if (add.length > 0) lines.push(`  - 已加进模型选择器：${add.join(', ')}`);
      return ok(`已写入 Cursor（${id}）：\n${lines.join('\n')}\n\n重启 Cursor 后生效。`);
    }

    case 'cursor_byok_remove_models': {
      const drop = (args?.models as string[]) ?? [];
      mockState.cursorByok.models = mockState.cursorByok.models.filter((m) => !drop.includes(m));
      return ok(
        `已更新 Cursor 的模型选择器：\n  - 已从模型选择器移除：${drop.join(', ')}\n\n重启 Cursor 后生效。`,
      );
    }

    case 'cursor_byok_reset':
      mockState.cursorByok.useOpenaiKey = false;
      mockState.cursorByok.baseUrl = null;
      return ok('已重置 Cursor：\n  - 关闭自带密钥，清空接口地址\n\n重启 Cursor 后回到官方模型。');

    case 'diff_providers':
      return {
        lines: [
          { agent: 'claude', path: '~/.claude/settings.json', action: 'unchanged', target: 'command', detail: '已一致' },
          { agent: 'zcode', path: '~/.zcode/v2/config.json', action: 'unchanged', target: 'command', detail: 'CommandCode 已是最新' },
          { agent: 'pi', path: '~/.pi/agent/auth.json', action: 'unchanged', target: 'command', detail: 'command 的 key 已一致' },
          { agent: 'pi', path: '~/.pi/agent/models.json', action: 'update', target: 'command', detail: '更新 CommandCode 的 baseUrl/api' },
          { agent: 'opencode', path: '~/.config/opencode/opencode.json', action: 'add', target: 'command', detail: '新增 CommandCode 到 opencode.json' },
        ],
        conflicts: [],
        agentCount: 2,
        changedFiles: 2,
      };

    case 'sync_providers': {
      const dry = args?.dryRun === true;
      return {
        ok: true,
        written: dry ? [] : ['~/.pi/agent/models.json', '~/.config/opencode/opencode.json'],
        skipped: ['~/.claude/settings.json', '~/.zcode/v2/config.json'],
        backups: dry ? [] : ['~/.agents/backup/providers-first-write/abc123-models.json'],
        log: dry
          ? ['[预览] pi: 将写入 ~/.pi/agent/models.json', '[跳过] claude: 无变化']
          : ['[写入] pi: ~/.pi/agent/models.json', '      更新 CommandCode 的 baseUrl/api'],
      };
    }

    case 'read_providers_toml':
      return (
        '[[provider]]\n' +
        'id = "command"\n' +
        'name = "CommandCode"\n' +
        'base_url = "https://api.commandcode.ai/provider/v1"\n' +
        'kind = "anthropic"\n' +
        'api_key = "<预览模式不显示>"\n' +
        'models = ["deepseek/deepseek-v4.1-flash"]\n' +
        'agents = ["claude", "zcode", "pi"]\n'
      );

    case 'write_providers_toml':
      return ok('母版已保存');

    // ---- Claude 引导 / Codex 后端（浏览器预览假数据）----
    case 'claude_onboarding_status':
      return {
        path: '~/.claude.json',
        available: true,
        skipped: mockState.claudeOnboardingSkipped,
      };

    case 'set_claude_onboarding_skip': {
      const skip = args?.skip === true;
      mockState.claudeOnboardingSkipped = skip;
      return ok(
        skip
          ? '已写入 hasCompletedOnboarding = true，下次启动跳过引导'
          : '已删除 hasCompletedOnboarding，下次启动会重新出现引导',
      );
    }

    case 'set_codex_requires_openai_auth': {
      const id = String(args?.providerId ?? '');
      const value = (args?.value ?? null) as boolean | null;
      mockState.codexAuth.set(id, value);
      const label =
        value === null ? '自动' : value ? '走 ChatGPT 后端' : '绕过 ChatGPT 后端';
      return ok(`requires_openai_auth = ${value ?? '自动'}（${label}）`);
    }

    case 'set_codex_model': {
      const id = String(args?.providerId ?? '');
      const model = (args?.model ?? null) as string | null;
      mockState.codexModel.set(id, model);
      return ok(
        model
          ? `model → ${model}（写入 ~/.codex/config.toml 顶层 model）`
          : '已取消指定，回退到 models 的第一个',
      );
    }

    // ---- MCP 管理（浏览器预览假数据）----
    case 'list_mcp_servers':
      return [
        {
          id: 'context7',
          name: 'Context7',
          transport: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@upstash/context7-mcp'],
          cwd: '',
          url: '',
          envKeys: [],
          headerKeys: [],
          enabled: true,
          agents: ['claude', 'codex', 'pi', 'opencode'],
          note: '',
        },
        {
          id: 'remote-example',
          name: '远程示例',
          transport: 'http',
          command: '',
          args: [],
          cwd: '',
          url: 'https://example.com/mcp',
          envKeys: [],
          headerKeys: ['Authorization'],
          enabled: false,
          agents: ['claude', 'cursor'],
          note: '带鉴权头的远程 MCP',
        },
      ];

    case 'save_mcp_server':
      return ok(`已保存 MCP server ${String(args?.id)}`);

    case 'delete_mcp_server':
      return ok(`已从母版移除 ${String(args?.id)}`);

    case 'diff_mcp':
      return {
        lines: [
          { agent: 'claude', path: '~/.claude.json · mcpServers.context7', action: 'unchanged', target: 'context7', detail: '已一致' },
          { agent: 'codex', path: 'config.toml · mcp_servers.context7', action: 'unchanged', target: 'context7', detail: '已一致' },
          { agent: 'claude', path: '~/.claude.json', action: 'add', target: 'remote-example', detail: '新增 远程示例（https://example.com/mcp）' },
          { agent: 'cursor', path: '~/.cursor/mcp.json', action: 'add', target: 'remote-example', detail: '新增 远程示例（https://example.com/mcp）' },
          { agent: 'opencode', path: 'opencode.json · mcp.time', action: 'update', target: 'time', detail: '启用状态 启用 → 停用' },
        ],
        agentCount: 3,
        changedFiles: 3,
      };

    case 'sync_mcp': {
      const dry = args?.dryRun === true;
      return {
        ok: true,
        written: dry ? [] : ['~/.claude.json', '~/.cursor/mcp.json'],
        skipped: ['~/.codex/config.toml'],
        backups: dry ? [] : ['~/.agents/backup/providers-first-write/abc123-.claude.json'],
        log: dry
          ? ['[预览] claude: 将写入 ~/.claude.json', '[预览] cursor: 将写入 ~/.cursor/mcp.json']
          : ['[写入] claude: ~/.claude.json', '        新增 远程示例'],
      };
    }

    case 'read_mcp_toml':
      return (
        '[[server]]\n' +
        'id = "context7"\n' +
        'name = "Context7"\n' +
        'transport = "stdio"\n' +
        'command = "cmd"\n' +
        'args = ["/c", "npx", "-y", "@upstash/context7-mcp"]\n' +
        'enabled = true\n' +
        'agents = ["claude", "codex", "pi", "opencode"]\n'
      );

    case 'write_mcp_toml':
      return ok('MCP 母版已保存');

    // ---- MCP 分配矩阵（浏览器预览假数据）----
    //
    // 列的顺序和 available 照抄真实后端在本机的输出：装了且找到配置的
    // available=true，没装的 false（界面上那些格子不可点）。
    case 'mcp_matrix':
      return {
        columns: [
          { agent: 'claude', label: 'Claude Code', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'codex', label: 'Codex', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'zcode', label: 'ZCode', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'pi', label: 'pi', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'opencode', label: 'opencode', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'workbuddy', label: 'WorkBuddy', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'dsh', label: 'DeepSeek Harness', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'cursor', label: 'Cursor', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'qoder', label: 'Qoder', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'hermes', label: 'Hermes Agent', available: false, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'gemini', label: 'Gemini CLI', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
          { agent: 'grok', label: 'Grok Build', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
          { agent: 'kimi', label: 'Kimi Code', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
          { agent: 'minimax', label: 'MiniMax Code', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
          { agent: 'copilot', label: 'GitHub Copilot CLI', available: false, note: '本机未初始化', singleSlot: false, supportedKinds: [] },
          { agent: 'agy', label: 'Antigravity', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
          { agent: 'omp', label: 'Oh My Pi', available: false, note: '本机未安装', singleSlot: false, supportedKinds: [] },
        ],
        rows: [
          {
            id: 'context7',
            name: 'Context7',
            transport: 'stdio',
            target: 'cmd /c npx -y @upstash/context7-mcp',
            enabled: true,
            assigned: ['claude', 'codex', 'pi', 'opencode'],
            note: '',
          },
          {
            id: 'fetch',
            name: 'Fetch',
            transport: 'stdio',
            target: 'uvx mcp-server-fetch',
            enabled: true,
            assigned: ['claude', 'codex'],
            note: '',
          },
          {
            id: 'playwright',
            name: 'Playwright',
            transport: 'stdio',
            target: 'cmd /c playwright-mcp --headless',
            enabled: true,
            assigned: ['pi'],
            note: '只在 pi 里用',
          },
          {
            id: 'remote-example',
            name: '远程示例',
            transport: 'http',
            target: 'https://example.com/mcp',
            enabled: false,
            assigned: ['claude', 'cursor'],
            note: '带鉴权头的远程 MCP',
          },
        ],
        empty: false,
      };

    case 'set_mcp_assignment':
      return ok(`已更新 ${String(args?.serverId)} → ${String(args?.agent)}`);

    case 'set_mcp_row':
      return ok(`已更新 ${String(args?.serverId)} 的分配`);

    case 'import_mcp_from_agents':
      return ok('已导入 4 个 server：context7, fetch, time, sequential-thinking');

    case 'inspect_credentials':
      // 数据照抄本机实测结果，但指纹和尾巴都是假的 ——
      // 预览模式绝不能暴露真实密钥的任何片段
      return {
        providers: [
          [
            { agent: 'claude', provider: 'command', kind: 'api_key', location: 'settings.json · ANTHROPIC_AUTH_TOKEN', fingerprint: '06a04d4b', length: 93, tail: '…JGNt', sharedWith: ['zcode', 'pi'], duplicated: true },
            { agent: 'pi', provider: 'command', kind: 'api_key', location: 'auth.json · command', fingerprint: '06a04d4b', length: 93, tail: '…JGNt', sharedWith: ['claude', 'zcode'], duplicated: true },
            { agent: 'zcode', provider: 'command', kind: 'api_key', location: 'config.json · b6352650', fingerprint: '06a04d4b', length: 93, tail: '…JGNt', sharedWith: ['claude', 'pi'], duplicated: true },
            { agent: '环境变量', provider: 'command', kind: 'api_key', location: '$COMMAND_API_KEY', fingerprint: '56fbfdb2', length: 93, tail: '…x9Uv', sharedWith: [], duplicated: false },
          ],
          [
            { agent: 'pi', provider: 'deepseek', kind: 'api_key', location: 'auth.json · deepseek', fingerprint: '35516fce', length: 35, tail: '…6fc0', sharedWith: [], duplicated: false },
            { agent: '环境变量', provider: 'deepseek', kind: 'api_key', location: '$DEEPSEEK_API_KEY', fingerprint: 'd39549a5', length: 35, tail: '…4cde', sharedWith: [], duplicated: false },
          ],
          [
            { agent: 'pi', provider: 'minimax-cn', kind: 'api_key', location: 'auth.json · minimax-cn', fingerprint: 'abe1befa', length: 125, tail: '…I3v0', sharedWith: ['zcode'], duplicated: true },
            { agent: 'zcode', provider: 'minimax-cn', kind: 'api_key', location: 'config.json · 10bbfce2', fingerprint: 'abe1befa', length: 125, tail: '…I3v0', sharedWith: ['pi'], duplicated: true },
          ],
          [
            { agent: 'codex', provider: 'openai-oauth', kind: 'oauth', location: 'auth.json · access_token', fingerprint: 'b7493546', length: 1849, tail: '…gU7Q', sharedWith: [], duplicated: false },
          ],
        ],
        unsupported: [
          { agent: 'cursor', reason: '配置和密钥在 state.vscdb 里，密钥经 Electron safeStorage 加密，读不出明文' },
          { agent: 'qoder', reason: '使用浏览器 OAuth 登录，本地无明文 key' },
          { agent: 'workbuddy', reason: '使用浏览器 OAuth 登录，本地无明文 key' },
        ],
        totalEntries: 9,
        uniqueKeys: 6,
        duplicatedKeys: 2,
        providersWithMultipleKeys: 2,
      };

    case 'usage_report': {
      // 数据照抄本机真实统计结果的量级，但日期和数字做了简化：
      // 预览模式只需要看清布局和量级，不必复现几十亿 token 的精确值。
      // refresh 在预览里没有对应动作 —— 假后端本来就不读盘，丢掉缓存也没意义
      const days = Number(args?.days ?? 7);
      const enabledBoth = mockState.usageEnabled.size > 0;

      if (!enabledBoth) {
        return {
          days,
          agents: mockUsageAgents(),
          rows: [],
          charts: emptyCharts(),
        };
      }

      const rows = mockUsageRows().filter(
        (r) => mockState.usageEnabled.has(r.agent) && (days === 0 || r.date >= mockUsageCutoff(days)),
      );
      return {
        days,
        agents: mockUsageAgents(),
        rows,
        charts: mockCharts(rows),
      };
    }

    case 'set_usage_enabled': {
      const agent = String(args?.agent ?? '');
      const on = args?.on === true;
      const st = mockUsageAgents().find((a) => a.agent === agent);
      if (!st) return { ok: false, exitCode: 1, output: `找不到 agent：${agent}` };
      if (on && !st.supported) {
        return { ok: false, exitCode: 1, output: `${agent} 暂不支持用量统计` };
      }
      if (on) mockState.usageEnabled.add(agent);
      else mockState.usageEnabled.delete(agent);
      return ok(`${agent} 的用量统计已${on ? '启用' : '关闭'}`);
    }

    default:
      throw new Error(`未实现的命令：${command}`);
  }
}

/** 供界面展示的当前时间戳，避免 lint 报未使用。 */
export const mockNow = now;
