> 给谁：改某个工具的 provider 注入器之前，先查该改哪个文件、哪些键。
> 
> 一句话：每个工具的配置文件、该改的键，以及 agentcenter 现在实际写了什么。
> 
> 不管：母版 `providers.toml` 的字段、界面、同步时的备份与哈希复查。那些在 [providers-management.md](providers-management.md)。
> 
> 状态：2026-11-12。CC Switch 一列从 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的 `main` 逐文件读出（`src-tauri/src/` 下每个 `*_config.rs` 与 `live/` 目录），没有钉死 commit。「我们」一列全部按源码逐项对过；标「本机未安装」的五个工具是按 CC Switch 移植、尚未实测。

`~` 是用户主目录。下面说的「关键字段」只指四件事：请求发到哪、用什么密钥、用哪个模型、走哪种协议。其余键归用户和工具自己，切换时不要动。

CC Switch 对 Claude Code、Codex、Gemini CLI、Grok Build、Claude Desktop 是一次只留一家：先清掉上一家的关键字段，再写这一家。OpenCode、OpenClaw、Hermes、Pi、DeepSeek Harness、Kimi Code、MiniMax Code 是多家并存，只增删对应条目。

## 总表

| 工具               | 文件                                                                        | 一次几家 | 我们有没有注入器           |
| ---------------- | ------------------------------------------------------------------------- | ---- | ------------------ |
| Claude Code      | `~/.claude/settings.json`（旧安装可能是 `claude.json`）                           | 一家   | 有                  |
| Codex            | `~/.codex/config.toml`                                                    | 一家   | 有                  |
| OpenCode         | `~/.config/opencode/opencode.jsonc`，没有则 `opencode.json`                   | 多家   | 有                  |
| Pi               | `~/.pi/agent/models.json`                                                 | 多家   | 有                  |
| Hermes           | `$HERMES_HOME/config.yaml`，否则 `~/.hermes/config.yaml`                     | 多家   | 有，已挂进注入列表          |
| DeepSeek Harness | `~/.dsh/profiles/<profile>/cordis.patch.yml` + `~/.dsh/.credentials.yaml` | 多家   | 有，写 4 个 profile    |
| ZCode            | `~/.zcode/v2/config.json` + `provider_config.json`                        | 多家   | 有；CC Switch 没有这个工具 |
| WorkBuddy        | `~/.workbuddy/models.json`                                                | 多家   | 有；CC Switch 没有这个工具 |
| Gemini CLI       | `~/.gemini/.env` + `~/.gemini/settings.json`                              | 一家   | 有，本机未安装            |
| Grok Build       | `~/.grok/config.toml`                                                     | 一家   | 有，本机未安装            |
| OpenClaw         | `~/.openclaw/openclaw.json`                                               | 多家   | 有，本机未安装            |
| Kimi Code        | `~/.kimi-code/config.toml`                                                | 多家   | 有，本机未安装            |
| MiniMax Code     | `~/.minimax/config.yaml`                                                  | 多家   | 有，本机未安装            |
| Claude Desktop   | `<Claude-3p>/configLibrary/<profile id>.json`                             | 一家   | 没有；本机未安装，未做        |

我们的注入器列表在 `src-tauri/src/providers/mod.rs` 的 `all_injectors()`，共 13 个；agent 侧的挂载在 `src-tauri/src/agents/mod.rs` 的 `all()`。

## Claude Code

文件是 `~/.claude/settings.json`。一次只能有一家。

CC Switch 把 `settings.json` 的键分成三类：关键字段、供应商独有字段、用户键。切换时**先清空全部关键字段**，再写目标供应商带来的值；清的是键，不是整份文件。

### 关键字段

`env` 里按前缀算关键字段的：

| 前缀               | 盖住什么             |
| ---------------- | ---------------- |
| `ANTHROPIC_`     | 地址、密钥、各档模型名、自定义头 |
| `AWS_`           | Bedrock 的区域和凭据   |
| `VERTEX_REGION_` | Vertex 按模型分的区域   |

另外点名的 `env` 关键字段：

| 键                                   |
| ----------------------------------- |
| `CLAUDE_CODE_SUBAGENT_MODEL`        |
| `CLAUDE_CODE_SUBAGENT_MODEL_FORCE`  |
| `CLOUD_ML_REGION`                   |
| `GOOGLE_APPLICATION_CREDENTIALS`    |
| `CLAUDE_CODE_OAUTH_TOKEN`           |
| `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`   |
| `CLAUDE_CODE_OAUTH_SCOPES`          |
| `CLAUDE_CODE_API_KEY_HELPER_TTL_MS` |

协议选择器（7 个，必须逐个点名）：

`CLAUDE_CODE_USE_BEDROCK`、`CLAUDE_CODE_USE_VERTEX`、`CLAUDE_CODE_USE_FOUNDRY`、`CLAUDE_CODE_USE_GATEWAY`、`CLAUDE_CODE_USE_MANTLE`、`CLAUDE_CODE_USE_ANTHROPIC_AWS`、`CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD`

`CLAUDE_CODE_USE_` **不能按前缀全收**：同一前缀下还有 `CLAUDE_CODE_USE_POWERSHELL_TOOL`、`USE_NATIVE_FILE_SEARCH`、`USE_COWORK_PLUGINS`、`USE_CCR_V2` 这类功能开关，归用户。

`CLAUDE_CODE_SKIP_*_AUTH` 按「前缀 `CLAUDE_CODE_SKIP_` 且结尾 `_AUTH`」匹配。

顶层关键字段（13 个）：

`apiKeyHelper`、`apiBaseUrl`、`primaryModel`、`smallFastModel`、`apiKey`、`model`、`fallbackModel`、`modelOverrides`、`modelPicker`、`advisorModel`、`awsAuthRefresh`、`awsCredentialExport`、`gcpAuthRefresh`

`permissions`、`hooks`、`statusLine`、`enabledPlugins` 不动。`env` 本身不算关键字段。

### 供应商独有字段

这些在 `env` 里，切入时写入，切走时只删「上一家带进来、值没被改过」的：

| 键                                                      | 作用               |
| ------------------------------------------------------ | ---------------- |
| `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`               | 上游不接实验性 beta 头   |
| `CLAUDE_CODE_DISABLE_ARTIFACT`                         | 上游不接 Artifact 工具 |
| `ENABLE_TOOL_SEARCH`                                   | 第三方地址默认关         |
| `CLAUDE_CODE_DISABLE_THINKING`                         | 网关兼容             |
| `DISABLE_INTERLEAVED_THINKING`                         | 网关兼容             |
| `CLAUDE_CODE_ALWAYS_ENABLE_EFFORT`                     | 网关兼容             |
| `CLAUDE_CODE_EXTRA_BODY`                               | 网关兼容             |
| `CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING`       | 网关兼容             |
| `CLAUDE_CODE_AUTO_MODE_SERVER`                         | 网关要设 `0`，否则会卡住   |
| `CLAUDE_CODE_MAX_CONTEXT_TOKENS`                       | 窗口               |
| `CLAUDE_CODE_AUTO_COMPACT_WINDOW`                      | 窗口               |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS`                        | 窗口               |
| `CLAUDE_CODE_DISABLE_1M_CONTEXT`                       | 窗口               |
| `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT` | 窗口               |
| `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY`           | 向网关拉模型列表         |

### 模型档位

`ANTHROPIC_DEFAULT_{OPUS,FABLE,SONNET,HAIKU}_MODEL`，各配 `_NAME` 和 `_DESCRIPTION`。默认模型是 `ANTHROPIC_MODEL`。小模型回退用 `ANTHROPIC_SMALL_FAST_MODEL`。

密钥统一写 `ANTHROPIC_AUTH_TOKEN`，同时拿掉 `ANTHROPIC_API_KEY`，避免两个都在时 Claude Code 报警（cc-switch #4919）。

### 我们怎么填

| 键                                          | 我们怎么填                                          |
| ------------------------------------------ | ---------------------------------------------- |
| `ANTHROPIC_BASE_URL`                       | 直连：母版 `base_url`。需要转换：`http://127.0.0.1:15722` |
| `ANTHROPIC_AUTH_TOKEN`                     | 母版密钥                                           |
| `ANTHROPIC_API_KEY`                        | 清掉                                             |
| `ANTHROPIC_MODEL`                          | `claude_model_map.default`，缺了用第 1 个模型          |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` 和 `_NAME`   | 映射的 opus，缺了用第 1 个模型                            |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` 和 `_NAME` | 映射的 sonnet，缺了用第 2 个                            |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` 和 `_NAME`  | 映射的 haiku，缺了用第 3 个                             |

没有 Fable 档。文件不存在时整次跳过，不会新建 `settings.json`。

### 本地协议转换

**只看协议，不看模型名。** 协议不是 Anthropic 时，`ANTHROPIC_BASE_URL` 改成 `http://127.0.0.1:15722`，窗口开着时把 `/v1/messages` 翻成上游的 Chat Completions 或 Responses，再把回复翻回来。

改成只看协议是这一轮的重要修正。早期实现还会把「模型 id 不是 `claude-` 开头」当成需要转换，但那是错的：MiniMax 的 `api.minimax.cn/anthropic`、CommandCode 的 `/provider/v1` 都是 Anthropic 兼容端点，模型名是厂商自己的，直连 `/v1/messages` 完全正常；按模型名判断会把它们错误地翻成 `/chat/completions`，直接 404。

### 代理模式的模型别名

代理模式下写稳定的 `claude-*` 别名，不写上游真实模型名：

| 档位        | 写入的别名              |
| --------- | ------------------ |
| Opus / 默认 | `claude-opus-5`    |
| Sonnet    | `claude-sonnet-5`  |
| Haiku     | `claude-haiku-5-5` |

原因：Claude Code 按别名查自己的模型表决定窗口和思考参数。直接写 `deepseek/deepseek-v4.1-flash` 会被当成未知模型，按 200K 窗口、`thinking: disabled` 处理，强思考的上游直接拒。真实模型由转换层按档位映射。

`_MODEL_NAME` 仍写上游真实模型的展示名，`/model` 菜单里能看出是哪家的哪个模型。

窗口开着时转换生效；窗口关掉后转换停止。上游地址写在 `~/.agents/claude-bridge.json`，密钥仍然只放在 `ANTHROPIC_AUTH_TOKEN`。

### 取消分配

只有当当前 `ANTHROPIC_BASE_URL` 能对上母版里某个「没分给 claude」的端点，**或者等于本地转换地址**时，才清掉关键字段。对不上的（订阅登录、用户自己写的）不动。

本地转换地址也要认：否则走转换时 `ANTHROPIC_BASE_URL` 是 `127.0.0.1`，对不上任何上游端点，取消分配后代理地址会一直留着。

### 跳过首次引导（`~/.claude.json`）

Claude Code 第一次启动会走一遍引导：选主题、登录 Anthropic 账号。第三方中转的 key 已经在 `ANTHROPIC_AUTH_TOKEN` 里，那一步登录是多余的。

跳过方式是在 **`~/.claude.json` 的根对象**写 `hasCompletedOnboarding: true`；关掉是**删掉这个键**（不是写 `false`，Claude Code 只认 `true`）。

注意文件分工，写错不会报错、只是没效果：

| 文件                        | 装什么                            |
| ------------------------- | ------------------------------ |
| `~/.claude/settings.json` | provider 配置（`env`）、权限、hooks    |
| `~/.claude.json`          | 客户端状态：登录态、引导进度、MCP server、项目历史 |

这个标记**和 provider 无关**，所以它不进 `Provider` 结构，也不参与 `providers sync`。改它用：

- 界面：侧栏的 Claude & Codex 页（勾选框）
- CLI：`agentcenter claude skip` / `claude restore` / `claude status`

`~/.claude.json` 里有几十个键（`oauthAccount`、`projects`、`mcpServers`、`tipsHistory`…），只增删这一个键，其余一个字节都不碰。

> 查找：我们 `src-tauri/src/providers/claude_onboarding.rs`。CC Switch 的对照是 `src-tauri/src/claude_mcp.rs` 的 `set_has_completed_onboarding` / `clear_has_completed_onboarding`，开关来自 `settings.rs` 的 `skip_claude_onboarding`。

> 查找：我们 `src-tauri/src/providers/claude.rs`（`FLOOR_ENV_PREFIXES`、`PROTOCOL_SELECTORS`、`FLOOR_ENV_KEYS`、`FLOOR_TOP`、`EXCLUSIVE_ENV`）和 `claude_bridge.rs`。CC Switch 的对照是 `src-tauri/src/live/floor.rs`。

## Codex

文件是 `~/.codex/config.toml`。`auth.json` 只留给官方登录。

### 一次只有一家

第三方统一写成一张表 `[model_providers.custom]`，顶层 `model_provider = "custom"`。密钥写在表的 `experimental_bearer_token` 里 —— Codex 0.149 起自定义 provider 不再读 `auth.json` 里的密钥。

`requires_openai_auth` 跟着「盘上有没有登录」走：有登录写 `true`，Codex 才显示账号信息、刷新 token；没登录写 `true` 会让 Codex 卡在登录页。

表名 `openai`、`ollama`、`lmstudio` 会让 Codex 0.148 起拒绝加载，不要用这三个当自定义 provider 的 id。

### 顶层关键字段

`model_provider`、`openai_base_url`、`model`、`review_model`、`model_reasoning_effort`、`plan_mode_reasoning_effort`、`disable_response_storage`、`model_catalog_json`、`experimental_bearer_token`、`base_url`、`wire_api`

嵌在别的表里、只清这几个键：`agents.default_subagent_model`、`agents.default_subagent_reasoning_effort`、`memories.extract_model`、`memories.consolidation_model`

供应商独有、切走时要收回的顶层键：`web_search`、`model_context_window`、`model_auto_compact_token_limit`、`model_supports_reasoning_summaries`、`model_verbosity`

### 我们怎么填

```toml
model_provider = "custom"
model = "<codex_model，没填就用第一个模型的 id>"
model_reasoning_effort = "<母版填了、且落在 Codex 词表里才写>"

[model_providers.custom]
name = "显示名"
base_url = "https://..."
wire_api = "responses"
experimental_bearer_token = "sk-..."
requires_openai_auth = false
supports_websockets = false        # 母版填了才写，不填删键
model_catalog_url = "https://.../models"  # 母版填了才写，不填删键
```

- `wire_api` 无论母版协议是什么都写 `responses` —— Codex 只支持这一种
- 早期实现按母版 id 建表、用 `env_key` + `~/.agents/codex-env.ps1`。这一轮改掉了：单槽位 + 内联 token，不再生成脚本
- 旧形态留下的表（表名是母版里出现过的 id、且不是 `custom`）会被清掉，否则 `env_key` 会和新的 token 并存
- 已有 `custom` 表时只改本工具声明管的键，`http_headers`、`query_params` 保留
- 取消 Codex 分配时，会清掉 `custom` 表里本工具写入的字段（name/base_url/wire_api/key/requires_openai_auth/supports_websockets/model_catalog_url）；清空后整表删除，`model_providers` 也空了就一起删。用户自己加的键会让表保留下来

### 关掉 WebSocket 重连、补模型目录

两个键都只在母版明确填了才写；不填就把文件里对应的键删掉，恢复 Codex 默认。

**`supports_websockets`**：Codex 拿到流式响应后会尝试把连接升级成 WebSocket。
端点不支持时（大量第三方中转都这样），它会失败、重连、再失败，日志里刷一串
无效重连，表现是回复迟迟不出来。写 `false` 后只走 HTTP，问题消失。

**`model_catalog_url`**：Codex 默认只从官方地址拉模型目录，自定义端点拉不到时
模型列表就是空的。补一个能返回 OpenAI 模型列表 JSON 的地址即可，通常是
`<base_url>/models`；指向 ChatGPT 后端时是
`https://chatgpt.com/backend-api/codex/models`。

注意：这个字段在 `[model_providers.openai]` 这类官方表上有域名白名单
（只能用 ChatGPT 主机），在 `custom` 这类自定义表上没有限制。

母版字段是三态：

| 母版值 | 写入 | 含义 |
|---|---|---|
| 不写（`None`） | 删除该键 | 回到 Codex 默认 |
| `codex_supports_websockets = false` / `true` | 写对应布尔值 | 关 / 开 WebSocket |
| `codex_model_catalog_url = "https://..."` | 写这个地址 | 用这个目录 |

界面上在「启用 provider」的 Codex 卡片里，按钮叫「连接设置」，两个键可以分别设。
「Claude & Codex」页的 **「修复连接」** 开关则把两件事合成一步：开启时一次写
`supports_websockets = false` 和 `model_catalog_url`（目录沿用已有值，没有就按
`<base_url>/models` 推导），关闭时两个键一起删掉。它**直接写 config.toml**：
目标表由顶层 `model_provider` 决定（不是写死 `custom`），所以不需要先在
母版里分配 provider，也不依赖本工具的 provider 管理。页面显示的值同样读自
config.toml，用户手动改过也能对上。
命令行：

```bash
agentcenter providers set <id> --codex-websockets false
agentcenter providers set <id> --codex-model-catalog-url https://example.com/v1/models
# 传空串清掉目录地址
agentcenter providers set <id> --codex-model-catalog-url ""
```


### 用哪个模型：只能靠顶层 `model`，而且不能靠 Codex 的选择器

Codex 的模型选择器（TUI 里的 `/model`）只列它自己的官方目录。
`codex debug models` 能把那份目录导出成 JSON，里面只有 12 个 `gpt-*` 条目 ——
自定义 provider 的模型一个都没有：

```
$ codex debug models | (数 "slug":)  → 12
$ codex debug models | (数 deepseek)  → 0
```

原因是 provider 表里**没有「模型列表」这个字段**。`ModelProviderToml` 的全部字段是
`base_url` / `model_catalog_url` / `env_key` / `env_key_instructions` /
`experimental_bearer_token` / `gateway_oauth` / `aws` / `project_id` / `location` /
`credential_file` / `wire_api` / `capabilities` / `query_params` / `http_headers` /
`env_http_headers` / `request_max_retries` / `stream_max_retries` / ... ——
没有一个是模型清单。唯一相关的 `model_catalog_url` 还被限制成只能用官方域名：

```
model_providers.openai.model_catalog_url must use an allowed ChatGPT host.
If you need to use a 3rd party host, configure a custom model_provider
```

（`features.api_key_model_discovery` 这个开关实测也不影响目录内容，加上它仍是 12 个。）

所以「用哪个模型」只能由 `config.toml` 顶层的 `model` 决定。好在 Codex 不校验
这个名字，会原样发给上游，找不到元数据只打一条 warning：

```
warning: Model metadata for `deepseek/deepseek-v4.1-flash` not found.
Defaulting to fallback metadata; this can degrade performance and cause issues.
```

母版里用 `codex_model` 指定，不填就回退到 `models` 的第一个（存量行为不变）。
界面上在「启用 provider」的 Codex 卡片里，叫「配置模型」。

### `model_reasoning_effort` 的词表必须收敛

这个值会被原样放进请求体的 `reasoning.effort`，上游是**封闭枚举**：

```
none | minimal | low | medium | high | xhigh | max
```

母版的档位词表跟它不一致。实测 CommandCode 的 `xiaomi/mimo-v2.5` 在母版里写的是
`enabled` / `off`，原样写进 `config.toml` 后每次请求都被拒：

```
403 {"error":{"message":"Invalid option: expected one of
\"none\"|\"minimal\"|\"low\"|\"medium\"|\"high\"|\"xhigh\"|\"max\"",
"param":"reasoning.effort"}}
```

所以注入器只写落在白名单里的值。不落白名单时：

- **不写**这个键（Codex 会用默认档位）
- 文件里残留的不合法值**一并删掉** —— 留着会让每次请求都失败

单测 `codex_effort_whitelist_matches_upstream_enum` 固化这条边界。

### `requires_openai_auth` 是「绕过 ChatGPT 后端」的开关

Codex 没有界面开关，路由到 ChatGPT 后端的行为**完全**由这个字段决定。`codex doctor` 也把它当 provider 属性报告，不是账号属性。

母版里用 `codex_requires_openai_auth` 控制它，三态：

| 母版值        | 写入的 `requires_openai_auth`           | 含义                                 |
| ---------- | ------------------------------------ | ---------------------------------- |
| 不写（`None`） | 看 `~/.codex/auth.json` 里有没有 `tokens` | 自动：有登录 `true`，没登录 `false`          |
| `false`    | `false`                              | 强制绕过 ChatGPT 后端，只用表里的 bearer token |
| `true`     | `true`                               | 强制走 ChatGPT 后端，带着官方登录请求            |

为什么需要手动入口:自动判断在两种常见场景下会选错——装了 Codex 但想强制用第三方 key，或登录态还在但临时切第三方。配第三方端点时应选 `false`；选 `true` 会把官方登录凭据发给对方地址。

写 `true` 而盘上没有登录时，Codex 会卡在登录页。所以自动模式的默认值跟着登录态走。

界面上这个开关在侧栏的 **Claude & Codex** 页里。它直接读写 `config.toml`
当前生效的表（表名取顶层 `model_provider`，不是写死 `custom`），所以显示值
和文件一致；「自动」表示删掉这个键，交给 Codex 判断。Provider 页的「后端开关」
弹窗仍然走母版 + 注入器，是「provider 级设置」，两者不要混用。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `CODEX_FLOOR_TOP`，`src-tauri/src/live/project/codex.rs` 的 `ROUTE_ID = "custom"` 与 `requires_openai_auth()`。我们 `src-tauri/src/providers/codex.rs`。

## OpenCode

CC Switch 先找 `~/.config/opencode/opencode.jsonc`，没有再用 `opencode.json`，两个都没有就写 `opencode.json`。多家并存。

### 两套 schema

| 套      | 顶层键         | 条目里的键                                                         |
| ------ | ----------- | ------------------------------------------------------------- |
| V1（旧）  | `provider`  | `npm`、`options`（含 `apiKey`）                                   |
| V2（原生） | `providers` | `package`、`settings`（含 `apiKey`）、`headers`、`body`、`canonical` |

新的 V1 条目要有 `npm` 和至少一个模型。

### 密钥内联

`provider.<id>.options.apiKey`（V1）或 `providers.<id>.settings.apiKey`（V2）。**不写 `~/.local/share/opencode/auth.json`** —— 那个文件是 opencode 自己的登录凭据，cc-switch 也不碰。

### 我们怎么填

- 按本机已装主版本选套：主版本号小于 2 或探测失败用 V1，否则 V2
- `.jsonc` 存在就写 `.jsonc`
- 模型是 `models.<id>.name`
- V1 的 npm 包：Anthropic 用 `@ai-sdk/anthropic`，OpenAI Responses 用 `@ai-sdk/openai`，其余用 `@ai-sdk/openai-compatible`
- V2 的 package：Anthropic 用 `@opencode/ai/providers/anthropic`，其余用 `@opencode/ai/providers/openai-compatible`
- `.jsonc` 的注释会在写入时丢掉（用 `serde_json` 读写）。要做保留注释的编辑需要引入 JSONC 文档层，当前没做

> 查找：CC Switch `src-tauri/src/opencode_config.rs` 的 `resolve_config_path`、`LEGACY_ONLY_KEYS`、`NATIVE_ONLY_KEYS`。我们 `src-tauri/src/providers/opencode.rs`。

## Pi

目录是 `~/.pi/agent/`（`PI_CODING_AGENT_DIR` 非空时改用它）。

CC Switch 只改 `models.json` 里的 `providers.<id>`。`settings.json` 的当前 provider 和模型归 Pi。`auth.json` 是 Pi 的 `/login`，CC Switch 不读也不写。

### 我们怎么填

只写 `models.json` 一个文件：

```jsonc
{
  "providers": {
    "<id>": {
      "name": "显示名",
      "baseUrl": "https://...",
      "api": "openai-completions",
      "apiKey": "sk-...",
      "models": [
        { "id": "...", "name": "...", "input": ["text"], "contextWindow": 1000000, "maxTokens": 128000 }
      ]
    }
  }
}
```

`api` 的值：`anthropic-messages`、`openai-completions`、`openai-responses`、`google-generative-ai`。母版填了才写 `reasoning`、`thinkingLevelMap`、`cost`。

早期实现把 key 写进 `auth.json` 的 `<id>.key`。这一轮改掉了：`auth.json` 归 Pi 的 `/login`，两处都写会让 Pi 不知道用哪个，也会覆盖用户登录的账号。

`apiKey` 支持 `$ENV` / `${ENV}` / `!command` 表达式，但本工具只写明文。

> 查找：CC Switch `src-tauri/src/pi_config/mod.rs` 文首说明。我们 `src-tauri/src/providers/pi.rs`。

## Hermes

文件是 `$HERMES_HOME/config.yaml`，否则 `~/.hermes/config.yaml`（CC Switch 在 Windows 上默认 `%LOCALAPPDATA%\hermes`，本机用 `HERMES_HOME` 显式指定，两种默认值都碰不到）。

### v12 起 `providers:` 只读

CC Switch 写的是顶层 **`custom_providers:` 列表**，每项用 `name` 匹配：

```yaml
custom_providers:
  - name: command
    base_url: https://api.commandcode.ai/provider/v1
    api_key: sk-...
    api_mode: chat_completions
    model: deepseek/deepseek-v4.1-flash
    models:
      deepseek/deepseek-v4.1-flash:
        context_length: 1000000
        supports_tools: true
```

v12 的 `providers:` **字典**归 Hermes 自己管（Web UI 里配的账号），CC Switch 读取时标成只读，写入直接拒绝并提示去 Web UI 改。

早期实现写的正是 `providers:` 字典 —— 在 v12 上会被 Hermes 覆盖，也会破坏用户 Web UI 里的配置。这一轮改成了 `custom_providers:` 列表。

### `api_mode` 四档

`chat_completions`、`anthropic_messages`、`codex_responses`、`bedrock_converse`。

| 本工具协议                  | 写入的 `api_mode`       |
| ---------------------- | -------------------- |
| `anthropic`            | `anthropic_messages` |
| `openai-chat`          | `chat_completions`   |
| `openai-responses`     | `codex_responses`    |
| `google-generative-ai` | `chat_completions`   |

早期实现把 Anthropic 也写成 `chat_completions`，会让 Hermes 用 OpenAI 格式请求 Anthropic 端点。

### 我们怎么填

- `model:` 段是用户当前在用的选择，**不主动改**；只在 `model.provider` 指向一个已被移除的条目时才纠正
- 模型元数据写 `models.<id>.context_length`、`max_tokens`、`supports_tools`
- 行级编辑，其他段和注释一个字节都不动

> 查找：CC Switch `src-tauri/src/hermes_config.rs` 的 `PROVIDER_SOURCE_CUSTOM_LIST` 和 `PROVIDER_SOURCE_DICT`。我们 `src-tauri/src/providers/hermes.rs`。

## DeepSeek Harness

家目录 `$DSH_HOME`，否则 `~/.dsh`。各 profile 共用一份 `~/.dsh/.credentials.yaml`，密钥在 `refs` 下。

### 写哪几个 profile

CC Switch 写四个：`web`、`desktop`、`headless`、`acp`。其中 `web` / `desktop` 总是写，`headless` / `acp` 只在目录已存在时写。`sdk` / `sdk-minimal` 属嵌入用途，不写。

早期实现只写 `desktop`。这一轮改成按上面的规则写多个。

### provider 结构

补丁里的 provider 挂在顶层 `- id: llm-pi-ai` 条目的 `config.providers` 下：

```yaml
- id: llm-pi-ai
  name: "@deepseek-ai/dsh-llm-pi-ai"
  config:
    providers:
      command:
        displayName: CommandCode
        apiKeyEnv: COMMAND_API_KEY
        api: openai-responses
        baseURL: https://...
        models:
          - id: deepseek/deepseek-v4.1-flash
            name: DeepSeek V4.1 Flash
            contextWindow: 1000000
            maxTokens: 128000
            input: [text]
            reasoningEfforts: { off: off, high: high }
```

`api` 三选一：`openai-completions`、`openai-responses`、`anthropic-messages`。

密钥不写进补丁，写进 `.credentials.yaml` 的 `refs.<ID>_API_KEY`，补丁里只留 `apiKeyEnv` 引用它。

> 查找：CC Switch `src-tauri/src/dsh_config.rs` 的 `TARGET_PROFILES`、`credentials_path`。我们 `src-tauri/src/providers/dsh.rs`。

## ZCode

CC Switch 没有这个工具。下面全是我们的注入器（本机实测）。

两个文件都写。只写 `config.json` 时，设置页的自定义供应商列表不出条目。身份用 `base_url` 对，ZCode 的键是 UUID，对不上母版 id。

| 文件                                 | 写什么                                                                                                                                                         |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `~/.zcode/v2/config.json`          | `provider.<uuid>.name`、`kind`（`anthropic` 或 `openai-compatible`）、`options.apiKey`、`options.baseURL`、`options.apiKeyRequired`、`source: custom`、`models.<id>` |
| `~/.zcode/v2/provider_config.json` | 供应商规则：`config.access.apiKey`、`config.api.type`、`config.api.baseUrl`、`personalModelIds`、`modelOrder`；模型规则：`contextWindow`                                    |

`config.json` 里模型还写 `limit.context`、`limit.output`、`modalities.input`、`modalities.output`，有推理时写 `reasoning.enabled`、`variants`、`defaultVariant`。

`provider_config.json` 的 `api.type`：`anthropic-messages`、`openai-responses`、`openai-chat-completions`。Google 这一类也写成 `openai-chat-completions`。

> 查找：`src-tauri/src/providers/zcode.rs` 的 `to_zcode_provider` 和 `to_personal_provider_rule`。

## WorkBuddy

CC Switch 没有这个工具。下面是我们的注入器（本机实测）。

文件是 `~/.workbuddy/models.json`，一个扁平数组。一个模型一条，同一家的 `url` 和 `apiKey` 在每条里重复一遍。按模型 `id` 匹配；已有条目只改 `url` 和 `apiKey`。取消分配时按 `url` 删掉能对上母版端点的条目。

新条目的键：`id`、`name`、`vendor: Custom`、`url`、`apiKey`、`supportsToolCall`、`supportsImages`、`supportsReasoning`、`useCustomProtocol`、`maxInputTokens`。母版填了推理档位才写 `reasoning.supportedEfforts`。`useCustomProtocol` 只有 OpenAI Responses 为 true。

> 查找：`src-tauri/src/providers/workbuddy.rs` 的 `make_entry`。

## Gemini CLI

本机未安装。下面是 CC Switch 的实现，我们已按它移植。

两个文件，目录都是 `~/.gemini/`：

| 文件              | 改哪些键                                           |
| --------------- | ---------------------------------------------- |
| `.env`          | 先删掉算关键字段的行，再写目标供应商的。注释和其余行不动                   |
| `settings.json` | 只改 `security.auth.selectedType` 和 `model.name` |

`.env` 里算关键字段的：所有 `GOOGLE_*`（包括 `GOOGLE_GEMINI_BASE_URL`、`GOOGLE_API_KEY`、`GOOGLE_GENAI_USE_VERTEXAI`、`GOOGLE_CLOUD_PROJECT`），加上点名的 `GEMINI_API_KEY`、`GEMINI_MODEL`、`GEMINI_API_KEY_AUTH_MECHANISM`、`GEMINI_CLI_CUSTOM_HEADERS`、`GEMINI_DEFAULT_AUTH_TYPE`、`GEMINI_CLI_USE_COMPUTE_ADC`、`CODE_ASSIST_ENDPOINT`、`CODE_ASSIST_API_VERSION`。

`GEMINI_SANDBOX`、`GEMINI_CLI_HOME`、`GEMINI_SYSTEM_MD` 不算，留给用户。

`selectedType`：官方 Google 登录写 `oauth-personal`，第三方写 `gemini-api-key`。这个值是**只读派生值** —— 编辑器里改了不存，由供应商类型推导。

`.env` 的编辑规则：跳过空行和 `#`；剥掉 `export `；键只允许字母数字下划线；值只 trim 不去引号；同名变量保留第一处原位改写；行尾风格和结尾换行沿用原文件。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `gemini_floor_env`，`src-tauri/src/gemini_config.rs`，`src-tauri/src/live/project/gemini.rs`。我们 `src-tauri/src/providers/gemini.rs`。

## Grok Build

本机未安装。下面是 CC Switch 的实现，我们已按它移植。

文件是 `~/.grok/config.toml`。写两处：`models.default` 指向表名，以及 `[model."<表名>"]` 整张表。

```toml
[models]
default = "agentcenter-grok"
session_summary = "agentcenter-grok"

[model."agentcenter-grok"]
model = "x-ai/grok-4.7"
base_url = "https://..."
name = "显示名"
api_key = "sk-..."
api_backend = "responses"
context_window = 500000
```

表里要有的键：`model`、`base_url`、`name`、`api_key` 或 `env_key`、`api_backend`、`context_window`（正整数）。`api_backend` 后端只校验非空，已知取值 `responses` / `chat_completions`。

三个特殊行为：

1. **整表替换**，不按字段合并 —— 避免残留上一家的 `env_key`
2. **按上次写入记录删旧表**，不按当前 `models.default` 找 —— Grok 自己的 `/settings` 会把它改成内置模型
3. `models.session_summary` 要跟着指向新表，否则 Grok 会用内置 `grok-4.6` 打第三方上游

官方卡不写模型表，Grok 回到自带的 xAI 登录。

> 查找：CC Switch `src-tauri/src/live/project/grok.rs`，`src-tauri/src/grok_config.rs` 的 `GrokModelConfig`。我们 `src-tauri/src/providers/grok.rs`。

## OpenClaw

本机未安装。下面是 CC Switch 的实现，我们已按它移植。

文件是 `~/.openclaw/openclaw.json`（JSON5）。供应商在 `models.providers.<id>`。条目键（JSON 里是驼峰）：`baseUrl`、`apiKey`、`api`、`models`、`headers`。模型条目有 `id`、`name`、`alias`、`cost.input`、`cost.output`、`contextWindow`。

当前模型在 `agents.defaults.model.primary`，备用在 `fallbacks`。CC Switch 的供应商写入**不碰**这两处，本工具也不碰。

`api` 允许值：`openai-completions`、`openai-responses`、`anthropic-messages`、`google-generative-ai`、`bedrock-converse-stream`。

> 查找：CC Switch `src-tauri/src/openclaw_config.rs` 的 `OpenClawProviderConfig`。我们 `src-tauri/src/providers/openclaw.rs`。

## Kimi Code

本机未安装。下面是 CC Switch 的实现，我们已按它移植。

家目录 `$KIMI_CODE_HOME`，否则 `~/.kimi-code`。文件是 `config.toml`。供应商在 `[providers.<id>]`，模型在 `[models.<alias>]`。

`provider.type` 只能是 `kimi`、`openai`、`openai_responses`、`anthropic`、`google-genai`、`vertexai`。还有 `base_url`。

密钥三选一：`api_key`、`api_key_env`、`oauth.key`。`api_key_env` 不能和 `api_key` 或 OAuth 同时出现。

每个 `[models.<alias>]` 必须有 `model`（上游真实模型名）和 `max_context_size`（正整数），`provider` 字段必须等于它所属的 provider id。

`default_model` 指向一个模型别名，别名再经 `models.<alias>.provider` 反查出供应商。这是用户的选择，本工具不主动改。

> 查找：CC Switch `src-tauri/src/kimi_config.rs` 的 `validate_provider`。我们 `src-tauri/src/providers/kimi.rs`。

## MiniMax Code

本机未安装。下面是 CC Switch 的实现，我们已按它移植。

家目录 `$MINIMAX_DATA_DIR` 或 `$MAVIS_DATA_DIR`，否则 `~/.minimax`。文件是 `config.yaml`。供应商在 `custom_provider.<id>`。

```yaml
custom_provider:
  <id>:
    name: 显示名
    kind: custom
    enabled: true
    api: openai-completions
    options:
      baseURL: https://...
      apiKey: sk-...
    models:
      <modelId>:
        name: ...
        reasoning: true
        modalities: { input: [text], output: [text] }
        limit: { context: 400000, output: 128000 }
```

`api` 三选一：`anthropic-messages`、`openai-completions`、`openai-responses`；缺省按 `anthropic-messages`。

`kind` 不为 `custom` 的节点是 MCode 自己的登录账号，拒绝覆盖。`minimax_api` 和 `defaultModel` 也归 MCode。

> 查找：CC Switch `src-tauri/src/mcode_config.rs` 的 `config_path` 和 `validate_provider`。我们 `src-tauri/src/providers/minimax.rs`。

## Claude Desktop

本机未安装（`%LOCALAPPDATA%\Claude-3p` 不存在），我们**没有做注入器**。下面只记录 CC Switch 的实现，将来要加时照这份来。

每个平台两个目录：正常版 `Claude` 与 3P 版 `Claude-3p`。Windows 上是 `%LOCALAPPDATA%\Claude` 和 `%LOCALAPPDATA%\Claude-3p`。

四处文件一次提交：

1. `<Claude>/claude_desktop_config.json` —— `deploymentMode: "3p"`
2. `<Claude-3p>/claude_desktop_config.json` —— `deploymentMode: "3p"`
3. `<Claude-3p>/configLibrary/00000000-0000-4000-8000-000000157210.json` —— 核心 profile
4. `<Claude-3p>/configLibrary/_meta.json` —— 登记这个 profile

profile 的关键字段：`inferenceProvider`（固定 `gateway`）、`inferenceGatewayBaseUrl`、`inferenceGatewayApiKey`、`inferenceGatewayAuthScheme`（固定 `bearer`）、`inferenceModels`、`modelDiscoveryEnabled`。

缺失时才写、已有就不动的：`disableDeploymentModeChooser`、`coworkEgressAllowedHosts`。

模型列表 `inferenceModels` 每项可为纯字符串，或 `{name, labelOverride?, supports1m?, prefer1m?}`，上限 200 条。

切回官方：两处 `deploymentMode` 改 `1p`，删除旧版网关配置，profile 文件保留但只清关键字段（Key 从磁盘清掉）。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `DESKTOP_PROFILE_FLOOR`，`src-tauri/src/claude_desktop_config.rs`。

## Cursor

**Cursor 支持自带密钥（BYOK），但这个 provider 现在没生效，原因是「OpenAI API Key」开关没打开。**

### 先说结论

| 项 | 结论 |
| --- | --- |
| Cursor 支不支持自定义接口 | **支持**。设置里有「OpenAI API Key」和「Override OpenAI Base URL」 |
| 为什么现在用不了 | `useOpenAIKey` 是 `false`。只填了 key 不算数，开关必须打开 |
| 配置在哪 | `%APPDATA%\Cursor\User\globalStorage\state.vscdb`（SQLite） |
| 关键键 | `applicationUser` 里的 `openAIBaseUrl`、`useOpenAIKey`、`availableAPIKeyModels` |
| 密钥在哪 | 同库的 `secret://cursorAuth/openAIKey`，Electron safeStorage 加密 |
| agentcenter 能不能自动配 | 不做。要写加密值 + 手动开关 + 重启，升级就失效 |
| 该怎么做 | 用户自己在界面上点几下，步骤见下 |

### 报错的真实原因

看到的报错是：

```
AI Model Not Found
Model name is not valid: "deepseek/deepseek-v4.1-flash"
```

日志里对应 `ERROR_BAD_MODEL_NAME (5)`，来自 `streamFromAgentBackend`。看起来像是 Cursor 不认这个模型，实际是 **Cursor 收到这个请求时没带上你的 API key**，于是把这个名字当自家目录里的模型去校验，校验不过就拒了。

Cursor 官方论坛里同样的报错有明确解释（`forum.cursor.com/t/.../158958`）：

> On the first error (Model name is not valid: ""), it happens when Override OpenAI Base URL is enabled, but the OpenAI API Key toggle is off.

另一个帖子（`/t/.../174141`）里官方回复说得更直接：

> The Model name is not valid error shows up when the request reaches us without your API key. In that case, Cursor treats the name as one of its own catalog models.

本机实测三个值，完全对得上：

```
useOpenAIKey      = false        ← 根因
openAIBaseUrl     = null
availableAPIKeyModels = null
```

而账号是 `membershipType: pro`、`subscriptionStatus: active`，不存在「免费用户不能用自定义模型」的限制。

### 正确配置步骤

Cursor Settings（`Ctrl+Shift+J`）→ Models → 展开 **API Keys**：

1. **打开「OpenAI API Key」开关**（这一步最容易漏，只粘 key 不开开关等于没配）。
   弹窗会提示部分功能走不了自带密钥，点「Enable」确认。
2. 在「API Key」里粘贴 CommandCode 的 key。
3. 打开「Override OpenAI Base URL」，填：

   ```
   https://api.commandcode.ai/provider/v1
   ```

   不末尾加 `/v1`（地址本身已经带 `/provider/v1`），填完 Cursor 会自动补 `/chat/completions`。
4. 确认「Anthropic API Key」和「Google API Key」两个开关是**关**的 —— 开着的话
   `claude-*` / `gemini-*` 开头的名字会走别的槽。
5. 在模型选择器里 Add Custom Model，填 `deepseek/deepseek-v4.1-flash`（服务商目录里的原名）。
6. 保存后**重启 Cursor**，让 `useOpenAIKey` 落盘。

### agentcenter 怎么做这套动作

实现了两处入口，共用 `src-tauri/src/providers/cursor_db.rs` 这份逻辑。它
**不进 `providers sync`** —— Cursor 的配置是 4 GB 的 SQLite、密钥要加密、
还必须先退出 Cursor，塞进自动同步会让「一条命令改所有 agent」变得不可控。

**界面**：「启用 provider」页里 Cursor 那张卡片（和别的 agent 卡片同形，内容
是 `src/lib/components/CursorByok.svelte`）。显示总开关、接口地址、密钥长度；
下面是模型选择器，可以勾选母版里这条 provider 的模型加进 Cursor，也能删除
Cursor 里已有的。Cursor 在运行时相关按钮都是灰的。

**命令行**：

```
agentcenter providers cursor status          # 看当前状态
agentcenter providers cursor plan command    # 看还差什么，不写
agentcenter providers cursor apply command --models a,b   # 写入并加模型
agentcenter providers cursor remove-model a,b             # 从选择器删模型
agentcenter providers cursor reset           # 关掉，回官方模型
```

命令之间的分工：

| 命令 | 写盘 | 前置条件 |
| --- | --- | --- |
| `status` | 否 | 无 |
| `plan` | 否 | 无 |
| `apply` | 是 | Cursor 进程不存在，否则退出码 2 |
| `remove-model` | 是 | 同上 |
| `reset` | 是 | 同上 |

### 模型选择器：还得手工加名字，但可以代做

**关键事实**：在 Cursor 里点「Add Custom Model」只做一件事 —— 往
`aiSettings.userAddedModels` 里塞一个字符串。目录条目
（`availableDefaultModels2` 里那条 `isUserAdded: true`）是 Cursor 自己补的：

```js
// handleAvailableModelsChange，workbench.glass.main.js
const i = applicationUserPersistentStorage.aiSettings.userAddedModels ?? [];
for (const a of i) n.includes(a) || n.push(a);   // 缺的补进可用列表
```

所以 agentcenter 只需要维护那个字符串数组，不用自己构造模型定义。

**但它不能替你决定加哪个**。母版里一条 provider 往往有好几个模型
（`command` 有 4 个），自动全加会把选择器塞满。所以界面上的模型是多选框：
勾哪个加哪个，不勾不加。CLI 用 `--models a,b` 显式指定，省略就完全不动模型列表。

**删除**是独立的操作（`remove-model` / 界面上的垃圾桶图标），只改
`userAddedModels`，不动开关、地址、密钥 —— 删一个模型不该顺带重写接口配置。

界面上会把 Cursor 里的模型分成两组显示：属于当前 provider 的（可勾选/取消），
和不在这个 provider 列表里的（只给删除按钮）。后者可能是别的 provider 或
用户手加的，不能因为切换 provider 就看不见。

`apply` 会做四件事，而且只做真正需要的那几件：

1. `useOpenAIKey` 置 `true`（如果已经是 true 就跳过）
2. `openAIBaseUrl` 改成母版的 `base_url`（末位斜杠不算差异）
3. 密钥用 safeStorage 同款算法加密后写进 `secret://cursorAuth/openAIKey`
   （长度和后 4 位都一致就认为是同一把，不重写）
4. 勾选的模型名追加进 `aiSettings.userAddedModels`（已在里面的不重复加）

只写密钥时不会碰 `applicationUser` 那一整条 JSON —— `CursorPlan::touches_application_user()`
会先判断这次改动到底涉不涉及它，涉及时才读改写那 540 KB。

加密这块是自己实现的，没有依赖 Electron：

```rust
// 1. Local State 的 os_crypt.encrypted_key，去掉 "DPAPI" 前缀后交给 Windows DPAPI
let master = dpapi_unprotect(&base64_decode(state["os_crypt"]["encrypted_key"])?[5..])?;
// 2. v10 + 12 字节 nonce + AES-256-GCM 密文
let mut blob = b"v10".to_vec();
blob.extend_from_slice(&nonce);
blob.extend_from_slice(&Aes256Gcm::new(master).encrypt(&nonce, key.as_bytes())?);
```

### 为什么必须要求 Cursor 退出

`applicationUser` 是一整条 540 KB 的 JSON。Cursor 把它缓存在内存里，
退出或定时保存时整条写回。我们趁它开着改库，会出现两种情况：

- 它下次保存时把我们写的值覆盖掉 —— 配置静默失效，用户以为改了其实没改；
- 并发写 4 GB 的 SQLite，有损坏风险。

所以 `apply` 会查 `tasklist`，发现 `Cursor.exe` 就拒绝并让用户退出。
`--force` 可以绕过，但只该用在自己知道在做什么的场景（比如测试写副本）。

### 已实测的部分

在真实库的副本上（4.26 GB，读写都走真实代码路径）跑完整流程，
结果记录在下面的验证表里：

| 验证项 | 结果 |
| --- | --- |
| `status` 读出开关/地址/密钥长度 | 与 Python 独立解密结果一致 |
| `plan` 识别出「开关没开 + 地址不对」 | 列出两条改动 |
| `apply` 写入副本 | 开关变 true，地址变成 CommandCode |
| Rust 加密、Python 解密交叉验证 | 93 字符，开头 `user_`，结尾 `x9Uv` |
| 第二次 `apply` | 报「无需改动」，幂等 |
| `availableDefaultModels2` / `aiSettings` / `membershipType` | 写入前后逐字节一致 |
| Cursor 运行中执行 `apply` | 退出码 2，拒绝写入 |

### 已知的脆弱点

| 风险 | 说明 |
| --- | --- |
| 键名是私有接口 | `applicationUser` 那个长键名是 Cursor 内部路径，升级可能改。改了会明确报「库里没有这个键」，不会静默写坏 |
| 密钥按用户绑定 | DPAPI 主密钥跟 Windows 账户走，换机器或换账户读不出旧密钥 |
| `reset` 不清密钥 | 只关开关、清地址。密钥留着，方便再启用时不用重填 |

### 网上还有别的坑（提前知道）

- **Agent 模式要求 tools 格式标准**。有用户反馈 Cursor 发的 `tools` 是扁平结构，某些网关要嵌套的 `{type:"function", function:{...}}`，会报 `'function' is required when type is 'function'`。CommandCode 是否兼容要看实际跑一次。
- **Base URL 不能指向 localhost / 内网**。Cursor 服务端要能访问到，否则报 `Access to private networks is forbidden`。
- **模型名里不要带 `claude` / `anthropic` / `gemini` 字样**，会被路由到别的密钥槽。`deepseek/deepseek-v4.1-flash` 没这个问题。
- **推理模型可能不受支持**。Cursor 官方文档把推理模型列在 BYOK 不支持的范围里。

### 如果配好了还是不行

先看日志判断请求有没有发出去：

```
%APPDATA%\Cursor\logs\<时间>\window*\exthost\anysphere.cursor-agent-host\
```

- 请求到了 CommandCode、报鉴权或格式错 → 是服务商兼容性问题
- 本地就报 `ERROR_BAD_MODEL_NAME` → 还是开关没开，或者 key 没存进去

> 查找：本机 `C:\dev\ide\cursor\resources\app\out\vs\workbench\workbench.glass.main.js`（`models-provider-controls.react.js` 里的 `Ux2` 组件就是这块设置面板）；文档证据来自 `forum.cursor.com` 的 158958 和 174141 两个帖子，以及 `github.com/18534516725/llm-api-setup-guides` 的 Cursor 说明。

## 这一轮核对到的偏差汇总

按 CC Switch 逐文件对过之后改掉的地方，按影响从大到小：

| 工具       | 原来                                        | 现在                                                       | 为什么                                      |
| -------- | ----------------------------------------- | -------------------------------------------------------- | ---------------------------------------- |
| Hermes   | 写 `providers:` 字典                         | 写 `custom_providers:` 列表                                 | v12 起 `providers:` 归 Hermes 自己管，写入会被覆盖   |
| Hermes   | `api_mode` 只有两档                           | 四档，按协议映射                                                 | Anthropic 端点被写成 `chat_completions` 会用错格式 |
| Codex    | 每家一张表 + `env_key` + 生成 `codex-env.ps1`    | 单槽位 + 一张 `custom` 表 + 内联 token                           | Codex 一次只有一家；0.149 起不读 `auth.json`       |
| Pi       | key 写 `auth.json`                         | key 内联在 `models.json`                                    | `auth.json` 归 Pi 的 `/login`，两处都写会互相覆盖    |
| opencode | key 写 `~/.local/share/opencode/auth.json` | key 内联在条目里                                               | 那个 auth.json 是 opencode 自己的登录凭据          |
| opencode | 固定写 `opencode.json`                       | `.jsonc` 优先                                              | 两个文件可能同时存在，写错文件等于没生效                     |
| Claude   | 只接管 10 个 `ANTHROPIC_*`                    | 完整关键字段清单（前缀 + 点名 + 协议选择器）                                | 切 Bedrock / Vertex 时旧凭据会残留               |
| Claude   | 按模型名判断要不要转换                               | 只看协议                                                     | Anthropic 兼容端点的模型名不是 `claude-*`，直连正常     |
| Claude   | 代理模式写上游真实模型名                              | 写稳定 `claude-*` 别名                                        | 未知模型名会让 Claude Code 按 200K 窗口、关思考处理      |
| DSH      | 只写 `desktop`                              | 写 `web` / `desktop` / 存在的 `headless` / `acp`             | 各 profile 的 patch 文件独立，只写一个会让列表不一致       |
| Claude   | 没有引导开关                                    | 新增「跳过首次引导」，写 `~/.claude.json` 的 `hasCompletedOnboarding` | 用第三方中转时首次登录 Anthropic 账号是多余的             |
