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
model = "<第一个模型的 id>"
model_reasoning_effort = "<母版填了才写>"

[model_providers.custom]
name = "显示名"
base_url = "https://..."
wire_api = "responses"
experimental_bearer_token = "sk-..."
requires_openai_auth = false
```

- `wire_api` 无论母版协议是什么都写 `responses` —— Codex 只支持这一种
- 早期实现按母版 id 建表、用 `env_key` + `~/.agents/codex-env.ps1`。这一轮改掉了：单槽位 + 内联 token，不再生成脚本
- 旧形态留下的表（表名是母版里出现过的 id、且不是 `custom`）会被清掉，否则 `env_key` 会和新的 token 并存
- 已有 `custom` 表时只改本工具声明管的键，`http_headers`、`query_params` 保留

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

界面上这个开关在侧栏的 **Claude & Codex** 页里。Codex 是单槽位，同一时刻只有一条
provider 生效，所以那一页只显示当前分配给 Codex 的那一条。

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
