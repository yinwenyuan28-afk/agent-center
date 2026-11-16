# agentcenter 实施计划

统一管理多个 AI coding agent 的全局配置：指令文件映射、skill 收集分类、第三方 skill 更新、git 版本管理。

## 计划文件索引

| 文件 | 内容 | 状态 |
|---|---|---|
| 本文件 | 指令映射、skill 管理、git 集成、API key 只读体检 | 已实施 |
| [providers-management.md](providers-management.md) | provider 统一管理与注入（母版、七个注入器、五道安全闸） | 已实施 |
| [provider-config-keys.md](provider-config-keys.md) | 各工具改哪个文件、哪些键；CC Switch 已核对，我们的实现标待确认 | 记录 |
| [provider-models-and-agent-interface.md](provider-models-and-agent-interface.md) | 模型元数据管理 + Agent 接口化重构 | 已实施 |
| [token-usage-stats.md](token-usage-stats.md) | token 用量统计：四家 session 位置、口径归一、按需读取 | 已实施 |
| [agent-config-management-reference.md](agent-config-management-reference.md) | 各 agent 的 AGENTS.md / provider / MCP 分别改哪个文件、怎么改；CLI 全量清单、agent 化缺口与建议补的能力 | 参考 |

## 要解决的问题

本机同时装了 8 个以上的 AI coding agent，每个 agent 读指令文件的位置和文件名都不一样：

| 文件 | 字节 | 内容 |
|---|---|---|
| `~/.claude/CLAUDE.md` | 2209 | 编码约定、Java 环境、计划先行 |
| `~/.zcode/AGENTS.md` | 2369 | 全局工作方法、文件归档、禁止黑话 |
| `~/.cursor/AGENTS.md` | 1553 | 通用开发准则、测试用 sqlite |
| `~/.config/opencode/AGENTS.md` | 2120 | 编码环境、按需加载原则 |
| `~/.pi/agent/AGENTS.md` | 600 | reasoning 写法 |
| `~/.codex/AGENTS.md` | 0 | **空文件** |

这些内容互不相同，也没有单一真实来源。同样的问题出现在 skills 上：`~/.agents/skills` 下有 18 个 skill，但 `.cursor/skills`(7)、`.config/opencode/skills`(2)、`.workbuddy/skills`(1 个 junction) 这些散落的副本没有进共享库。

agentcenter 提供一个入口，把上面这些收敛起来，并给一个桌面界面便于日常操作。

## 环境前置

本机当前状态：**没有 Rust，没有 MSVC 构建工具**，所以 Tauri 编译不了。

| 依赖 | 状态 | 处理 |
|---|---|---|
| WebView2 运行时 | 已有 157.0.4341.42 | 无需处理 |
| Node / npm | 已有 24.17.0 / 11.13.0 | 无需处理 |
| Microsoft C++ Build Tools | **缺失** | 用户手动装，勾选 "Desktop development with C++" |
| Rust (rustup) | **缺失** | 用户手动装，之后 `rustup default stable-msvc` |
| Tauri CLI | 缺失 | 走 `npm i -D @tauri-apps/cli`，不装 cargo-tauri |

安装顺序：**先 Build Tools，再 Rust**。Rust 安装时会检测链接器，顺序错了会装成 GNU 工具链。

验证（新开终端）：

```
rustc --version
cargo --version
```

## 技术选型

- **后端**：Rust，一个二进制两种运行方式。带子命令走 CLI 输出文本，不带子命令开 GUI 窗口。这样 npm 只需分发一个 exe。
- **前端**：Svelte 5 + Vite + TypeScript，不引组件库，样式手写。选它是因为后续要加 MCP / cursor rules / API key 管理，页面数量和状态复杂度都会上去，纯手写 DOM 会变成体力活。
- **配置**：TOML，放在 `~/.agents/agentcenter.toml`，跟着 git 一起版本管理。
- **git**：调用系统 `git` 命令，不用 Rust git 库。保证用户的 git 配置、钩子、凭据照常生效。

## 目录结构

```
C:\dev\code\agentcenter\
├── src-tauri\
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src\
│       ├── main.rs          入口：有子命令走 CLI，没有则开窗口
│       ├── cli.rs           clap 子命令定义
│       ├── config.rs        agentcenter.toml 读写
│       ├── registry.rs      各 agent 的默认路径表
│       ├── link.rs          链接策略与健康检查
│       ├── skills.rs        收集、去重、标签索引
│       ├── skillscli.rs     调用 npx skills
│       ├── git.rs           调用 git 命令
│       └── commands.rs      Tauri IPC 处理函数
├── src\                     Svelte 5 前端
│   ├── main.ts
│   ├── App.svelte
│   ├── app.css
│   ├── lib\ipc.ts           Tauri invoke 的类型化封装
│   └── pages\
│       ├── Mappings.svelte  指令映射
│       ├── Skills.svelte    skill 列表与标签
│       ├── Store.svelte     npx skills 更新
│       └── Git.svelte       git 状态与提交
├── npm\
│   ├── package.json         bin 指向 bin/agentcenter.js
│   └── bin\agentcenter.js   按平台找 exe 并 spawn
└── plan\
    └── README.md            本文件
```

## 核心设计

### 配置文件

放在 `~/.agents/agentcenter.toml`。内置注册表提供默认值，这个文件只写要覆盖的部分。

```toml
[source]
instructions = "~/.agents/AGENTS.md"

[[mappings]]
agent = "claude"
to    = "~/.claude/CLAUDE.md"
mode  = "auto"
disabled = false        # true = 用户取消了这条，sync 跳过

[[mappings]]
agent = "zcode"
to    = "~/.zcode/AGENTS.md"

[skills.tags]
ui-design       = ["前端", "设计"]
deepseek-search = ["搜索", "联网"]
```

**标签为什么不写进 skill 目录**：那 6 个 skill 由 `npx skills` 管理，改动目录内文件会让下次 `npx skills update` 产生冲突。录在外部索引里，第三方 skill 目录保持原样。

**标签为什么不能变成子目录**：ZCode 只扫 `~/.agents/skills/<名字>/SKILL.md` 两层，加分类目录就读不到。磁盘上必须平铺。

### agent 路径表

只收录确实有单一指令文件的 agent，不猜路径。目录不存在则自动跳过，不创建目录。

| agent | 指令文件 | skills 目录 | 原生读 `~/.agents/skills` |
|---|---|---|---|
| claude | `~/.claude/CLAUDE.md` | `~/.claude/skills` | 否，需挂载 |
| codex | `~/.codex/AGENTS.md` | `~/.codex/skills` | 是 |
| zcode | `~/.zcode/AGENTS.md` | `~/.zcode/skills` | 否，需挂载 |
| cursor | `~/.cursor/AGENTS.md` | `~/.cursor/skills` | 是 |
| opencode | `~/.config/opencode/AGENTS.md` | `~/.config/opencode/skills` | 是 |
| pi | `~/.pi/agent/AGENTS.md` | `~/.pi/agent/skills` | 是 |
| dsh | `~/.dsh/AGENTS.md` | 无 | 未知 |
| zed | `%APPDATA%\Zed\AGENTS.md` | 无 | 未知 |

qoder / codebuddy / workbuddy 没有单一指令文件（workbuddy 是 SOUL/IDENTITY/USER/MEMORY 一整套），只在 skills 层面参与。

### 链接策略：自动降级

本机开发者模式未开启（`AllowDevelopmentWithoutDevLicense` 不存在），不在管理员组，所以符号链接会失败。每一级都明确报告实际用了哪种，不静默：

```
目录（skills）
  symlink_dir → junction（无需权限，同盘）→ 报错

单文件（AGENTS.md）
  symlink_file → hard_link（无需权限，同盘）→ copy + 指纹
```

- 建链接前把已存在的真实文件移到 `~/.agents/backup/<时间戳>/`，不删除、不覆盖
- 若内容与母版不同，额外提示合并
- `copy` 模式写 `.agentcenter-manifest.json` 记录 SHA-256，`status` 靠它判断漂移
- `status` 报三态：`ok` / `drifted` / `broken`
- 取消映射（`unmap`）走同一套边界：链接只删链接、硬链接只删目录项、改过的副本进备份；
  同时在配置里记 `disabled = true`，否则下次 `sync` 会立刻建回来

### skill 收集

`agentcenter collect` 默认只预览，加 `--apply` 才动手。

| 情况 | 处理 |
|---|---|
| 目标里没有同名 skill | 移入 `~/.agents/skills/`，原位置建 junction 指回来 |
| 同名且哈希相同 | 删掉副本，原位置建 junction |
| 同名但内容不同 | **不动**，列进冲突表交用户决定 |
| 目录是坏的链接 | 只报告，不自动删 |

批量按上表处理（「一键收集」），也可以点「扫描散落 skill」逐条选：移入 / 删副本改链接 /
采用散落版 / 采用中心库版 / 删除 / 删除坏链接。详见「2026-11-12 映射取消 + 散落 skill 逐条处置」。

### 集成 npx skills

调查报告里的三个硬约束：

1. **不固定写死 CLI 路径**。优先找全局 `skills`，找不到用 `npx skills`。绝不硬编码 `_npx\<hash>\...`，那个目录会被清掉。
2. **"版本"是内容哈希不是版本号**。`.skill-lock.json` 里 GitHub 源是 git tree SHA，其它源是 SHA-256。界面展示「来源仓库 + 装置时间 + 哈希前 8 位」，不假装有 v1.2.3。
3. **`update`/`check` 没有 JSON**。`check` 有更新时退出码为 1；`update` 输出带颜色的英文文本，原样透传到界面日志区。

**已知风险**：`npx skills update` 会按自己的目录表写文件，可能把 junction 换回真实目录。更新完必须再跑一次 `agentcenter sync`。

### git 集成

只给命令，绝不自动提交。

- `agentcenter git status` — 分支、领先/落后、改动文件清单
- `agentcenter git commit --paths a,b,c -m "msg"` — 只加指定路径，代码里不出现 `git add -A`
- `agentcenter git push` — 推当前分支

界面上的提交框默认列出本次操作碰过的文件，可勾选，不勾的不提交。

## 实施阶段

每个阶段独立验收，不用等全部做完。

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 环境准备：Build Tools + rustup | **待用户安装**，Rust 与 MSVC 均未装 |
| 1 | 链接引擎 + 指令映射 | 代码完成，前端已验收；待 Rust 环境就绪后编译验证 |
| 2 | skill 收集与标签 | 代码完成，待编译验证 |
| 3 | npx skills 集成 | 代码完成，待编译验证 |
| 4 | git 集成 | 代码完成，待编译验证 |
| 5 | Svelte 界面四个页面 | **已验收**：浏览器里跑通全部四页与按钮交互 |
| 6 | npm 打包 | 转发层已写好，待编译出 exe 后验证 |

### 已完成的验收记录

2026-11-09：

- `npm install` 装了 49 个包，无报错
- 浏览器实测四页：指令映射 8 行正确渲染、Skills 18 项与 19 个标签正确、Skill 更新 6 项受管、Git 空状态正确
- 交互验证：点「同步全部」后日志区正确回显假后端返回

2026-11-09 第二轮（Rust 环境就绪后）：

- Rust 1.99.1 + `stable-x86_64-pc-windows-msvc` 工具链确认可用
- crates.io 直连会卡在 `Updating crates.io index`，已配置 rsproxy 镜像（`~/.cargo/config.toml`）
- 484 个依赖全部拉取并编译通过
- **`cargo check` 无错误无警告**
- 前端换成 Tailwind + shadcn 风格：品牌绿 `#10a37f`、侧栏 210px、圆角 10px、暗色主题跟随系统
- 构建产物：CSS 16.76 kB（gzip 4.43 kB），JS 115.48 kB（gzip 38.80 kB）

### 已修复的问题

1. **前后端字段名不一致**：Rust 默认序列化成 snake_case，前端类型是 camelCase，界面会拿到 undefined。给 7 个结构体加了 `#[serde(rename_all = "camelCase")]`，并写了 `scripts/check-contract.mjs` 做机械比对，挂进 `npm run verify`。
2. **组件注释被当文本渲染**：Svelte 组件顶部的 `/** */` 写在 `<script>` 外面会被当成模板文本输出。四个基础组件都改为注释放在 `<script>` 内部。
3. **Vite 监听 Rust 编译产物导致崩溃**：`cargo build` 写 `src-tauri/target/` 时文件句柄被占用，Vite 的文件监听抛 EBUSY 直接把开发服务器打崩。已在 `vite.config.ts` 的 `server.watch.ignored` 里排除 `src-tauri/target/**` 和 `src-tauri/gen/**`。
4. **`fs::read_dir(dir).flatten()` 编译错误**：`Result` 不是迭代器。改为先 `let Ok(entries) = ...` 再对 `ReadDir` 调 `flatten()`。
5. **缺应用图标**：`tauri-build` 要求 `icons/icon.ico`。写了 `scripts/make-icon.mjs` 手写 PNG/ICO 编码生成，不引原生图形库。

### 待修的问题

- `CollectPlan` 的 `apply_plan` 执行日志目前被丢弃，只返回计划结构。前端 Skills 页的「一键收集」按钮因此只显示「收集完成」而没有逐条明细 —— 单条处置有逐条回执，批量那个还没有。需要在类型里加日志字段。

### 2026-11-12 映射取消 + 散落 skill 逐条处置

**需求**

1. AGENTS 文档映射要能取消，也就是删掉那条硬链接。
2. Skills 页点「扫描散落 skill」后弹对话框，列出各 agent 自己目录里散落的 skill，并提供选项逐条处置。

**1. 取消映射**

只删链接是不够的：下一次「同步全部」会立刻把链接建回来，用户会以为按钮没生效。所以取消动作
是「删文件 + 写标记」两步。

- `link::remove_one` 负责删，边界和 `sync_one` 对称，母版一律不碰：
  链接 → 只删链接；硬链接 → 删目录项（母版那份仍在）；内容相同的副本 → 直接删；
  内容不同或被改过 → 移进 `~/.agents/backup/` 再报告位置
- `agentcenter.toml` 的 `[[mappings]]` 增加 `disabled` 字段。取消时写入，同步时跳过
- 取消后再恢复，若这条 override 和内置默认完全一致（路径相同、mode 是 auto），整条删掉，
  配置文件回到「没写过这个 agent」的干净状态，不留 `disabled = false` 的垃圾条目
- CLI：`agentcenter unmap <agent>` / `agentcenter remap <agent>`；`status` 里已取消的项显示提示且不计入「需处理」
- 界面：每行一个「取消映射」，确认框写清后果；取消后该行显示「已取消」徽标和「恢复映射」按钮

**2. 散落 skill 逐条处置**

`OrphanSkill` 补了描述、文件数、字节数、指纹四个字段 —— 冲突时用户要靠这些信息判断采用哪一份，
只给路径看不出来。

新增 `OrphanAction` 六种动作，界面按扫描出的关系只给可用的那几个：

| 关系 | 可选动作 |
|---|---|
| `unique` | `import` 移入中心库 / `delete` 删除 |
| `identical` | `dedupe` 删副本改链接 / `delete` |
| `conflict` | `take-orphan` 采用散落版 / `take-shared` 采用中心库版 / `delete` |
| `broken-link` | `remove-link` 删除坏链接 |

`skills::apply_action` 的特点是**每条都重新校验现场**：对话框开着的时候用户完全可能在资源管理器里
动过文件，不能信任界面拿着的旧扫描结果。采用散落版覆盖中心库时，中心库那份先复制进备份目录。

只有「删除」不进备份 —— 那是用户明确要求的真删除，确认框里写明了这一点。

「一键收集」保留，只处理 `unique` 和 `identical`；冲突仍要人逐条决定。

**验证**

- `link::remove_one` 五种目标形态（硬链接 / 相同副本 / 改过的副本 / 目标不存在 / 不同的目录）
  用一个临时 example 全部跑通，测试产生的备份已从 `~/.agents/backup/` 回收
- `cargo check` 无错误无警告；`svelte-check` 0 错误；`npm run build` 通过
- 界面交互在浏览器预览（假后端）里可验证：取消后状态变「已取消」并出现「恢复映射」，
  散落对话框按关系给出不同按钮、处置后该条从清单消失并出现在处置记录里

### 2026-11-09 第三轮改动（用户反馈）

按用户要求调整：

1. **精简 skill 标签**：mock 数据里原来给 18 个 skill 都编了标签（前端/设计/office/学习/思考 等），这是我擅自加的。改为只保留三个：`UI设计`（ui-design、frontend-design）、`搜索`（deepseek-search）、`文档`（agents-md、ai-text-readable、project-docs-*），其余全部留空。
2. **Skills 页新增两套筛选**：
   - 标签筛选增加「未分类 N」项，专门筛出没有标签的 skill（用哨兵值 `__untagged__` 区分，避免与真实标签撞名）
   - 新增来源筛选：全部 / 第三方（`managed: true`）/ 本地自建
   - 顶部显示「显示 M / N」计数
3. **页面改名**：导航与标题「指令映射」→「AGENTS文档映射」。
4. **新增母版内容区**（位于映射表格上方）：显示 `~/.agents/AGENTS.md` 的内容、行数与字节数，带「编辑 / 保存 / 取消」；文件不存在时显示创建入口。Rust 侧新增 `doc.rs` 模块与 `read_doc` / `save_doc` / `create_doc` 三个命令，`create` 在文件已存在时拒绝覆盖。
5. **删除 `~/.claude/CLAUDE.md`**：已移入回收站，内容备份在 `C:\temp\agentcenter-cleanup-20261109\CLAUDE.md.backup`（2209 字节）。

**注意**：`.claude/CLAUDE.md` 里有 5 条规则是 `.zcode/AGENTS.md` 没有的（UTF-8 编码、改代码前先给计划并确认、java8 环境路径、不经同意不跑 maven/npm、若依风格 CRUD）。按「以 .zcode 为准」执行后这些不再生效，需要时从备份取回合并。

**验证结果**（浏览器实测）：
- 导航与 h1 均为「AGENTS文档映射」
- 标签 pills：文档 / UI设计 / 搜索 / 未分类 11
- 来源 pills：全部 / 第三方 / 本地自建
- 点「未分类」→ 11 行；点「第三方」→ 6 行；两者组合 → 6 行
- 文档区显示 8 行 389 字节，点「编辑」后出现 textarea（155 字符），按钮变为保存/取消
- `cargo build` 与 `npm run build` 均通过，`svelte-check` 0 错误

### 2026-11-10 新增：API key 体检

用户提出「统一管理所有 agent 的不同 provider 的 API key」。先做了只读体检，没有做写入。

**为什么先只读**：凭证是最容易出事的地方。先让用户看清现状（有几把 key、散在哪、哪些重复），
比急着做统一写入更有价值。写入要处理备份、格式回写、冲突检测，风险高得多。

**实测数据**（`agentcenter creds inspect`）：

- 19 条凭证，16 个不同 key
- 2 组重复：CommandCode key 在 claude/zcode/pi 三处；MiniMax CN key 在 zcode/pi 两处
- 2 个 provider 配了多把 key：`command`（3 处同一把 + 环境变量另一把）、`deepseek`（pi 的 auth.json 与 DEEPSEEK_API_KEY 是两把不同的）
- 4 个 agent 读不到明文：cursor（系统凭据库）、qoder / workbuddy / codebuddy（浏览器 OAuth）

**实现要点**：

- `credentials.rs` 纯只读。密钥原文在 Rust 内存里只用于算 SHA-256 前 8 位指纹，绝不进入返回结构
- `normalize_provider` 做服务名归一化：环境变量 `DEEPSEEK_API_KEY`、pi 的 `deepseek`、base URL 都归到同一个 provider，否则同一服务会被拆成多组，用户看不出「配了两把」
- 统计「一个 provider 有几把 key」必须用 `BTreeSet` 去重，不能用 `Vec::dedup()` —— 后者只去掉相邻重复

**界面**：新增「API Key」页，按 provider 分组展示，重复项标黄，多把 key 的 provider 打徽标，底部列出读不到明文的 agent 及原因。带「只看重复」筛选。

**契约检查**：11 个类型 61 个字段一致。

### 2026-11-10 Provider / MCP 统一交互：池 + 分配

用户提出「参考 mcp 的设计（统一管理，逐个注入的模型），修改 provider 的设计」。

**这次改了什么**

1. **Provider 从两页合一页。** 原来「Provider 母版」改配置、「Provider 同步」看差异再注入，
   两个页面职责割裂。现在和 MCP 一致：一个池子，每行一个「分配」按钮展开 agent 卡片，
   顶部一个「应用到各 agent」。`src/pages/ProviderSync.svelte` 已删除，侧边栏 7 项收到 6 项。

2. **分配面板不用 checkbox。** 卡片式按钮，点整块切换，选中实心绿 + 对勾。
   默认不展示矩阵，只列 provider——满屏勾选框太吵。

3. **注入器补上「取消分配就移除」。** 这是最关键的行为变化。原来所有注入器只增改不删，
   所以界面上取消勾选没有任何效果。现在每个注入器都会把「母版里有、但没分配给本 agent」
   的条目从配置文件里移除，但仍然只删母版里认得出的，用户手配的和内置模板不碰。

4. **Claude 单槽位互斥。** Claude Code 只有一组 `ANTHROPIC_*` 环境变量，同一时刻只能一个
   provider 生效。勾某个 provider 的 Claude 时，后端自动把 claude 从其他 provider 的
   `agents` 里摘掉，否则母版会有多条都声明注入 claude 而注入器只取第一个，界面勾选和
   实际行为对不上。

**验证**：`scripts/test-provider-matrix.mjs` 在 `AGENTCENTER_HOME` 隔离的副本上跑 10 项全通过
（分配写入、未分配不写入、取消后移除、原有 9 个 provider 保留、幂等、claude 键清空与保留）。
Claude 互斥另用 `providers assign` 双向验证。界面在浏览器里实测：默认无表格无 checkbox、
展开 7 张卡片、选中 `rgb(16,163,127)` 实心绿、未安装的 DSH 灰掉禁用。

**修掉的坑**：Codex 在没有 provider 分配时仍会建空表 + 写空 env 脚本，把「无改动」变成
「写入」；改成没有要分配的就不建表不建脚本，已存在的脚本清成只有注释（留旧 key 会让
Codex 连错端点）。

### 2026-11-12 修掉双击启动的黑窗口

**现象**：每次启动 `target\release\agentcenter.exe` 都带一个黑窗口。

**根因**：读 PE 头确认 `Subsystem = 3`（console 子系统）。console 子系统程序在
**进程创建那一刻**就由 Windows 分配控制台 —— 我们的代码还没开始跑，窗口已经被
画出来了。所以任何「事后隐藏/销毁」都只能缩短它，消不掉闪现。

**试错记录（两条都不行）**

1. `ShowWindow(SW_HIDE)` 隐藏窗口 + `GetConsoleProcessList` 判独占。
   自动验证不出来：探测脚本连对照组（`cmd /c timeout`）都报 0 个可见控制台。
   改全量枚举做 A/B 才拿到结论 —— 窗口类名是 `CASCADIA_HOSTING_WINDOW_CLASS`，
   即 Windows Terminal 托管，`GetConsoleWindow()` 只返回伪句柄，真正的可见窗口
   归 `WindowsTerminal.exe`，藏不掉。A/B 显示对照组和实验组都新增 1 个可见窗口。
2. `FreeConsole()` 解绑。A/B 显示新增可见控制台 0 个 —— **机制有效**，但用户
   实测仍能看到闪现：控制台由系统在启动瞬间创建，解绑发生在其后。证明「减少
   存活时间」不等于「消除」。

**最终方案**：改成 windows 子系统（PE `Subsystem = 2`），系统根本不分配控制台。
CLI 输出靠 `console::ensure_output_channel()` 补回：

1. 先看 `GetStdHandle(STD_OUTPUT_HANDLE)` 是否可用。可用就原样返回 —— 它可能指向
   终端、重定向文件或管道，**不能改绑 `CONOUT$`**，否则重定向和管道全失效。
2. 不可用才 `AttachConsole(ATTACH_PARENT_PROCESS)` + 重新绑定标准句柄到 `CONOUT$`。

**顺带纠正一条错的旧注释**：`main.rs` 原写着「AttachConsole 在父进程是等待中的 cmd
时经常拿不到，导致 status 静默失败」。探针实测 AttachConsole 返回 true、CONOUT$ 能
打开、stdout 写入 flush 均成功。当初静默失败的真实原因是附加后没有重新绑定标准句柄，
std 缓存的仍是启动时的无效值。

**验证**

| 场景 | 结果 |
|---|---|
| PE 头 | `Subsystem = 2`（GUI） |
| 双击启动黑窗口（A/B 对照 `cmd /c timeout`） | 对照新增 1 个可见控制台，agentcenter 新增 **0** |
| `agentcenter mappings` 直接运行 | 11 行正常输出 |
| `agentcenter mappings > out.txt` | 11 行写入文件 |
| `agentcenter mappings \| findstr` | 管道正常 |
| `status` 退出码 | 0 正确传回 cmd |
| 非法子命令退出码 | 2 正确传回 |

新增 `src-tauri/src/console.rs`；`main.rs` 加 `#![windows_subsystem = "windows"]`，
CLI 分支调用 `ensure_output_channel()`。

### 2026-11-12 修掉点 Git 页时连闪黑窗口

**现象**：改成 GUI 子系统后主窗口不再闪了，但点侧栏「Git」会连闪好几个黑窗口。

**根因**：这是上一步修复的必然副作用。主程序现在没有控制台，而 Windows 的规则是
「无控制台的进程启动 console 子系统子进程时，系统给子进程新建控制台窗口」。
`git::status()` 一次要跑四条命令（`rev-parse`、`rev-list`、`remote get-url`、
`status --porcelain`），所以连闪四个。

**修复**：新增 `src-tauri/src/proc.rs`，提供 `new` / `output` / `status` 三个入口，
统一给子进程加 `CREATE_NO_WINDOW`（`0x0800_0000`）—— 告诉系统「建控制台但不显示窗口」。
把全部 7 处裸 `Command::new` 改过来：

| 文件 | 位置 | 子进程 |
|---|---|---|
| `git.rs` | `git()` | `git` |
| `link.rs` | junction 分支 | `cmd /C mklink` |
| `skillscli.rs` | `skills_command()` / `which_skills()` | `cmd` / `npx` / `skills` / `sh` |
| `providers/opencode.rs` | 版本探测两处 | `opencode` / `powershell` |

**验证**（关键是父进程必须真的没有控制台，否则子进程会继承终端的控制台，测不出区别）：

写了一个 `#![windows_subsystem = "windows"]` 的探针，和 agentcenter 完全同构，
内部连跑 3 条真实 git 命令，两种方式各跑一次，采样 9 次取峰值：

| 模式 | 峰值新增可见控制台 |
|---|---|
| 裸起（修复前行为） | **1 个** ← 复现问题 |
| `CREATE_NO_WINDOW`（修复后） | **0 个** |

CLI 回归：`agentcenter status` 输出正常、退出码正确。

**约束**：此后代码里不允许出现裸的 `std::process::Command::new`，新增子进程调用
必须走 `crate::proc`，否则黑窗口问题会重新出现。



## 验证方式

- **链接正确性**：同步后用 `dir /al` 和 PowerShell `Get-Item | Select LinkType,Target` 确认实际形态与工具报告一致
- **不破坏现有数据**：动手前记录 6 个指令文件 SHA-256 和 18 个 skill 目录哈希；同步后核对母版未变、无文件丢失
- **降级链生效**：在当前无开发者模式环境下跑，确认不弹 UAC 也能完成（走 junction / hardlink 分支）
- **幂等**：连续跑两次 `sync`，第二次必须零改动
- **git 不被偷改**：每次操作前后 `git status --short`，只允许预期内路径变化
- **npx skills 不被破坏**：`collect` 后跑 `npx skills list -g --json`，确认 6 个受管 skill 仍为 tracked

## 已知风险

1. **`~/.agents` 是 197 个文件的 git 仓库**，有远端 `git@github.com:yinwenyuan28-afk/.agents.git`，17 次提交，工作区干净。收集 skill 会改变磁盘布局，建议先 dry-run 看清单再放行。
2. **`npx skills update` 可能打断链接**。这是那个 CLI 自身的行为，工具只能做到更新后提醒重跑 sync，做不到阻止。
3. **`.codex/AGENTS.md` 是 0 字节空文件**。同步时会把它移进备份目录，然后建成指向母版的链接。如果 Codex 因为没有内容而正常工作，这一步不会有副作用；如果有其他机制依赖它存在，需要提前确认。
