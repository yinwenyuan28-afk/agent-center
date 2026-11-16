# agentcenter

统一管理多个 AI coding agent 的全局配置。

五个功能：

| 功能              | 做什么                                             |
| --------------- | ----------------------------------------------- |
| **AGENTS 文档映射** | 一份母版 `~/.agents/AGENTS.md`，链接到各 agent 自己的指令文件位置；可逐条取消与恢复 |
| **Skills 中心库**  | 收集散落在各 agent 目录的 skill，打标签分类；扫描后逐条决定移入 / 去重 / 采用哪一份 |
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
agentcenter unmap claude             # 取消某个映射：删链接，并让后续 sync 跳过它
agentcenter remap claude             # 恢复被取消的映射：立刻重建链接

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
agentcenter providers set <id> --codex-model deepseek/deepseek-v4.1-flash  # Codex 用哪个模型
agentcenter providers remove <id>
agentcenter providers example        # 输出示例母版
agentcenter providers doctor         # 检查 .gitignore 是否已忽略母版

agentcenter providers cursor status  # Cursor 自带密钥状态（开关/地址/密钥长度/模型）
agentcenter providers cursor plan command    # 看要让 command 生效还差什么
agentcenter providers cursor apply command --models a,b   # 写入并加模型
agentcenter providers cursor remove-model a,b             # 从模型选择器删模型
agentcenter providers cursor reset           # 关掉自带密钥，回到官方模型

agentcenter claude status            # Claude Code 是否会显示首次引导
agentcenter claude skip              # 跳过首次引导（写 ~/.claude.json 的 hasCompletedOnboarding）
agentcenter claude restore           # 恢复首次引导（删掉那个键）

agentcenter models backfill          # 从各 agent 现有配置回填模型元数据
agentcenter models backfill --dry-run # 只看将补哪些字段
agentcenter agents                   # 列出所有 agent 及其支持的能力

agentcenter usage status             # 各 agent 的用量统计状态与启用情况
agentcenter usage enable zcode       # 把某家纳入统计（默认一家都不统计）
agentcenter usage disable zcode      # 从统计中移除
agentcenter usage report --days 7    # 按 天 × agent × 模型 输出用量
agentcenter usage report --days 0    # --days 0 表示全部历史
agentcenter usage report --refresh   # 丢掉缓存，从头重算全部历史

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

### Codex 用哪个模型：得单独指定

Codex 的模型选择器（TUI 里 `/model`）只列它自己的官方目录 —— `codex debug models`
导出来只有 12 个 `gpt-*` 条目，自定义 provider 的模型一个都没有。原因是 provider
表里没有「模型列表」这个字段，唯一相关的 `model_catalog_url` 还不接受第三方域名。

所以「用哪个模型」只能由 `~/.codex/config.toml` 顶层的 `model` 决定。Codex 不校验
这个名字，会原样发给上游，找不到元数据只打一条 warning。母版里用 `codex_model`
指定，不填就回退到 `models` 的第一个：

```bash
agentcenter providers set command --codex-model deepseek/deepseek-v4.1-flash
agentcenter providers sync
```

界面上在「启用 provider」的 Codex 卡片里，按钮叫「配置模型」。

`model_reasoning_effort` 另有一个坑：这个值会被原样放进请求体的
`reasoning.effort`，上游只接受 `none|minimal|low|medium|high|xhigh|max`。母版里的
写法不一定对得上（比如 CommandCode 的 mimo 写的是 `enabled`），原样写进去每次请求
都会被拒。注入器只写落在白名单里的值，不在白名单就不写这个键，并把文件里残留的
不合法值删掉。

## 配置文件

`~/.agents/agentcenter.toml`，跟着 git 一起版本管理。只写要覆盖的部分，没写的用内置默认值。

```toml
[source]
instructions = "~/.agents/AGENTS.md"

[[mappings]]
agent = "claude"
to    = "~/.claude/CLAUDE.md"
mode  = "auto"          # auto / symlink / junction / hardlink / copy
disabled = false        # true 表示用户取消了这条映射，sync 时跳过

[skills.tags]
ui-design       = ["前端", "设计"]
deepseek-search = ["搜索", "联网"]

[usage]
# 纳入 token 用量统计的 agent。默认空 = 一家都不读。
# 用 `agentcenter usage enable <agent>` 或界面上的开关写入。
enabled = ["zcode", "pi"]
```

在 `[[mappings]]` 里写一个内置表没有的 `agent`，就能接入任何新工具，不用等工具更新版本。

`disabled` 由 `agentcenter unmap` / 「取消映射」按钮自动写入，一般不需要手改。取消后再恢复时，如果这条条目除了 `disabled` 之外和内置默认完全一样，工具会整条删掉，配置文件回到「没写过这个 agent」的状态。

## token 用量统计

从各 agent **自己写的 session 文件**里读用量，不额外记账。所以这一页只有读操作，唯一的写操作是切换某家是否纳入统计。

数据位置（都是实测确认的）：

| agent | 位置 | 结构 | 覆盖率 |
|---|---|---|---|
| ZCode | `~/.zcode/cli/db/db.sqlite` 表 `model_usage` | SQLite，一条一个模型调用 | 全部历史 |
| pi | `~/.pi/agent/sessions/<项目>/**/*.jsonl` | 每行一个事件，用量在 `message.usage` | 全部历史 |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | 每行一条 rollout，用量在 `token_usage_record.payload.usage` | **只有未压缩的 .jsonl** |
| Claude | `~/.claude/projects/<项目>/<会话>.jsonl` | 每行一个事件，用量在 `message.usage` | 全部历史 |
| Cursor | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` 表 `cursorDiskKV` | SQLite，键 `bubbleId:*` | **只能估算**，见下 |

### 统计是增量的

这些数据加起来上 GB（本机 pi 595 MB / 601 个会话文件 / 36,163 条记录，Cursor 的库 4.2 GB）。
每次都全量重读的话，打开一次统计页要等几十秒。所以每个 agent 的采集结果缓存在
`~/.agents/usage-cache/<agent>.json`，下次只处理新增的部分：

| agent | 增量依据 | 全量 → 增量 |
|---|---|---|
| pi | 会话文件是纯追加写的，记字节偏移，只解析新追加的行 | 1.72s → **0.17s** |
| Claude | 同上 | 0.08s → 0.06s |
| Codex | 同上，且「turn_id → 模型」的映射要跟着偏移一起存 | 0.06s → 0.05s |
| Cursor | `cursorDiskKV` 的 rowid 单调递增，记水位，只查 `rowid > 水位` | 3.55s → **0.06s** |
| ZCode | **不做增量**：全量查询 0.05 秒，而且它的行是「先插 running、跑完改 completed」，按 rowid 设水位会漏 | 0.13s → 0.12s |
| 五家一起 | | 37.00s → **0.26s** |

自动退回全量的情况：文件变短、文件前 4 KB 的哈希对不上（说明不是追加而是整体改写）、
缓存版本号变了。也可以在界面上点「全量重算」或跑 `usage report --refresh` 手动强制重算。

三条容易踩的实现细节：

- **Codex 的模型名不在用量记录里**，在前面 `turn_context` 行的 `payload.model`。只读文件尾巴时
  那些行早读过了，所以「turn_id → 模型」必须跟着偏移一起持久化。漏掉不会报错，只会让新增记录
  全部因为找不到模型被静默丢掉。
- **半行不能当记录吃掉。** 进程正在写时文件尾部可能只有半条 JSON。只有读到换行才推进偏移；
  最后那段没换行的内容，只有能完整解析才当作已读。
- **哈希不用 `DefaultHasher`。** 它的输出在不同 Rust 版本间不保证稳定，存到磁盘后换个编译器
  版本就全部对不上、缓存永久失效。所以手写 FNV-1a。

### Cursor 为什么只能估算

Cursor 按**请求次数**计费，不按 token 记账，所以本地没有单次 token 用量。实测三处都是空：

| 字段 | 数量 | 值 |
|---|---|---|
| `bubbleId:*` 的 `tokenCount` | 136,139 条 | 全部 `{inputTokens: 0, outputTokens: 0}` |
| `bubbleId:*` 的 `tokensUsed` | 753 条 | 全部 `null` |
| `agent-transcripts/*.jsonl` | 461 文件 37,988 行 | 完全没有 token 字段 |

唯一有真实值的是 `contextWindowStatusAtCreation.tokensUsed`（753 条非零）——
它记录「这条消息发出时上下文窗口里有多少 token」。同一会话内这个值递增：

```
03:40:38  tokensUsed=94595
03:41:23  tokensUsed=98691   +4096   ← 这条消息带来的新增
03:43:00  tokensUsed=107156  +8465
```

所以用**相邻快照的递增量**估算：只累加 `tu > prev` 的段（上下文压缩会让值回落，
那些段跳过）。实测 57 个会话里递增 629 次、递减 52 次。这是**下界**，
首条消息的绝对值不计、压缩掉的上下文也不计。`output` 本地完全没有记录，恒为 0。

界面上 Cursor 的说明文字里写明了是估算值，不是实际计费量。

### 折线图与饼图

- **折线图**：按模型分组，每天一个点，平滑曲线连接。不同模型不同颜色，
  颜色在两个图里保持一致
- **饼图**：环形图显示各模型占比，中心显示总量，右侧配明细列表
- 最多保留 8 个模型，其余合并成「其他」
- 手写 SVG，不引图表库（构建产物只增加约 9 KB）


### 口径为什么要归一

四家的 `input` 含义不一样，直接相加会算错：

| agent | `input` 是否含缓存 | 依据 |
|---|---|---|
| pi | **否** | 36163 条样本全部满足 `totalTokens = input + output + cacheRead + cacheWrite` |
| Claude | **否** | `cache_read_input_tokens` / `cache_creation_input_tokens` 是独立字段 |
| ZCode | **是** | `computed_total_tokens` 恒等于 `input_tokens + output_tokens`，`cache_read` 是其子集 |
| Codex | **是** | `total_tokens` 恒等于 `input_tokens + output_tokens`，`cached_input_tokens` 是其子集 |

所以统计时统一折算成「输入含缓存」：pi / Claude 的 `input + cacheRead + cacheWrite`，另外两家取原值。界面上的「输入（含缓存）」就是归一后的量，可以横向比较。

### 几个已核实的实现细节

- **Codex 的 `token_usage_record` 是单次请求的量，不是累计值。** 实测一个 97 条记录的会话里 `sum(每条 usage)` 与最后一条的 `thread_token_usage` 完全相等（input 8311231 / output 31731），所以逐条累加正确，不能再叠加 `turn_token_usage`。
- **全 0 token 的记录要丢掉。** pi 里占 3.4%（本机 36163 条中 1241 条），是请求失败、被中断或指向本地模型的空回包。留着会让调用次数虚高，还会在界面上塞满 `0 / 0` 的行。
- **ZCode 的库是活库（1 GB，WAL 模式），必须只读打开。** `rusqlite` 用 `SQLITE_OPEN_READ_ONLY`，不会干扰正在写它的 ZCode。
- **pi 的子代理会话不会重复计数。** 除 `<项目>/<会话>.jsonl` 外还有 `<会话>/<hash>/run-0/session.jsonl` 这类文件，实测它们的消息 id 与父文件零重叠，全扫是安全的。
- **时间按本机时区切日。** 用 UTC 的话东八区早上 8 点前的用量会被算进前一天。

### 已知限制

- **Codex 的压缩历史没算。** 0.16x 之前的 rollout 是 `.jsonl.zst`，本机 135 个文件还没纳入，需要引入 zstd 解压库。界面和 `usage status` 里都写明了这一点。
- **不算金额。** 只有 pi 自带 `cost` 字段，母版 `providers.toml` 里也只有一条模型配了单价。单价不全时算总价会误导，所以这一版只出 token。
- **Cursor 是估算，且不含输出。** 详见上面「Cursor 为什么只能估算」。
- **首次统计要付全量的代价**：pi 约 1.7 秒（595 MB / 601 个文件），Cursor 约 3.5 秒（4.2 GB 的库）。
  之后都是增量。换机器、删掉 `~/.agents/usage-cache/`、或跑 `--refresh` 会重新变成首次。
- **Cursor 的删除检测不到**：库里被删掉的行不会被增量发现，缓存会保留它们。
  怀疑数字偏大时跑一次 `--refresh` 重扫。

## 链接策略

Windows 上建符号链接需要管理员权限或开发者模式；批量建 junction 和硬链接不需要。所以默认按这个顺序降级：

```
目录（skills）   symlink → junction → 报错
单文件（AGENTS.md） symlink → hardlink → copy + 指纹
```

每一级的结果都会明确报告，不会静默降级。界面和 `status` 都能看到实际用的是哪种。

建链接前，已存在的真实文件会被移进 `~/.agents/backup/<时间戳>/`，不删除、不覆盖。如果内容和母版不同，会额外提示。

**取消映射也是走同一套边界。** `agentcenter unmap <agent>`（界面上的「取消映射」）会删掉目标位置的链接或副本：

- 符号链接 / junction：只删链接，指向的内容原地不动
- 硬链接：删掉这个目录项，母版那一份仍在
- 内容与母版一致的副本：直接删
- 内容与母版不同的真实文件：移进 `~/.agents/backup/` 后报告位置，不直接删

它同时会在 `agentcenter.toml` 里给这个 agent 记 `disabled = true`。不记的话，下一次「同步全部」会立刻把链接建回来，看起来像按钮失灵。恢复用 `agentcenter remap <agent>`。

## 几个刻意的设计决定

**API key 体检是只读的，不写入。**

`agentcenter creds inspect` 和界面上的「API Key」页只做一件事：把散在各 agent 里的 key 汇总出来，让你看清分布。它不修改任何凭证文件，也不尝试读取系统凭据库。

返回给界面的数据里**没有密钥原文**，只有 SHA-256 指纹前 8 位和末尾 4 位。原文在 Rust 内存里只停留于算指纹的那一瞬间，连序列化的机会都没有。

这个边界是有意设的：凭证是最容易出事的地方，先把「看清楚现状」做扎实，比急着做「统一写入」更有价值。

**标签不写进 skill 目录，也不做成子目录。**
写进目录会让 `npx skills update` 报冲突；做成子目录会让 ZCode 读不到（它只扫 `<skills>/<名字>/SKILL.md` 两层）。所以标签存在配置文件里，磁盘上保持平铺。

**散落 skill 的处置逐条确认，不按名字猜。**
扫描后弹出清单，每条按扫描时算出的关系给不同选项：

| 关系      | 可选动作                          |
| ------- | ----------------------------- |
| 中心库没有   | 移入中心库 / 删除                    |
| 内容相同    | 删副本改链接 / 删除                   |
| 同名不同内容  | 采用散落版 / 采用中心库版 / 删除           |
| 坏链接     | 删除坏链接                         |

「一键收集」只处理前两类 —— 冲突必须由人决定采用哪一份，工具替用户选风险太大。每个动作执行前弹确认框写清后果，执行时再重新校验一次现场（对话框开着的时候文件完全可能被改过），不信任界面上拿着的旧扫描结果。

采用散落版覆盖中心库时，中心库那一份会先复制进 `~/.agents/backup/`。**唯一不进备份的是「删除」** —— 那是用户明确要求的真删除，确认框里写明了。

**git 只给命令，绝不自动提交。**
`git commit` 逐个 `git add <路径>`，代码里没有 `git add -A`，也会拒绝仓库外的路径。界面上必须勾选路径才能提交。

**"版本"是内容哈希，不是版本号。**
`npx skills` 的 `.skill-lock.json` 里，GitHub 源记的是 git tree SHA，其它源是 SHA-256。界面展示指纹，不假装有语义版本。

**调用系统 git 而不是用库。**
你的 git 配置、钩子、凭据助手、SSH 密钥全部照常生效。

**一个 exe 同时当 GUI 和 CLI，用 windows 子系统 + 自己接回控制台。**
为了让双击启动不闪黑窗口，exe 是 windows 子系统（PE `Subsystem = 2`）—— 系统
根本不分配控制台，所以没有窗口可闪，不是「藏得快」而是「压根没有」。

CLI 分支单独处理输出：先看 `GetStdHandle(STD_OUTPUT_HANDLE)` 有没有可用句柄
（终端、重定向文件、管道都算），有就原样保留；没有再 `AttachConsole` 接父控制台
并重新绑定标准句柄。**重点是不能无条件改绑 `CONOUT$`** —— 那会让
`agentcenter status > out.txt` 和 `| more` 全部失效。详见 `src-tauri/src/console.rs`。

早先试过两条错路，都记在 `console.rs` 里：console 子系统 + `ShowWindow` 隐藏窗口
（Windows Terminal 下 `GetConsoleWindow()` 只给伪句柄，藏不掉）；console 子系统 +
`FreeConsole`（能销毁，但控制台是进程创建时由系统分配的，事后处理只能缩短闪现）。

`main.rs` 里原有一条注释说「AttachConsole 经常拿不到」，实测不准确：真正漏的是
附加后没有重新绑定标准句柄，std 缓存的仍是失效的旧值，输出被直接丢弃。

**所有子进程都必须带 `CREATE_NO_WINDOW`。**
主程序没有控制台，Windows 会给它启动的 console 子系统子进程（`git.exe`、`npx`、
`cmd`、`powershell`、`opencode`）各自新建一个控制台窗口。打开 Git 页面时
`status()` 连跑 `rev-parse`、`rev-list`、`remote get-url`、`status --porcelain`
四条命令，就会连闪四个黑窗口。

所以所有子进程一律走 `src-tauri/src/proc.rs` 的 `new` / `output` / `status`，
里面统一设 `CREATE_NO_WINDOW`。代码里不允许出现裸的 `std::process::Command::new`
—— 加了新命令请用这三个函数。

## 已知限制

- `npx skills update` 会按它自己的目录表写文件，可能把 junction 换回真实目录。更新完需要再跑一次 `agentcenter sync`。
- `skills check` 没有 JSON 输出，靠退出码判断（1 = 有更新），更新日志是彩色英文文本原样透传。
- 目前只覆盖 Windows。junction 那一级在其它平台不可用，会自动跳过到下一级。
- **Cursor 的 provider 不进 `providers sync`，改用单独一组命令**。Cursor 支持自带密钥，
  但配置在 `state.vscdb`（4 GB 的 SQLite）里、密钥经 Electron safeStorage 加密，
  而且**必须先完全退出 Cursor 才能写**（它开着时会用内存状态覆盖）。界面上在
  Provider 页有个「Cursor 自带密钥」折叠区块，命令行是
  `providers cursor status / plan / apply / reset`；写入检测到 Cursor 进程会拒绝。
  常见的 `Model name is not valid` 报错，根因通常是只填了 key、没打开
  「Use OpenAI API Key」开关。细节见 [plan/provider-config-keys.md](plan/provider-config-keys.md)。
- **从母版彻底删掉一条 MCP server 时，agent 配置里的残留不会被清理**。
  注入器只能看到「池子里有、但没分配给我」的 id。要清理请先取消分配应用一次。
- Hermes 的配置目录优先读 `HERMES_HOME` 环境变量，取不到才用 `~/.hermes`。
  本机 `~/.hermes` 是空的（只有两个备份脚本），真正在用的是
  `C:\dev\ide\hermes-agent\Hermes Agent CN Desktop` 下的桌面版。
- **Gemini CLI、Grok Build、OpenClaw、Kimi Code、MiniMax Code、GitHub Copilot CLI、
  Antigravity、Oh My Pi 八个注入器是按 cc-switch 的源码移植的，本机没装这些工具，
  尚未实测**。装好之后要对一遍真实文件。
- **Claude Desktop 没有注入器**。本机 `%LOCALAPPDATA%\Claude-3p` 不存在，
  结构虽已知但没验证，不做猜测实现。
- **opencode 的 `.jsonc` 注释会在写入时丢失**。用 `serde_json` 读写，
  带注释的文件会先剥注释再写回。要做保留注释的编辑需要引入 JSONC 文档层。
- **Codex 现在写内联 `experimental_bearer_token`**，不再生成
  `~/.agents/codex-env.ps1`。早期版本生成的这个脚本不再由工具维护。
- **只清理母版声明过的 MCP id**。Codex / Claude 里用户手写的、母版没提到的
  server 一律原样保留 —— 包括它们身上的旧写法（比如 `type = "stdio"`，
  Codex 0.167 会为此报一条「忽略未知字段」警告）。要让那类警告消失，
  把对应 server 也加进母版、再同步一次。

## 目录结构

```
agentcenter/
├── src-tauri/                    Rust 后端
│   ├── src/
│   │   ├── main.rs               入口，有子命令走 CLI，没有则开窗口
│   │   ├── lib.rs                模块声明 + Tauri 命令注册
│   │   ├── console.rs            CLI 输出通道接管（GUI 子系统下接回父控制台）
│   │   ├── proc.rs               子进程启动封装，统一加 CREATE_NO_WINDOW
│   │   ├── cli.rs                clap 子命令定义
│   │   ├── cli_run.rs            子命令执行，输出终端文本
│   │   ├── commands.rs           Tauri IPC 命令
│   │   ├── types.rs              序列化结构，与前端 types.ts 一一对应
│   │   ├── config.rs             agentcenter.toml 读写
│   │   ├── link.rs               链接引擎（降级链、健康检查、备份）
│   │   ├── doc.rs                AGENTS.md 母版的读取与写入
│   │   ├── skills.rs             skill 收集、去重、标签、单条处置
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
│   │   │   ├── dsh.rs            DSH：provider+mcp（plugin patch 数组，写 4 个 profile）
│   │   │   ├── hermes.rs         Hermes Agent：provider+mcp（读 HERMES_HOME）
│   │   │   ├── cursor.rs         Cursor：mcp
│   │   │   ├── qoder.rs          Qoder：mcp
│   │   │   ├── gemini.rs         Gemini CLI：provider+mcp（按 cc-switch 移植）
│   │   │   ├── grok.rs           Grok Build：provider+mcp（按 cc-switch 移植）
│   │   │   ├── openclaw.rs       OpenClaw：provider（按 cc-switch 移植）
│   │   │   ├── kimi.rs           Kimi Code：provider+mcp（按 cc-switch 移植）
│   │   │   ├── minimax.rs        MiniMax Code：provider+mcp（按 cc-switch 移植）
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
│   │   └── mcp/                  MCP 母版、扫描、注入调度、分配矩阵
│   │       ├── mod.rs            模块声明
│   │       ├── model.rs          McpServer / Transport / 差异与同步报告
│   │       ├── store.rs          mcp.toml 读写 + gitignore 检查
│   │       ├── scan.rs           从 17 个 agent 读现有 server
│   │       ├── diff.rs           母版 vs 现状对比
│   │       ├── matrix.rs         server 池 × agent 分配矩阵
│   │       └── inject/           一个工具一个注入器
│   │           ├── mod.rs        McpInjector trait + 同步调度
│   │           ├── shared.rs     连接字段转换、Windows cmd /c 包装与还原
│   │           ├── map_agent.rs  Claude / Cursor / Qoder（同构 mcpServers）
│   │           ├── codex.rs      Codex（不写 type，HTTP 用 http_headers）
│   │           ├── opencode.rs   opencode（三份配置合并，command 数组）
│   │           ├── pi.rs         Pi（拒 SSE、名字规则、停用保留条目）
│   │           ├── zcode.rs      ZCode（mcp.servers 两层键）
│   │           ├── workbuddy.rs  WorkBuddy（disabled 反义，不碰 connector）
│   │           ├── hermes.rs     Hermes（行级 YAML 编辑）
│   │           ├── dsh.rs        DSH（plugin patch 数组，写 4 个 profile）
│   │           ├── gemini.rs     Gemini CLI（httpUrl + 单个毫秒 timeout）
│   │           ├── grok.rs       Grok Build（headers 而非 http_headers）
│   │           ├── kimi.rs       Kimi Code（transport: sse）
│   │           ├── minimax.rs    MiniMax Code
│   │           ├── copilot.rs    GitHub Copilot CLI（local + disabledMcpServers）
│   │           ├── agy.rs        Antigravity（disabled 反义，拒 SSE）
│   │           └── omp.rs        Oh My Pi（disabledServers 拒绝名单）
│   │   └── usage/                token 用量统计（读各 agent 自己的 session）
│   │       ├── mod.rs            口径归一、按天聚合、图表数据、启用名单
│   │       └── cache.rs          增量缓存：JSONL 按字节偏移，只读新追加的部分
│   ├── capabilities/             Tauri 2 权限声明
│   ├── icons/                    应用图标
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                          前端（Svelte 5）
│   ├── main.ts                   挂载入口
│   ├── App.svelte                外壳：标题栏 + 侧栏导航 + 页面切换
│   ├── app.css                   Tailwind 主题与 CSS 变量
│   ├── pages/
│   │   ├── Mappings.svelte       AGENTS 文档映射（含取消/恢复）
│   │   ├── Skills.svelte         中心库、标签与散落 skill 处置对话框
│   │   ├── Store.svelte          npx skills 更新
│   │   ├── Providers.svelte      Provider 池（provider 管理 / 启用 provider）
│   │   ├── ClaudeCodex.svelte    Claude 引导开关 + Codex 后端开关
│   │   ├── Mcp.svelte            MCP server 池 + 分配
│   │   ├── Usage.svelte          token 用量：按需开关 + 按天 × 模型
│   │   └── Git.svelte            版本管理
│   └── lib/
│       ├── ipc.ts                前后端通信（Tauri 或假数据）
│       ├── mock.ts               浏览器预览的假后端
│       ├── types.ts              共享类型，与 types.rs 对应
│       ├── utils.ts              cn() 类名合并
│       └── components/           Button / Badge / Table / Log / PageHead / TitleBar / CursorByok
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
│   ├── verify-codex-mcp.mjs      用 codex doctor 校验 MCP 写出无未知字段
│   ├── verify-pi-schema.mjs      用 Pi 自己的 schema 校验 models.json
│   └── test-toml-comments.mjs    TOML 注释保留测试
├── plan/
│   ├── README.md                 实施计划与验收记录
│   ├── providers-management.md   provider 管理专项计划
│   ├── provider-config-keys.md   各工具 provider 配置文件与键值核实表
│   └── mcp-management.md         各工具 MCP 配置文件与键值核实表
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
