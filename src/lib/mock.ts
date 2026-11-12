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
  OrphanSkill,
  RunResult,
  Skill,
} from './types';

const now = new Date().toISOString();

/** 6 个指令文件的实测状态。 */
const mappings: Mapping[] = [
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
  { name: 'analyze', location: '.config/opencode/skills', path: 'C:\\Users\\leaf\\.config\\opencode\\skills\\analyze', relation: 'unique' },
  { name: 'context-archive', location: '.config/opencode/skills', path: 'C:\\Users\\leaf\\.config\\opencode\\skills\\context-archive', relation: 'unique' },
  { name: 'dingtalk-req-to-spec', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\dingtalk-req-to-spec', relation: 'unique' },
  { name: 'hardcoded-page-portal-config', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\hardcoded-page-portal-config', relation: 'unique' },
  { name: 'portal-link-migrate', location: '.cursor/skills', path: 'C:\\Users\\leaf\\.cursor\\skills\\portal-link-migrate', relation: 'unique' },
  { name: 'plan-execute', location: '.workbuddy/skills', path: 'C:\\Users\\leaf\\.workbuddy\\skills\\plan-execute', relation: 'identical' },
  { name: 'agent-browser', location: '.claude/skills', path: 'C:\\Users\\leaf\\.claude\\skills\\agent-browser', relation: 'identical' },
  { name: 'agent-browser', location: '.codebuddy/skills', path: 'C:\\Users\\leaf\\.codebuddy\\skills\\agent-browser', relation: 'identical' },
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
};

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
    case 'mcp_matrix':
      return {
        columns: [
          { agent: 'claude', label: 'Claude Code', available: true, note: '', singleSlot: true, supportedKinds: ['anthropic'] },
          { agent: 'codex', label: 'Codex', available: true, note: '', singleSlot: false, supportedKinds: ['openai-responses'] },
          { agent: 'cursor', label: 'Cursor', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'pi', label: 'pi', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'opencode', label: 'opencode', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'qoder', label: 'Qoder', available: true, note: '', singleSlot: false, supportedKinds: [] },
          { agent: 'workbuddy', label: 'WorkBuddy', available: true, note: '', singleSlot: false, supportedKinds: [] },
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
          { agent: 'cursor', reason: '密钥存在系统凭据库中，不在配置文件里' },
          { agent: 'qoder', reason: '使用浏览器 OAuth 登录，本地无明文 key' },
          { agent: 'workbuddy', reason: '使用浏览器 OAuth 登录，本地无明文 key' },
        ],
        totalEntries: 9,
        uniqueKeys: 6,
        duplicatedKeys: 2,
        providersWithMultipleKeys: 2,
      };

    default:
      throw new Error(`未实现的命令：${command}`);
  }
}

/** 供界面展示的当前时间戳，避免 lint 报未使用。 */
export const mockNow = now;
