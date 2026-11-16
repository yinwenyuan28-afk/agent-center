# 各 agent 的配置管理参考（AGENTS.md / provider / MCP）与 CLI 全量清单

> 给谁：要写「让 agent 自己管理这些配置」的 skill、或要改某个注入器的人。
>
> 一句话：19 个 agent、三类配置，每类分别改哪个文件、用什么方式改、有哪些例外，以及现在 CLI 能做到哪一步。
>
> 核实方式：逐文件读 `src-tauri/src/` 源码，再用 `agentcenter agents` 与磁盘上的真实文件对照。
> 核实日期 2026-11-16。凡标「本机未安装」的 agent，行为来自照 cc-switch 移植的代码，没有在本机实测。

---

## 一、整体模型

所有管理都走同一个形状：**一份母版 → 计算出每个 agent 的目标文件内容 → 统一写入**。

| 配置 | 母版 | 谁计算 | 谁写入 |
|---|---|---|---|
| 指令文件 | `~/.agents/AGENTS.md` | `link.rs`（不生成内容，只做映射） | `link.rs` 建链接/副本 |
| provider | `~/.agents/providers.toml` | `agents/<id>.rs` 转给 `providers/<id>.rs` 的 `plan()` | `providers/safe_write.rs` |
| MCP | `~/.agents/mcp.toml` | `mcp/inject/<id>.rs` 的 `plan()` | `mcp/inject/mod.rs` + `safe_write` |

三条贯穿全部配置的规则：

1. **先算后写**。先把所有 agent 的新内容在内存里算完，任何一个算失败就整次中止，一个文件都不写。避免"改完前三个、第四个失败"留下半同步状态。
2. **只改自己声明管的键**。用户在 agent 自己界面里改过的超时、工具白名单、OAuth、模型选择一律原样保留。
3. **删也要有依据**。母版里没分配给某个 agent 的条目会从该 agent 配置里移除，但只移除母版里认得出的 id；用户手动加的、agent 内置的一概不碰。

> 母版为空时的行为不同：provider 和 MCP 的空母版一律「什么都不做」。否则清空母版会被理解成「把各 agent 的配置全删掉」。

---

## 二、19 个 agent 的能力总表

`order` 是界面排序，同 `agentcenter agents` 的输出顺序。

| order | id | 名称 | 本机已装 | 指令文件 | provider | MCP |
|---|---|---|---|---|---|---|
| 10 | `claude` | Claude Code | 是 | 有 | 有（单槽位） | 有 |
| 20 | `codex` | Codex CLI | 是 | 有 | 有（单槽位） | 有 |
| 30 | `zcode` | ZCode | 是 | 有 | 有 | 有 |
| 40 | `pi` | pi | 是 | 有 | 有 | 有 |
| 50 | `opencode` | opencode | 是 | 有 | 有 | 有 |
| 60 | `workbuddy` | WorkBuddy | 是 | **无** | 有 | 有 |
| 70 | `dsh` | DeepSeek Harness | 是 | 有 | 有 | 有 |
| 80 | `hermes` | Hermes Agent | 是 | **无** | 有 | 有 |
| 110 | `cursor` | Cursor | 是 | 有 | **单独命令** | 有 |
| 120 | `qoder` | Qoder | 是 | **无** | 未实现 | 有 |
| 130 | `qoder-cn` | Qoder CN | 否 | 有 | 未实现 | 未实现 |
| 140 | `codebuddy` | CodeBuddy | 是 | **无** | 未实现 | 未实现 |
| 150 | `trae-cn` | Trae CN | 否 | 有 | 未实现 | 未实现 |
| 170 | `zed` | Zed | 是 | 有 | 未实现 | 未实现 |
| 180 | `gemini` | Gemini CLI | 否 | 有 | 有（单槽位） | 有 |
| 190 | `grok` | Grok Build | 否 | **无** | 有（单槽位） | 有 |
| 200 | `openclaw` | OpenClaw | 否 | **无** | 有 | 未实现 |
| 210 | `kimi` | Kimi Code | 否 | **无** | 有 | 有 |
| 220 | `minimax` | MiniMax Code | 否 | **无** | 有 | 有 |

「单槽位」= 这个 agent 只有一组配置决定当前端点，同一时刻只能有一个 provider 生效。母版里勾某个 provider 上这个 agent 时，会自动把它从其它 provider 上摘掉（`providers/matrix.rs::set_row`）。

provider 可注入 = 13 个（`agents::provider_capable()`）。MCP 可注入 = 14 个（`agents::mcp_capable()`）。这两个列表都由各 agent 实现自己的 `supports_provider()` / `supports_mcp()` 决定，不再有独立的清单文件。

---

## 三、指令文件（AGENTS.md）

### 3.1 机制

母版只有一份：`~/.agents/AGENTS.md`（路径可在 `~/.agents/agentcenter.toml` 的 `[source].instructions` 改）。其它 agent 的指令文件是它的**链接或副本**。

`link.rs` 的降级链：

```
目录：symlink → junction（mklink /J，无需权限） → 报错
单文件：symlink → 硬链接（无需权限） → 副本 + 指纹
```

Windows 上建符号链接需要管理员权限或开启开发者模式，所以默认会落到 junction / 硬链接。实际用了哪一级每次都会报告，不静默降级。

三条安全边界：

- 建链接前，已存在的真实文件先移到 `~/.agents/backup/<时间戳>/`，不删除、不覆盖。内容与母版不同时会额外提示合并。
- 副本模式靠 `.agentcenter-manifest.json` 里的 SHA-256 判断是否被改过；`status` 报 `ok` / `drifted` / `broken` 三态。
- 取消映射（`unmap`）不只删链接，还写 `disabled = true`。否则下一次 `sync` 立刻建回来。

### 3.2 每个 agent 的指令文件路径

| agent | 指令文件 | 备注 |
|---|---|---|
| claude | `~/.claude/CLAUDE.md` | 不读 `AGENTS.md` |
| codex | `~/.codex/AGENTS.md` | |
| zcode | `~/.zcode/AGENTS.md` | |
| pi | `~/.pi/agent/AGENTS.md` | 在 `agent/` 子目录下 |
| opencode | `~/.config/opencode/AGENTS.md` | |
| dsh | `~/.dsh/AGENTS.md` | |
| cursor | `~/.cursor/AGENTS.md` | |
| qoder-cn | `~/.qoder-cn/AGENTS.md` | |
| trae-cn | `~/.trae-cn/AGENTS.md` | |
| zed | `%APPDATA%\Zed\AGENTS.md` | 真正按 APPDATA 存放的 GUI 应用 |
| gemini | `~/.gemini/AGENTS.md` | |
| workbuddy | 无 | 是 SOUL / IDENTITY / USER / MEMORY 一整套 |
| hermes | 无 | 用 `config.yaml` + SOUL / IDENTITY 体系 |
| qoder | 无 | 只有工作区级，没有用户级 |
| codebuddy | 无 | |
| grok | 无 | 只声明了 skills 目录 |
| openclaw | 无 | |
| kimi | 无 | |
| minimax | 无 | |

没有单一指令文件的 agent 不会出现在 `resolved_mappings()` 里，`sync` / `status` 也不管它们。

### 3.3 skills 目录

`skills.rs` 只扫已安装 agent 的 `skills_dir()`，用来找散落在各处的 skill 副本。**即使某个 agent 原生读 `~/.agents/skills`，它自己的目录也要扫** —— Cursor 和 OpenCode 都属于这种，它们各自的 `skills/` 里常还有单独装的。

| agent | skills 目录 | 原生读 `~/.agents/skills` |
|---|---|---|
| claude | `~/.claude/skills` | 否，需挂链接 |
| codex | `~/.codex/skills` | 是 |
| zcode | `~/.zcode/skills` | 否 |
| pi | `~/.pi/agent/skills` | 是 |
| opencode | `~/.config/opencode/skills` | 是 |
| cursor | `~/.cursor/skills` | 是 |
| workbuddy | `~/.workbuddy/skills` | 否 |
| dsh | `~/.dsh/skills` | 否 |
| hermes | `$HERMES_HOME/skills` | 否 |
| qoder | `~/.qoder/skills` | 未核实 |
| qoder-cn | `~/.qoder-cn/skills` | 未核实 |
| codebuddy | `~/.codebuddy/skills` | 未核实 |
| trae-cn | `~/.trae-cn/skills` | 未核实 |
| zed | `%APPDATA%\Zed\skills` | 未核实 |
| gemini | `~/.gemini/skills` | 未核实 |
| grok | `~/.grok/skills` | 未核实 |
| openclaw | `~/.openclaw/skills` | 未核实 |
| kimi | `~/.kimi-code/skills` | 未核实 |
| minimax | `~/.minimax/skills` | 未核实 |

---

## 四、provider

### 4.1 母版结构

`~/.agents/providers.toml`，一个 provider 一条记录、只存一把 key：

```toml
[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "openai-responses"          # 默认协议
api_key = "sk-..."
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["claude", "zcode", "pi"] # 注入到哪些 agent
note = ""

# 按 agent 覆盖协议（可选）
[provider.agent_kinds]
zcode = "anthropic"
```

四条关键设计：

- **协议是「provider × agent」的属性，不是 provider 的属性。** 实测同一个端点在 ZCode 里要写 `anthropic`、在 Pi 里要写 `openai-responses`。所以 `kind` 只是默认值，`agent_kinds` 按 agent 覆盖。
- **`agents` 字段就是分配关系。** 改分配不需要改任何 agent 的配置文件，改这里再 `sync` 即可。
- **模型可以只写 id，也可以带元数据。** 带元数据时整组写成内联表（TOML 不允许数组里混字符串和表）。母版留空的字段注入时不动目标里的原值。
- **文件里有明文 key。** `providers set` 保存后会检查 `~/.agents/.gitignore` 是否忽略 `providers.toml`，`providers doctor` 也是查这一项。

### 4.2 每个 provider 注入器写什么

「单槽位」的 agent 切换时会先清掉上一家的关键字段再写新的；「多槽位」的只增删自己那一条。

| agent | 文件 | 写法 | 槽位 | 支持的协议 |
|---|---|---|---|---|
| claude | `~/.claude/settings.json` | `env` 里写 `ANTHROPIC_*` 等；上游不是 Anthropic 协议时把 `ANTHROPIC_BASE_URL` 指向本地转换端口 `127.0.0.1:15722`，模型名换成稳定的 `claude-*` 别名 | 单 | anthropic / openai-chat / openai-responses |
| codex | `~/.codex/config.toml` | 一张 `[model_providers.custom]` 表，key 写 `experimental_bearer_token`；顶层 `model_provider` / `model` / `model_reasoning_effort` | 单 | 只有 openai-responses |
| zcode | `~/.zcode/v2/config.json` + `~/.zcode/v2/provider_config.json` | 两份同步写。键是 UUID，不是母版 id，所以**用 base_url 认身份**。`provider_config.json` 才是设置页「自定义供应商」真正读的 | 多 | 全部 |
| pi | `~/.pi/agent/models.json` | `providers.<id>` 带 `baseUrl` / `api` / `apiKey` / `models`。key 内联在这里，**不碰 `auth.json`**（那是 `/login` 的地盘） | 多 | 全部四种一对一映射 |
| opencode | `~/.config/opencode/opencode.jsonc` 优先，否则 `opencode.json` | 按已装版本选写法：major < 2 用 V1（`provider.<id>.options.apiKey`），否则 V2（`providers.<id>.settings.apiKey`）。key 内联，**不碰** `~/.local/share/opencode/auth.json` | 多 | 全部 |
| workbuddy | `~/.workbuddy/models.json` | 扁平**模型**数组，一个 provider 的 N 个模型就是 N 条记录，`url` 和 `apiKey` 每条都存一遍 | 多 | 全部 |
| dsh | `~/.dsh/profiles/<profile>/cordis.patch.yml` + `~/.dsh/.credentials.yaml` | 行级文本编辑（不用 YAML 库，避免丢注释）。provider 嵌在 `llm-pi-ai` 条目的 `config.providers` 下，key 走 `apiKeyEnv` + credentials 的 `refs`。`web`/`desktop` 目录不存在也会写（会建目录），`headless`/`acp` 只在已存在时写 | 多 | 全部 |
| hermes | `$HERMES_HOME/config.yaml`（默认 `~/.hermes`） | 写 `custom_providers:` 列表，行级编辑。`providers:` 字典在 v12 起归 Hermes 自己管，**只读不写** | 多 | 全部 |
| gemini | `~/.gemini/.env` + `~/.gemini/settings.json` | `.env` 每行 `KEY=VALUE`，先删 `GOOGLE_*` 等关键字段再写；`settings.json` 只改 `security.auth.selectedType` 和 `model.name` | 单 | anthropic / openai-chat / openai-responses |
| grok | `~/.grok/config.toml` | 固定表名 `[model."agentcenter-grok"]` 整表替换，`models.default` 和 `session_summary` 一起指过去 | 单 | openai-chat / openai-responses |
| openclaw | `~/.openclaw/openclaw.json` | `models.providers.<id>`，多 provider 并存。不碰 `agents.defaults.model.primary`（用户的运行时选择）。JSON5 文件用 `serde_json` 读写，**注释会丢** | 多 | 全部 |
| kimi | `~/.kimi-code/config.toml` | `[providers.<id>]` + `[models.<alias>]`，一个 provider 一个别名。不改 `default_model`，只在它指向被删条目时纠正 | 多 | 全部 |
| minimax | `~/.minimax/config.yaml` | `custom_provider.<key>` 段级替换，其余段和注释保留。`kind != "custom"` 的节点是 MCode 自己的登录账号，拒绝覆盖 | 多 | 全部 |
| cursor | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` | **不进 sync**。见 4.4 | — | — |

`providers/*/scan.rs` 里对 13 个 agent 各有一个扫描函数，只读，用来生成 `providers diff` 和 `models backfill` 的数据。

### 4.3 写入时的安全步骤

`safe_write.rs` 是唯一写用户文件的入口，一个文件写入要过这几步：

1. **解析失败即中止** —— 计划阶段就拒绝，这里复核一次。
2. **hash 复查** —— 写入前重读文件，和计划时算的 SHA-256 比。不一致说明这期间被外部改过，中止并提示重新运行。不用修改时间判断，因为它在网络盘上不可靠、时钟回拨也会骗人。
3. **首写备份** —— 每个文件在整个生命周期内只备份第一次，放在 `~/.agents/backup/providers-first-write/`，文件名是「原路径 hash 前 12 位 + 原文件名」，同目录放一个 `.source` 记录原路径。
4. **原子写入** —— 同目录临时文件 → fsync → rename。Windows 上 rename 覆盖已存在文件会失败，所以先删再改名。
5. **解析符号链接** —— 写到链接的最终目标，不把链接替换成普通文件（用 stow / chezmoi 管理 dotfiles 的人会中招）。

### 4.4 Cursor 是例外

Cursor 的自带密钥（BYOK）配置在 SQLite 库 `state.vscdb` 里，不是文本文件：

| 键 | 在哪个表 | 值 |
|---|---|---|
| `applicationUser` JSON 里的 `useOpenAIKey` | `ItemTable` | 总开关 |
| 同一个 JSON 里的 `openAIBaseUrl` | `ItemTable` | 接口地址 |
| `secret://cursorAuth/openAIKey` | `ItemTable` | 密钥，`v10` + 12 字节 nonce + AES-256-GCM 密文 |

加密链：`%APPDATA%\Cursor\Local State` 里的 `os_crypt.encrypted_key` → 去掉 `DPAPI` 前缀 → Windows DPAPI 解开 → 32 字节 AES 主密钥。主密钥按用户和机器绑定。

三条限制使它不能走 `providers sync`：

- 库是 4 GB 级二进制，不是「算出新文本再写入」的模型；
- 密钥必须先加密；
- **Cursor 运行中写库会被它的内存状态覆盖**，还可能与这个库的写锁冲突。

所以走单独一组命令 `agentcenter providers cursor status / plan / apply / remove-model / reset`，执行前要求 Cursor 完全退出（检测到在运行就返回退出码 2，除非加 `--force`）。

### 4.5 Claude 的本地协议转换

上游不是 Anthropic 协议时（`openai-chat` / `openai-responses`），Claude Code 无法直接对话。处理办法：

- `ANTHROPIC_BASE_URL` 写成 `http://127.0.0.1:15722`（`claude_bridge.rs` 的本地端口）；
- 模型名写成 `claude-opus-5` / `claude-sonnet-5` / `claude-haiku-5-5` 这类 Claude 认识的稳定别名，转换层再按档位翻成上游真实模型。

写上游真实模型名也能用，但 Claude Code 会按未知模型给它 200K 窗口、关掉思考参数。稳定别名让它按自己的模型表处理。

**注意：转换端口只在图形界面开着时启动**（`lib.rs::run_gui` 里 `claude_bridge::ensure_started()`）。走 CLI 时不会起这个端口，此时 Claude Code 连上去会失败。这是 CLI 化管理的一个硬约束。

---

## 五、MCP

### 5.1 母版结构

`~/.agents/mcp.toml`：

```toml
[[server]]
id = "context7"
name = "Context7"
transport = "stdio"          # stdio / sse / http
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
cwd = ""
url = ""
enabled = true
agents = ["claude", "codex", "pi"]
note = ""

[server.env]
API_KEY = "..."
```

- `transport` 三档对应三种真实协议：`stdio`（本地子进程）、`sse`（旧远程）、`http`（Streamable HTTP）。
- `agents` 就是分配关系。**没列出的 agent 就是「不该有这个 server」**，注入时要从那个 agent 的配置里清掉。
- 保存时校验 id 唯一、stdio 必须有 `command`、远程必须有 `url`。
- 文件里可能有 key（远程型的 `headers`、stdio 型的 `env`），所以和 `providers.toml` 一样要进 `.gitignore`。

### 5.2 每个 MCP 注入器写什么

| agent | 文件 | 键路径 | 取消分配时 | 特殊约束 |
|---|---|---|---|---|
| claude | `~/.claude.json` | 顶层 `mcpServers` | 删条目 | 同文件的 `projects.<路径>.mcpServers` 是项目级，只改顶层 |
| cursor | `~/.cursor/mcp.json` | 顶层 `mcpServers` | 删条目 | |
| qoder | `~/.qoder/settings.json` | 顶层 `mcpServers` | 删条目 | 同文件还有 `enabledPlugins` / `permissions` / `security` / `model` |
| pi | `~/.pi/agent/mcp.json` | 顶层 `mcpServers` | **只标 `enabled: false`**，条目保留 | 不支持 SSE；名字只能 `[A-Za-z0-9_-]`，`-` 和 `_` 视为同一个；顶层 `imports` / `settings` 不能丢 |
| zcode | `~/.zcode/cli/config.json` | `mcp.servers.<id>`（多两层） | 删条目 | schema 严格：多一个未知键整个 server 被丢弃。`env` 不叫 `environment`、`headers` 不叫 `http_headers`、`type` 只能是 `http`；`command` 必须是字符串 |
| codex | `~/.codex/config.toml` | `[mcp_servers.<id>]` | 删表 | **不写 `type`**（写了被当未知字段忽略）；请求头字段是 `http_headers` 不是 `headers`；stdio 和远程的专属字段不能混放 |
| opencode | `~/.config/opencode/` 下三份文件合并加载（`config.json` → `opencode.json` → `opencode.jsonc`） | `mcp.<id>` | 删条目 | 写进「已经有 `mcp` 段」的那份，否则配置会被拆到两个文件里；stdio 用 `type: "local"`、`command` 是数组、`env` 叫 `environment` |
| gemini | `~/.gemini/settings.json` | 顶层 `mcpServers` | 删条目 | 没有 `type`，靠字段名推断；HTTP 用 `httpUrl`（写 `url` 会被当 SSE）；`timeout` 只有一个且是毫秒 |
| grok | `~/.grok/config.toml` | `[mcp_servers.<id>]` | 删表 | 和 Codex 几乎一样，但请求头字段是 `headers`（不是 `http_headers`）；不写 `type` |
| hermes | `$HERMES_HOME/config.yaml` | `mcp_servers.<id>` | 删条目 | 行级编辑；`enabled` / `timeout` / `tools` / `sampling` / `roots` / `auth` 是它自己的字段，`auth: "oauth"` 丢了会从已授权降级成未认证 |
| dsh | `~/.dsh/profiles/<profile>/cordis.patch.yml` | patch 数组里 `name: "@deepseek-ai/dsh-mcp-client"` 的条目 | 删 patch 条目 | 不支持 SSE；名字 1–32 字符且只能 `[A-Za-z0-9_-]`；要写进每个**已存在**的 profile（`web` / `desktop` 目录不存在也跳过，不替用户建目录）；同一个 server 在几个 profile 里连接方式不一致时拒绝写入 |
| workbuddy | `~/.workbuddy/connectors/default/mcp.json` | 顶层 `mcpServers` | 删条目 | 启用字段是反义的 `disabled`；`type` 有多种写法；83 个内置市场连接器（`connector:` 前缀）永远不碰 |
| kimi | `~/.kimi-code/mcp.json` | 顶层 `mcpServers` | 删条目 | SSE 用 `transport: "sse"` 而不是 `type`；私有字段 `deferred` / `toolTimeoutMs` 保留 |
| minimax | `~/.minimax/mcp.json` | 顶层 `mcpServers` | 删条目 | 结构和 Claude 同构，多一个 `enabled: true` |

三条跨工具的通用规则（`mcp/inject/shared.rs`）：

1. **Windows 上 `npx` / `npm` / `yarn` / `pnpm` / `node` / `bun` / `deno` 要包一层 `cmd /c`。** 这些在 Windows 里实际是 `.cmd` 批处理，直接当 `command` 写进程起不来。但 Codex、Grok、DSH、WorkBuddy 自己写出来的不带包装，这几个跟着它们走，避免多一层外壳进程。
2. **只替换连接字段，工具自己的字段原样保留**（`timeout`、`tools`、`env_vars`、`exposure`、`runtime`、`staticEnv` 等）。
3. **只操作母版里有的 id。** 用户手动配的、agent 内置市场目录的条目，清理逻辑永远不会碰到。

### 5.3 三个「有注入器但没接入」的工具

`mcp/inject/` 下有 `copilot.rs`、`agy.rs`、`omp.rs`，但 `agents::all()` 里没有对应 agent。结果是：

- `mcp scan` / `mcp diff` 会读它们（`scan_all()` 里列了）；
- `mcp sync` **永远碰不到它们**（只遍历 `agents::mcp_capable()`）。

| 注入器 | 文件 | 现状 |
|---|---|---|
| `copilot` | `~/.copilot/mcp-config.json` + `~/.copilot/settings.json` 的 `disabledMcpServers` | 未接入。禁用时还会把 `tools` / `timeout` 存到 `~/.agents/copilot-mcp-options.json` |
| `agy`（Antigravity） | `~/.gemini/config/mcp_config.json` | 未接入。和 Gemini CLI 的 `~/.gemini/settings.json` 不是同一个文件 |
| `omp`（Oh My Pi） | `~/.omp/<profile>/agent/mcp.json` | 未接入。停用要同时写 `enabled: false` 和顶层 `disabledServers` |

要接入只需在 `agents/mod.rs::all()` 加一个 agent 实现，`supports_mcp()` 返回 true。

---

## 六、CLI 全量清单

二进制两种用法：带子命令走命令行输出文本，不带子命令（或 `ui`）开图形窗口。npm 只分发一个 exe。

### --json

全局参数，写在子命令之前（`agentcenter --json agents`）。作用于读命令：

| 命令 | 输出 |
|---|---|
| `agents` | 能力矩阵数组，字段都 camelCase |
| `status` / `mappings` | 映射数组，含健康状态与是否已取消 |
| `providers list` | provider 数组（key 只给长度和末尾 4 位） |
| `providers matrix` / `mcp matrix` | 矩阵对象（`columns` + `providers`/`rows`） |
| `providers diff` / `mcp diff` | 差异报告（`lines` / `conflicts` / 统计） |
| `mcp list` | `{masterPath, servers}` |
| `doc path` / `doc read` | 路径与内容 |
| `skills list` / `skills scan` | skill 数组 |
| `creds inspect` | 体检报告（只有指纹，无原文） |
| `usage status` / `usage report` | 状态数组 / 完整报告 |
| `bridge status` | 端口状态（含 `listening` / `ours` / `configured`） |

写命令（`set` / `add` / `sync` / `remove` / `assign` / `doc write`）仍输出人话文本，只有 `sync` 例外：它也会输出 JSON 结果对象（含 `written` / `skipped` / `log`），便于调用方确认写了哪些文件。

`skills collect --json` 只输出计划、**不执行**。要真正收集必须不带 `--json` 并加 `--apply`。

不加 `--json` 时仍是**面向人的固定宽度文本**，字段位置靠列宽对齐。解析时按空白切分会碰到模型名里的 `/`、路径里的空格。

### 6.1 退出码约定

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 失败（读配置失败、写入失败、母版里没有该条目等） |
| 2 | 「需要你处理」：`status` 有异常项、`providers doctor` 发现 `.gitignore` 没忽略母版、Cursor 命令检测到 Cursor 正在运行 |

`skills check` 会把 `npx skills` 的退出码原样传出（1 = 有更新）。`git commit` / `git push` 也是原样透传 git 的退出码。

### 6.2 指令文件映射

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `agentcenter sync` | `--dry-run` | 把母版同步到所有映射 | 是 |
| `agentcenter status` | | 检查每条映射的健康状态（`ok` / `drifted` / `broken`） | 否 |
| `agentcenter mappings` | | 列出全部映射（agent / 母版 / 目标） | 否 |
| `agentcenter unmap <agent>` | | 删链接并记下「已取消」 | 是 |
| `agentcenter remap <agent>` | | 清标记并立刻重建链接 | 是 |

`sync` 会跳过两类：`disabled = true` 的、配置目录不存在的。

母版内容本身的读写：

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `doc path` | | 母版路径、是否存在、字节数 | 否 |
| `doc read` | | 输出母版内容（原样，不加装饰） | 否 |
| `doc write` | `--file <路径>`（省略读标准输入） | 覆盖写入。内容为空会被拒绝 | 是 |
| `doc create` | `--file <路径>`（省略读标准输入） | 只在不存在时创建，已存在则拒绝 | 是 |

**写母版只改这一份文件**，各 agent 的指令文件是链接或副本，要再跑 `sync` 才同步出去。`doc write` 拒绝空内容是有意的：脚本里管道接错时会把母版清空。

### 6.3 provider

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `providers list` | | 列出母版里的 provider（base_url / kind / models / agents / note） | 否 |
| `providers diff` | | 母版 vs 各 agent 现状，含冲突项 | 否 |
| `providers sync` | `--dry-run` | 注入到各 agent | 是 |
| `providers set <id>` | `--name` `--base-url` `--api-key` `--kind` `--agent-kind <agent>=<kind>`（可重复） `--models a,b` `--agents a,b` `--codex-model <model>` | 新增或更新一条 provider。更新时只覆盖显式给出的字段 | 是（母版） |
| `providers remove <id>` | | 从母版删除一条 | 是（母版） |
| `providers matrix` | | 分配矩阵：每个 provider 用在哪些 agent 上 | 否 |
| `providers assign <id>` | `--agents a,b`（省略 = 清空） | 整行设置某个 provider 的分配 | 是（母版） |
| `providers example` | | 输出示例母版 | 否 |
| `providers doctor` | | 检查母版路径、可否解析、`.gitignore` 是否忽略 | 否 |

`providers set` 的几个细节：

- 新增时 `--name` / `--base-url` / `--api-key` 三者必填。
- `--kind` 取值：`anthropic` / `openai-chat` / `openai-responses` / `google-generative-ai`。解析是宽容的，`openai-compatible`、`gemini`、`responses` 这类旧写法都会被接受。
- `--codex-model` 必须出现在 `--models` 里，否则报错。Codex 的模型选择器只列它自己的官方目录，自定义 provider 的模型进不去，只能靠这个值写进 `config.toml` 顶层的 `model`。
- **`providers set --agents` 不做任何校验**：它直接把列表写进母版的 `agents` 字段，既不检查 agent 名是否已知，也不做单槽位互斥。写错名字（比如 `claud`）会留在母版里，`sync` 时被静默忽略。
- **`providers assign` 才做校验和互斥**：它先过滤掉 `provider_capable()` 之外的 agent，再对单槽位 agent（claude / codex / gemini / grok）把其它 provider 上的同名 agent 摘掉。要改分配优先用 `assign`。
- 新增时 `--agents` 同样不过滤。

Cursor 专项：

| 命令 | 参数 | 作用 |
|---|---|---|
| `providers cursor status` | | 开关、接口地址、密钥长度、自定义模型、Cursor 是否在运行 |
| `providers cursor plan <provider>` | `--models a,b` | 只显示要改什么 |
| `providers cursor apply <provider>` | `--models a,b` `--force` | 写入库。Cursor 在运行且没 `--force` 时返回 2 |
| `providers cursor remove-model` | `<models...>` `--force` | 只从模型列表里删，不动开关/地址/密钥 |
| `providers cursor reset` | | 关开关、清地址 |

### 6.4 MCP

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `mcp list` | | 列出母版里的 server | 否 |
| `mcp diff` | | 母版 vs 各 agent 现状 | 否 |
| `mcp sync` | `--dry-run` | 注入到各 agent。母版为空时返回 1 | 是 |
| `mcp add <id>` | `--name` `--transport` `--command` `--args a,b` `--cwd` `--url` `--env K=V`（可重复） `--header K:V`（可重复） `--disabled` `--agents a,b` `--note` `--clear-env` `--clear-headers` | 新增或更新一条。更新时只覆盖显式给出的字段 | 是（母版） |
| `mcp remove <id>` | | 从母版删除。已注入的条目要再跑 `mcp sync` 才清理 | 是（母版） |
| `mcp assign <id>` | `--agents a,b`（省略 = 清空） | 整行设置分配 | 是（母版） |
| `mcp matrix` | | 分配矩阵：每条 server 分配给哪些 agent | 否 |
| `mcp import` | | 从各 agent 现有配置导入 server 到母版（只加不覆盖） | 是（母版） |
| `mcp example` | | 输出示例母版 | 否 |

`mcp add` 的两条约定：

- `--env` / `--header` 只在**第一个**分隔符处切分，所以值里可以再包含 `=` 或 `:`（URL、Base64、header 值里的冒号都常见）。
- `--args` 允许以 `-` 开头（`npx -y pkg`），否则 clap 会把它当成自己的参数。
- 清空 env / headers 要用显式的 `--clear-env` / `--clear-headers`：命令行上「不传」和「传空」没法区分。
- `--agents` 会过滤掉 `mcp_capable()` 之外的 agent 名。

### 6.5 skills

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `skills list` | | 列出中心库 `~/.agents/skills` 里的 skill | 否 |
| `skills scan` | | 扫描各 agent 目录里散落的 skill，按 unique / identical / CONFLICT / broken 分类 | 否 |
| `skills collect` | `--apply` | 收集散落副本；不加 `--apply` 只预览。只处理 unique 和 identical，冲突不动 | 是 |
| `skills apply` | `<name>` 或 `--path <路径>；`--action <动作>`；`--dry-run` | 对**一条**散落 skill 执行选定处置。`--dry-run` 只列出每条可用哪些动作 | 是 |
| `skills tag <name>` | `--add a,b`（传空串 = 清空） | 给 skill 打标签，标签存在 `agentcenter.toml` 里 | 是 |
| `skills check` | | 调 `npx skills` 检查第三方更新，退出码 1 = 有更新 | 否 |
| `skills update [names...]` | | 调 `npx skills` 更新，不传名字则全部 | 是 |

标签为什么不写进 skill 目录：那会让 `npx skills update` 报冲突。为什么不做成子目录：ZCode 只扫 `<skills>/<名字>/SKILL.md` 两层，加一级分类就读不到。

#### skills apply 的六个动作

| 动作 | 做什么 | 适用的关系 |
|---|---|---|
| `import` | 移入中心库，原位置建链接指回来 | unique / conflict |
| `dedupe` | 删散落副本，原位置改建链接指向中心库 | identical / conflict |
| `take-orphan` | 采用散落版：备份中心库那份，把散落这份搬进去，原位置建链接 | unique / conflict |
| `take-shared` | 采用中心库版：删散落副本，原位置建链接 | identical / conflict |
| `delete` | 只删这一份，不建链接（彻底断开该 agent 的访问） | 任意 |
| `remove-link` | 删掉坏链接本身，不碰任何真实内容 | 任意 |

四条实现上的约定：

1. **每条都重新扫描现场**，不信任调用方传的路径或先前 `scan` 的结果 —— `scan` 和 `apply` 之间文件可能被动过。
2. **参数校验在碰磁盘之前**。动作名拼错会直接报「不认识的动作」，不会被「目标不存在」盖过去。
3. **关系与动作的搭配会校验**。对内容相同的副本选 `import` 会被拒绝，并列出该关系下真正可用的动作。
4. **按名字找不到唯一一条时拒绝执行**。同名 skill 散落在多个 agent 目录时，会列出全部路径要求改用 `--path`。

`take-orphan` 覆盖中心库前会先把旧版复制进 `~/.agents/backup/<时间戳>/`，中心库原有内容不会因为误操作消失。

### 6.6 凭证、Claude 引导、模型元数据

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `creds inspect` | | 体检：所有 agent 的 key 分布、指纹、重复情况 | 否 |
| `claude status` | | Claude Code 会不会显示首次引导 | 否 |
| `claude skip` | | 写 `~/.claude.json` 的 `hasCompletedOnboarding = true` | 是 |
| `claude restore` | | 删掉那个键 | 是 |
| `models backfill` | `--dry-run` | 从各 agent 现有配置读出模型元数据，补进母版里留空的字段 | 是（母版） |
| `bridge status` | | 转换端口在不在听、是不是本工具的转换层、配置指向哪里 | 否 |
| `bridge run` | | 前台运行转换端口，Ctrl+C 结束 | — |
| `bridge start` | | 后台启动一个脱离终端的转换进程 | 是（写 pid 文件） |
| `bridge stop` | | 停掉后台启动的进程 | 是 |

`creds inspect` 只读：返回给调用方的数据里没有密钥原文，只有 SHA-256 指纹前 8 位和末尾 4 位。原文在 Rust 内存里只停留算指纹那一瞬。

`models backfill` 会额外报告两件容易出错的事：母版里有但读不到对应端点的 provider；某个 agent 里**实际用的协议与母版不同**的地方（注入会按母版写回，如果实际值才对，应该改母版的 `agent_kinds`）。

### 6.7 token 用量

| 命令 | 参数 | 作用 |
|---|---|---|
| `usage status` | | 各 agent 是否支持、是否启用、数据源在哪、有多少条记录 |
| `usage report` | `--days N`（默认 7，0 = 全部历史） `--refresh` | 按 天 × agent × 模型 输出用量表 |
| `usage enable <agent>` | | 纳入统计 |
| `usage disable <agent>` | | 移出统计 |

默认一个都不统计，要先显式打开（存在 `agentcenter.toml` 的 `[usage].enabled`）。理由：这些 agent 的用量库加起来上 GB，读谁由用户决定。

支持采样的只有五家（前四家是真实计量，cursor 是估算）：

| agent | 数据源 | 口径 |
|---|---|---|
| claude | `~/.claude/projects/**/*.jsonl` | 只统计 assistant 消息，跳过 `<synthetic>` 报错消息；input 不含缓存，已折算 |
| codex | `~/.codex/sessions/**/*.jsonl` | 只读未压缩的 `rollout-*.jsonl`；`token_usage_record` 是单次请求量，可累加；input 已含缓存 |
| zcode | `~/.zcode/cli/db/db.sqlite` 的 `model_usage` 表 | 只统计 `status = 'completed'`；只读打开，不干扰正在用的库 |
| pi | `~/.pi/agent/sessions/**/*.jsonl` | 会话按项目分目录，子代理会话在更深的子目录；input 不含缓存 |
| cursor | `%APPDATA%\Cursor\User\globalStorage\state.vscdb` | **估算值**。Cursor 本地不记单次 token，用「上下文窗口快照的相邻差值」推算，只算递增段，输出恒为 0 |

### 6.8 其它

| 命令 | 参数 | 作用 | 写盘 |
|---|---|---|---|
| `agents` | | 列出 19 个 agent 的 id / 名称 / 是否安装 / provider 可注入性 / MCP 可注入性 / 单槽位 / 指令文件路径 | 否 |
| `git status` | | `~/.agents` 仓库的分支、领先落后、改动清单 | 否 |
| `git commit` | `--paths a,b` `-m <msg>` | 只 `git add` 指定路径后提交 | 是 |
| `git push` | | 推当前分支 | 是 |
| `ui` | | 打开图形界面（同不带参数） | — |

`git commit` 逐条 `git add <路径>`，代码里没有 `git add -A`，也会拒绝仓库外的路径。

### 6.9 环境变量

| 变量 | 作用 |
|---|---|
| `AGENTCENTER_HOME` | 整体换掉家目录。测试隔离用，所有路径解析都经过 `providers/store.rs::home_dir()` |
| `HERMES_HOME` | Hermes 的配置根目录 |
| `DSH_HOME` | DeepSeek Harness 的配置根目录 |
| `KIMI_CODE_HOME` | Kimi Code 的数据目录 |
| `MINIMAX_DATA_DIR` / `MAVIS_DATA_DIR` | MiniMax Code 的数据目录 |
| `OMP_HOME` | Oh My Pi 的数据目录（当前未接入） |

---

## 七、给 agent 用的现状评估

目标形态是「agent 通过 skill + cli 自己完成这些管理」。按这个目标逐项看现在能不能做到。

### 7.1 已经能纯 CLI 完成的

| 操作 | 命令序列 |
|---|---|
| 看现状 | `agents` → `providers list` / `mcp list` / `skills list` → `status` |
| 新增或改一个 provider | `providers set <id> ...` → `providers diff` → `providers sync --dry-run` → `providers sync` |
| 调整 provider 分配 | `providers assign <id> --agents a,b` → `providers sync` |
| 删一个 provider | `providers remove <id>` → `providers sync` |
| 新增或改一条 MCP server | `mcp add <id> ...` → `mcp diff` → `mcp sync --dry-run` → `mcp sync` |
| 删一条 MCP server | `mcp remove <id>` → `mcp sync` |
| 调整 MCP 分配 | `mcp assign <id> --agents a,b` → `mcp sync` |
| 从现有配置导入 MCP | `mcp import` |
| 改 AGENTS.md 母版内容 | `doc path` → `doc read` → `doc write --file x.md` → `sync --dry-run` → `sync` |
| 拉起 Claude 转换端口 | `bridge status` → `bridge start` |
| 补齐模型元数据 | `models backfill --dry-run` → `models backfill` |
| 体检 key | `creds inspect` |
| 同步指令文件 | `sync --dry-run` → `sync` |
| 收集散落 skill（批量） | `skills scan` → `skills collect` → `skills collect --apply` |
| 处置单条散落 skill | `skills apply --dry-run` → `skills apply --path <路径> --action <动作>` |
| 看用量 | `usage enable <agent>` → `usage report --days 7` |
| 提交母版改动 | `git status` → `git commit --paths ... -m "..."` |

### 7.2 缺口（2026-11-16 的实现状态）

前七项已补齐，新增的 `--json` / `mcp add|remove|assign|matrix|import` / `doc read|write|create|path` / `bridge status|run|start|stop` / 运行中进程提醒都在这一轮做完。剩余的：

1. ~~没有结构化输出~~ → **已补**。全局 `--json` 覆盖 15 条读命令；`mcp sync` / `providers sync` 也输出结果对象。
2. ~~MCP 母版没有写入口~~ → **已补**。`mcp add` / `mcp remove` / `mcp assign` / `mcp import`。
3. ~~AGENTS.md 母版内容没有 CLI 入口~~ → **已补**。`doc read` / `doc write` / `doc create` / `doc path`。
4. ~~MCP/skill 逐条处置仍只在图形界面~~ → **skill 部分已补**。`skills apply --path/--action` 对齐了 `skills::apply_action` 的六种处置，参数校验在碰磁盘之前、关系与动作的搭配也会校验。（MCP 侧本来就没有「逐条处置」这个概念，不涉及。）
5. ~~Claude 转换端口只在 GUI 里启动~~ → **已补**。`bridge start` / `stop` / `status`，且 `status` 能区分「本工具转换层在听」和「端口被别的程序占了」（靠 `GET /health`）。
6. ~~没有「agent 在运行」的检测~~ → **已补**。`providers sync` / `mcp sync` 真写前会提醒哪些 agent 正在运行。只是提醒，不阻止。
7. **`mcp diff` 和 `mcp sync` 的 agent 范围仍不一致。** 前者读 17 个（含未接入的 copilot / agy / omp），后者只写 14 个。diff 会报出 copilot 缺 server，sync 永远不会补。要修就得把这三个接入 `agents::all()`，属于「补能力」而不是「补 CLI」。
8. `agents --json` 里的路径是 `~` 形式（`providerTargets`、`instructionFile` 等）。这是有意的人可读形态；需要绝对路径时用 `doc path` 拿单个，或自己展开 `~`。

### 7.3 配套的 skill

`~/.agents/skills/agentcenter-config/`：

| 文件 | 作用 |
|---|---|
| `SKILL.md` | 操作流程与纪律：读现状 → 改母版 → diff → sync；不要直接编辑 agent 配置文件；不要在有 agent 运行时 sync |
| `scripts/check_skill_contract.py` | 校验 SKILL.md 里写的 CLI 契约是否还成立（82 项）。源码改动后跑一次，防止 skill 给出错误指引 |

`check_skill_contract.py` 检查三类：子命令是否都还在、每条 `--json` 是否输出可解析 JSON、退出码语义是否没变。它只读，不改任何配置。

用法：

```
python -I ~/.agents/skills/agentcenter-config/scripts/check_skill_contract.py
```

### 7.4 建议下一步

1. 考虑给 `providers set` 的 `--agents` 补上与 `providers assign` 相同的校验与单槽位互斥 —— 目前两者行为不一致，容易踩坑。
2. 把 copilot / agy / omp 接入 `agents::all()`（当前不打算做，`mcp diff` 和 `mcp sync` 的范围差异保留）。
