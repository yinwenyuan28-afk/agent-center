# agentcenter

统一管理多个 AI coding agent 的全局配置。

五个功能：

| 功能              | 做什么                                             |
| --------------- | ----------------------------------------------- |
| **AGENTS 文档映射** | 一份母版 `~/.agents/AGENTS.md`，链接到各 agent 自己的指令文件位置 |
| **Skills 中心库**  | 收集散落在各 agent 目录的 skill，打标签分类                    |
| **Skill 更新**    | 封装 `npx skills`，管理第三方 skill 的版本                 |
| **Provider**    | 所有 provider 一个池子，每个只留一把 key，逐条选注入到哪些 agent  |
| **模型元数据**     | 每个 provider 下的模型可填上下文窗口、输入模态、推理档位 |
| **MCP**         | 所有 MCP server 一个池子，逐条选注入到哪些 agent                |
| **Git**         | `~/.agents` 的版本管理，按路径提交                         |

Provider 和 MCP 用同一套交互：默认只列清单，点某一行的「分配」才展开 agent 卡片，
点右上角「应用到各 agent」才真正写配置文件。

模型元数据在 provider 编辑表单里：每个模型一行摘要（`1M ctx · 128K out · text`），
点开填上下文窗口、输出上限、输入模态、推理档位。留空表示「不指定」，
注入时沿用该 agent 配置里的现有值。已有配置里的真实元数据可以一键回填
（`agentcenter models backfill`）。

**协议按 agent 分开。** 同一个端点在不同 agent 里用的协议可能完全不同——
实测 `opencode.ai/zen/go` 在 ZCode 里是 anthropic、在 Pi 里是 openai-responses。
所以 provider 上有一个默认协议，每个已分配的 agent 可以单独覆盖。
每个 agent 的下拉只列它**真能表达**的协议（Codex 只有 responses、
Claude 只有 anthropic），避免选了却被静默改写。
`agentcenter models backfill` 会报告母版和实际不一致的地方。

## 为什么需要这个

同时用 8 个以上的 AI coding agent 时，配置会散成这样：

| 文件                             | 内容           |
| ------------------------------ | ------------ |
| `~/.claude/CLAUDE.md`          | 编码约定、计划先行    |
| `~/.zcode/AGENTS.md`           | 全局工作方法、文件归档  |
| `~/.cursor/AGENTS.md`          | 通用开发准则       |
| `~/.config/opencode/AGENTS.md` | 编码环境、按需加载    |
| `~/.pi/agent/AGENTS.md`        | reasoning 写法 |
| `~/.codex/AGENTS.md`           | **空文件**      |

这些内容互不相同，也没有单一真实来源。skills 也一样：`~/.agents/skills` 下 18 个，另外 `.cursor/skills`、`.config/opencode/skills` 里还有散落的副本。

API key 更是如此。实测本机 19 条凭证、16 个不同 key，其中 2 组重复：
CommandCode 那把同时配在 claude / zcode / pi 三处，MiniMax 那把在 zcode / pi 两处。

agentcenter 把这些收敛到一处：一份母版 + 一套分类索引 + 一个 provider 库 + 一个界面。

## 装之前需要什么

### 必须自己装（要管理员权限）

| 依赖                            | 怎么装                                                                       |
| ----------------------------- | ------------------------------------------------------------------------- |
| **Microsoft C++ Build Tools** | 下 `vs_BuildTools.exe`，安装时勾选 **Desktop development with C++**              |
| **Rust**                      | 从 https://rustup.rs 下 `rustup-init.exe`，装完执行 `rustup default stable-msvc` |

**顺序不能反**：先 Build Tools 再 Rust。Rust 安装时会检测链接器，反了会装成 GNU 工具链。

装完在新终端里验证（如果提示找不到命令，把 `C:\Users\<你>\.cargo\bin` 加进 PATH，或重开终端）：

```
rustc --version
cargo --version
```

### crates.io 镜像

直连 crates.io 在国内网络下常会卡在 `Updating crates.io index` 后超时。仓库里用的配置放在 `~/.cargo/config.toml`：

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
```

不需要镜像就把这个文件删掉或注释掉全部内容。

### 已经有了

- WebView2 运行时（Windows 10 1803 以后自带）
- Node 18 以上

## 开发

```
npm install
npm run dev        # 浏览器预览，用假数据，不需要 Rust
npm run tauri dev  # 开真正的桌面窗口
npm run build      # 构建前端
npm run check      # 类型检查
npm run verify     # 契约检查 + 类型检查 + 构建
```

只想看界面的话，`npm run dev` 就够了——浏览器里跑的是 `src/lib/mock.ts` 的假数据，内容照抄本机实测结果。这样不用等 Rust 环境就绪就能调界面。

改前端时**不要同时跑 `npm run dev` 和 `npm run tauri dev`**：两个都会抢 5731 端口，后者会直接失败。端口被占时用 `node scripts/free-port.mjs 5731` 清理。

## 命令行用法

```
agentcenter                          # 开窗口
agentcenter sync                     # 同步全部 AGENTS 映射
agentcenter sync --dry-run           # 只看要做什么
agentcenter status                   # 查看各映射健康状态
agentcenter mappings                 # 列出映射表

agentcenter skills list              # 列出中心库的 skill
agentcenter skills scan              # 扫描散落副本
agentcenter skills collect           # 预览收集计划
agentcenter skills collect --apply
agentcenter skills tag ui-design --add UI设计
agentcenter skills check             # 检查第三方更新
agentcenter skills update            # 更新全部

agentcenter creds inspect            # API key 体检（只读）

agentcenter providers list           # 列出母版里的 provider
agentcenter providers matrix         # 显示分配矩阵：每个 provider 用在哪些 agent
agentcenter providers assign <id> --agents claude,pi   # 整行设置分配（省略 --agents 清空）
agentcenter providers diff           # 对比母版与各 agent 现状（只读）
agentcenter providers sync           # 注入到各 agent
agentcenter providers sync --dry-run # 只看要做什么
agentcenter providers set <id> --name ... --base-url ... --api-key ...
agentcenter providers set <id> --agent-kind pi=openai-responses   # 按 agent 覆盖协议
agentcenter providers remove <id>
agentcenter providers example        # 输出示例母版
agentcenter providers doctor         # 检查 .gitignore 是否已忽略母版

agentcenter claude status            # Claude Code 是否会显示首次引导
agentcenter claude skip              # 跳过首次引导（写 ~/.claude.json 的 hasCompletedOnboarding）
agentcenter claude restore           # 恢复首次引导（删掉那个键）

agentcenter models backfill          # 从各 agent 现有配置回填模型元数据
agentcenter models backfill --dry-run # 只看将补哪些字段
agentcenter agents                   # 列出所有 agent 及其支持的能力

agentcenter git status
agentcenter git commit --paths skills/ui-design -m "feat: 更新 ui-design"
agentcenter git push
```

`status` 有需要处理的项时退出码为 2，便于脚本判断。

### 新增 provider 的完整流程

```
# 1. 建母版（首次）
agentcenter providers example > ~/.agents/providers.toml

# 2. 编辑填入真实 key，或用命令添加
agentcenter providers set minimax \
  --name "MiniMax 中文站" \
  --base-url "https://api.minimax.cn/anthropic" \
  --kind anthropic \
  --api-key "sk-..." \
  --models "MiniMax-M3" \
  --agents "zcode,pi"

# 3. 先看清会发生什么
agentcenter providers diff

# 4. 确认后注入
agentcenter providers sync
```

**同步前必须退出 ZCode / Pi / opencode。** 它们运行时会用内存态覆盖配置文件，
你改了也会被冲掉。

## 配置文件

`~/.agents/agentcenter.toml`，跟着 git 一起版本管理。只写要覆盖的部分，没写的用内置默认值。

```toml
[source]
instructions = "~/.agents/AGENTS.md"

[[mappings]]
agent = "claude"
to    = "~/.claude/CLAUDE.md"
mode  = "auto"          # auto / symlink / junction / hardlink / copy

[skills.tags]
ui-design       = ["前端", "设计"]
deepseek-search = ["搜索", "联网"]
```

在 `[[mappings]]` 里写一个内置表没有的 `agent`，就能接入任何新工具，不用等工具更新版本。

## 链接策略

Windows 上建符号链接需要管理员权限或开发者模式；批量建 junction 和硬链接不需要。所以默认按这个顺序降级：

```
目录（skills）   symlink → junction → 报错
单文件（AGENTS.md） symlink → hardlink → copy + 指纹
```

每一级的结果都会明确报告，不会静默降级。界面和 `status` 都能看到实际用的是哪种。

建链接前，已存在的真实文件会被移进 `~/.agents/backup/<时间戳>/`，不删除、不覆盖。如果内容和母版不同，会额外提示。

## 几个刻意的设计决定

**API key 体检是只读的，不写入。**

`agentcenter creds inspect` 和界面上的「API Key」页只做一件事：把散在各 agent 里的 key 汇总出来，让你看清分布。它不修改任何凭证文件，也不尝试读取系统凭据库。

返回给界面的数据里**没有密钥原文**，只有 SHA-256 指纹前 8 位和末尾 4 位。原文在 Rust 内存里只停留于算指纹的那一瞬间，连序列化的机会都没有。

这个边界是有意设的：凭证是最容易出事的地方，先把「看清楚现状」做扎实，比急着做「统一写入」更有价值。

**标签不写进 skill 目录，也不做成子目录。**
写进目录会让 `npx skills update` 报冲突；做成子目录会让 ZCode 读不到（它只扫 `<skills>/<名字>/SKILL.md` 两层）。所以标签存在配置文件里，磁盘上保持平铺。

**git 只给命令，绝不自动提交。**
`git commit` 逐个 `git add <路径>`，代码里没有 `git add -A`，也会拒绝仓库外的路径。界面上必须勾选路径才能提交。

**"版本"是内容哈希，不是版本号。**
`npx skills` 的 `.skill-lock.json` 里，GitHub 源记的是 git tree SHA，其它源是 SHA-256。界面展示指纹，不假装有语义版本。

**调用系统 git 而不是用库。**
你的 git 配置、钩子、凭据助手、SSH 密钥全部照常生效。

## 已知限制

- `npx skills update` 会按它自己的目录表写文件，可能把 junction 换回真实目录。更新完需要再跑一次 `agentcenter sync`。
- `skills check` 没有 JSON 输出，靠退出码判断（1 = 有更新），更新日志是彩色英文文本原样透传。
- 目前只覆盖 Windows。junction 那一级在其它平台不可用，会自动跳过到下一级。
- **Cursor 的 provider 管不了**。它的配置文件里没有任何 base_url / api_key 字段，
  模型走 Cursor 自己的服务端订阅。`agentcenter agents` 里它的 provider 列显示 `—`。
- **从母版彻底删掉一条 MCP server 时，agent 配置里的残留不会被清理**。
  注入器只能看到「池子里有、但没分配给我」的 id。要清理请先取消分配应用一次。
- Hermes 的配置目录优先读 `HERMES_HOME` 环境变量，取不到才用 `~/.hermes`。
  本机 `~/.hermes` 是空的（只有两个备份脚本），真正在用的是
  `C:\dev\ide\hermes-agent\Hermes Agent CN Desktop` 下的桌面版。
- **Gemini CLI、Grok Build、OpenClaw、Kimi Code、MiniMax Code 五个注入器是按
  cc-switch 的源码移植的，本机没装这些工具，尚未实测**。装好之后要对一遍真实文件。
- **Claude Desktop 没有注入器**。本机 `%LOCALAPPDATA%\Claude-3p` 不存在，
  结构虽已知但没验证，不做猜测实现。
- **opencode 的 `.jsonc` 注释会在写入时丢失**。用 `serde_json` 读写，
  带注释的文件会先剥注释再写回。要做保留注释的编辑需要引入 JSONC 文档层。
- **Codex 现在写内联 `experimental_bearer_token`**，不再生成
  `~/.agents/codex-env.ps1`。早期版本生成的这个脚本不再由工具维护。

## 目录结构

```
agentcenter/
├── src-tauri/                    Rust 后端
│   ├── src/
│   │   ├── main.rs               入口，有子命令走 CLI，没有则开窗口
│   │   ├── lib.rs                模块声明 + Tauri 命令注册
│   │   ├── cli.rs                clap 子命令定义
│   │   ├── cli_run.rs            子命令执行，输出终端文本
│   │   ├── commands.rs           Tauri IPC 命令
│   │   ├── types.rs              序列化结构，与前端 types.ts 一一对应
│   │   ├── config.rs             agentcenter.toml 读写
│   │   ├── link.rs               链接引擎（降级链、健康检查、备份）
│   │   ├── doc.rs                AGENTS.md 母版的读取与写入
│   │   ├── skills.rs             skill 收集、去重、标签
│   │   ├── skillscli.rs          npx skills 封装（含 ANSI 清理）
│   │   ├── credentials.rs        API key 只读体检（遍历 agents 聚合）
│   │   ├── git.rs                git 封装（按路径提交）
│   │   ├── agents/               每个 agent 一个实现文件
│   │   │   ├── mod.rs            Agent trait + 注册表（all/find/ids）
│   │   │   ├── model.rs          凭证临时记录与归一化
│   │   │   ├── claude.rs         Claude Code：provider(单槽位)+mcp+凭证+CLAUDE.md
│   │   │   ├── codex.rs          Codex：provider+mcp+凭证+AGENTS.md
│   │   │   ├── zcode.rs          ZCode：provider+mcp+凭证
│   │   │   ├── pi.rs             pi：provider+mcp+凭证
│   │   │   ├── opencode.rs       opencode：provider+mcp
│   │   │   ├── workbuddy.rs      WorkBuddy：provider+mcp
│   │   │   ├── dsh.rs            DSH：provider
│   │   │   ├── hermes.rs         Hermes Agent：provider+mcp（读 HERMES_HOME）
│   │   │   ├── cursor.rs         Cursor：mcp
│   │   │   ├── qoder.rs          Qoder：mcp
│   │   │   ├── gemini.rs         Gemini CLI：provider（按 cc-switch 移植）
│   │   │   ├── grok.rs           Grok Build：provider（按 cc-switch 移植）
│   │   │   ├── openclaw.rs       OpenClaw：provider（按 cc-switch 移植）
│   │   │   ├── kimi.rs           Kimi Code：provider（按 cc-switch 移植）
│   │   │   ├── minimax.rs        MiniMax Code：provider（按 cc-switch 移植）
│   │   │   └── ...               其余只有路径与 skills 的 agent
│   │   └── providers/            provider 母版、注入调度、分配矩阵
│   │       ├── mod.rs            Injector trait + 同步调度
│   │       ├── model.rs          Provider / ModelSpec / FilePlan / Diff
│   │       ├── store.rs          providers.toml 读写 + 头部注释保留
│   │       ├── safe_write.rs     五道安全闸（hash 复查、备份、原子写）
│   │       ├── scan.rs           从现有配置读 provider 和模型元数据
│   │       ├── backfill.rs       把读到的元数据补回母版空字段
│   │       ├── diff.rs           母版 vs 现状对比
│   │       ├── matrix.rs         provider 池 × agent 分配矩阵
│   │       ├── hermes.rs         Hermes 注入器（custom_providers 列表，行级编辑）
│   │       ├── zcode.rs          ZCode 注入器（config.json + provider_config.json）
│   │       ├── pi.rs             Pi 注入器（models.json，key 内联）
│   │       ├── opencode.rs       opencode 注入器（.jsonc 优先，key 内联）
│   │       ├── codex.rs          Codex 注入器（单槽位 custom 表 + 内联 token）
│   │       ├── claude.rs         Claude 注入器（关键字段清单 + 代理别名）
│   │       ├── claude_bridge.rs  本地协议转换（Messages → Chat/Responses）
│   │       ├── claude_onboarding.rs  Claude 首次引导开关（~/.claude.json）
│   │       ├── dsh.rs            DSH 注入器（4 个 profile + credentials）
│   │       ├── workbuddy.rs      WorkBuddy 注入器（扁平模型数组）
│   │       ├── gemini.rs         Gemini CLI 注入器（.env + settings.json）
│   │       ├── grok.rs           Grok Build 注入器（model 表）
│   │       ├── openclaw.rs       OpenClaw 注入器（models.providers）
│   │       ├── kimi.rs           Kimi Code 注入器（providers + models 别名）
│   │       ├── minimax.rs        MiniMax Code 注入器（custom_provider）
│   │       └── injector_test.rs  关键字段边界与协议映射的单测
│   ├── capabilities/             Tauri 2 权限声明
│   ├── icons/                    应用图标
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                          前端（Svelte 5）
│   ├── main.ts                   挂载入口
│   ├── App.svelte                外壳：标题栏 + 侧栏导航 + 页面切换
│   ├── app.css                   Tailwind 主题与 CSS 变量
│   ├── pages/
│   │   ├── Mappings.svelte       AGENTS 文档映射
│   │   ├── Skills.svelte         中心库与标签
│   │   ├── Store.svelte          npx skills 更新
│   │   ├── Providers.svelte      Provider 池 + 分配
│   │   ├── ClaudeCodex.svelte    Claude 引导开关 + Codex 后端开关
│   │   ├── Mcp.svelte            MCP server 池 + 分配
│   │   └── Git.svelte            版本管理
│   └── lib/
│       ├── ipc.ts                前后端通信（Tauri 或假数据）
│       ├── mock.ts               浏览器预览的假后端
│       ├── types.ts              共享类型，与 types.rs 对应
│       ├── utils.ts              cn() 类名合并
│       └── components/           Button / Badge / Table / Log / PageHead / TitleBar
├── scripts/                      开发脚本
│   ├── build-release.mjs         停进程 → 构建前端 → 编译 release
│   ├── free-port.mjs             释放被占的 5731 端口
│   ├── make-icon.mjs             生成应用图标
│   ├── check-contract.mjs        前后端类型契约检查
│   ├── check-desc-length.mjs     统计 skill 描述长度
│   ├── scan-credentials.mjs      扫描凭证存储结构
│   ├── find-duplicate-keys.mjs   检测重复 key
│   ├── make-test-provider.mjs    生成测试母版
│   ├── test-zcode-inject.mjs     ZCode 注入器测试
│   ├── test-multi-inject.mjs     Pi / opencode 注入器测试
│   ├── test-claude-codex.mjs     Claude / Codex 注入器测试
│   ├── test-hermes-zcode.mjs     Hermes / ZCode 注入器与 MCP 测试
│   ├── test-protocol-e2e.mjs     按 agent 覆盖协议的端到端测试
│   ├── test-agent-kinds.mjs      同一 provider 不同协议
│   ├── test-models.mjs           模型元数据下发与回填
│   ├── test-workbuddy-dsh.mjs    WorkBuddy / DSH 注入器测试
│   ├── test-provider-matrix.mjs  分配矩阵与移除语义
│   ├── test-mcp-*.mjs            MCP 注入 / 矩阵 / 差异测试
│   └── test-toml-comments.mjs    TOML 注释保留测试
├── plan/
│   ├── README.md                 实施计划与验收记录
│   └── providers-management.md   provider 管理专项计划
├── npm/                          npm 分发包装（bin 转发 + 平台 exe）
└── package.json
```

### 怎么新增一个 agent

一个 agent 的全部能力收在一个文件里。新增只需要两步。

**第一步**：新建 `src-tauri/src/agents/<id>.rs`。trait 里除身份和路径外
都有默认实现，不支持的项直接不写：

```rust
use super::Agent;
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct Gemini;

impl Agent for Gemini {
    fn id(&self) -> &'static str { "gemini" }
    fn label(&self) -> &'static str { "Gemini CLI" }
    fn config_dir(&self) -> PathBuf { home_dir().join(".gemini") }
    fn skills_dir(&self) -> PathBuf { home_dir().join(".gemini").join("skills") }

    // 支持 provider 的话再加这些
    fn supports_provider(&self) -> bool { true }
    fn provider_ready(&self) -> bool { home_dir().join(".gemini").join("settings.json").exists() }
    fn provider_targets(&self) -> Vec<PathBuf> { vec![home_dir().join(".gemini").join("settings.json")] }
    fn provider_plan(&self, providers: &[crate::providers::model::Provider])
        -> anyhow::Result<Vec<crate::providers::model::FilePlan>> {
        // 把母版的 provider 翻译成这个 agent 的配置形态
    }
}
```

**第二步**：在 `agents/mod.rs` 的 `all()` 里加一行
`Box::new(gemini::Gemini)`。

改完跑 `agentcenter agents` 自检：清单、路径、能力标记都会列出来。

**不需要动的**：`providers/matrix.rs`（列从 `agents::provider_capable()` 生成）、
`providers/scan.rs`、`providers/diff.rs`（路径从 `provider_targets()` 取）、
`mcp/matrix.rs`、`mcp/scan.rs`、`mcp/diff.rs`、`credentials.rs`（遍历
`agents::all()`）、`config.rs`、`skills.rs`。重构前这些每处都有一份
agent 清单，新增一个 agent 要改 10 个文件，而且已经互相漂移了。

### 注入器的统一抽象

provider 的注入逻辑仍按 agent 分成 `providers/<id>.rs`，都实现
同一个 `Injector` trait（`agents/<id>.rs` 委托给它）：

```rust
pub trait Injector {
    fn agent(&self) -> &'static str;
    fn targets(&self) -> Vec<PathBuf>;          // 有些 agent 要改两个文件
    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>>;
}
```

`plan` **只读**，产出「文件应该变成什么样」的计划。真正写入统一走
`safe_write`，所以每个注入器不需要自己处理备份、hash 复查、原子写。

同步的调度顺序是「**先全部 plan，再全部写**」：任何一个 agent 规划失败，
整次同步中止，不写任何文件。这避免了"前三个改完、第四个失败"的半同步状态。

### 测试用什么隔离真实配置

Windows 上 `dirs::home_dir()` 走 `SHGetKnownFolderPath`，改 `USERPROFILE`
是无效的。所以工具支持 `AGENTCENTER_HOME` 环境变量整体重定向家目录，
所有路径解析都经过 `store::home_dir()`：

```bash
AGENTCENTER_HOME=C:\temp\provider-test\home agentcenter providers sync
```

所有注入器测试都用这个机制，真实配置完全不受影响。
