> 给谁：改某个工具的 provider 注入器之前，先查该改哪个文件、哪些键。
>
> 一句话：每个工具的配置文件、该改的键，以及 agentcenter 现在实际写了什么。
>
> 不管：母版 `providers.toml` 的字段、界面、同步时的备份与哈希复查。那些在 [providers-management.md](providers-management.md)。
>
> 状态：2026-11-11。CC Switch 一列从 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的 `main` 读出，没有钉死 commit。「我们」一列从本仓库注入器读出，还没拿对应工具的当前版本逐项对过，整列标待确认。

`~` 是用户主目录。下面说的「关键字段」只指四件事：请求发到哪、用什么密钥、用哪个模型、走哪种协议。其余键归用户和工具自己，切换时不要动。

CC Switch 对 Claude Code、Codex、Gemini CLI、Grok Build 是一次只留一家：先清掉上一家的关键字段，再写这一家。OpenCode、OpenClaw、Hermes、Pi、DeepSeek Harness、Kimi Code、MiniMax Code 是多家并存，只增删对应条目。

## 总表

| 工具 | 文件 | 一次几家 | 我们有没有注入器 |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/settings.json` | 一家 | 有，待确认 |
| Codex | `~/.codex/config.toml` | 一家 | 有，待确认；另写了 `~/.agents/codex-env.ps1` |
| OpenCode | `~/.config/opencode/opencode.jsonc` 或 `opencode.json` | 多家 | 有，待确认；另写了 `~/.local/share/opencode/auth.json` |
| Pi | `~/.pi/agent/models.json` | 多家 | 有，待确认；另写了 `auth.json` |
| Hermes | `~/.hermes/config.yaml` | 多家 | 代码在，没挂进注入列表，待确认 |
| DeepSeek Harness | `~/.dsh/profiles/<profile>/cordis.patch.yml` 和 `~/.dsh/.credentials.yaml` | 多家 | 有，只写 desktop，待确认 |
| ZCode | `~/.zcode/v2/config.json` 和 `provider_config.json` | 多家 | 有，待确认；CC Switch 没有这个工具 |
| WorkBuddy | `~/.workbuddy/models.json` | 多家 | 有，待确认；CC Switch 没有这个工具 |
| Gemini CLI | `~/.gemini/.env` 和 `~/.gemini/settings.json` | 一家 | 没有 |
| Grok Build | `~/.grok/config.toml` | 一家 | 没有 |
| OpenClaw | `~/.openclaw/openclaw.json` | 多家 | 没有 |
| Kimi Code | `~/.kimi-code/config.toml` | 多家 | 没有 |
| MiniMax Code | `~/.minimax/config.yaml` | 多家 | 没有 |
| Claude Desktop | `configLibrary/<profile id>.json` | 一家 | 没有；父目录未逐行打开，待核对 |

我们的注入列表在 `src-tauri/src/providers/mod.rs` 的 `all_injectors()`：ZCode、Pi、OpenCode、Codex、WorkBuddy、DeepSeek Harness、Claude Code。Hermes 的注入器文件在，这个函数里没有它，点「应用到各 agent」不会写 Hermes。

## Claude Code

文件是 `~/.claude/settings.json`。旧安装可能是 `claude.json`，CC Switch 的路径函数会兼顾这个名字。一次只能有一家。

CC Switch 切换时先清空下面全部关键字段，再写目标供应商带来的值。清的是键，不是整份文件。`permissions`、`hooks`、`statusLine`、插件不动。

`env` 里按前缀算关键字段的：

| 前缀 | 盖住什么 |
| --- | --- |
| `ANTHROPIC_` | 地址、密钥、各档模型名、自定义头 |
| `AWS_` | Bedrock 的区域和凭据 |
| `VERTEX_REGION_` | Vertex 按模型分的区域 |

另外点名的 `env` 关键字段：

| 键 |
| --- |
| `CLAUDE_CODE_USE_BEDROCK` |
| `CLAUDE_CODE_USE_VERTEX` |
| `CLAUDE_CODE_USE_FOUNDRY` |
| `CLAUDE_CODE_USE_GATEWAY` |
| `CLAUDE_CODE_USE_MANTLE` |
| `CLAUDE_CODE_USE_ANTHROPIC_AWS` |
| `CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD` |
| `CLAUDE_CODE_SKIP_*_AUTH`（前缀 `CLAUDE_CODE_SKIP_` 且结尾 `_AUTH`） |
| `CLAUDE_CODE_SUBAGENT_MODEL` |
| `CLAUDE_CODE_SUBAGENT_MODEL_FORCE` |
| `CLOUD_ML_REGION` |
| `GOOGLE_APPLICATION_CREDENTIALS` |
| `CLAUDE_CODE_OAUTH_TOKEN` |
| `CLAUDE_CODE_OAUTH_REFRESH_TOKEN` |
| `CLAUDE_CODE_OAUTH_SCOPES` |
| `CLAUDE_CODE_API_KEY_HELPER_TTL_MS` |

`CLAUDE_CODE_USE_` 不能按前缀全收。`CLAUDE_CODE_USE_POWERSHELL_TOOL`、`USE_NATIVE_FILE_SEARCH` 这类是功能开关，留给用户。

顶层关键字段：`apiKeyHelper`、`apiBaseUrl`、`primaryModel`、`smallFastModel`、`apiKey`、`model`、`fallbackModel`、`modelOverrides`、`modelPicker`、`advisorModel`、`awsAuthRefresh`、`awsCredentialExport`、`gcpAuthRefresh`。

供应商自己带进来、切走时只删「值没被改过」的 `env` 键：

| 键 | 作用 |
| --- | --- |
| `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` | 上游不接实验性 beta 头 |
| `CLAUDE_CODE_DISABLE_ARTIFACT` | 上游不接 Artifact 工具 |
| `ENABLE_TOOL_SEARCH` | 第三方地址默认关 |
| `CLAUDE_CODE_DISABLE_THINKING` | 网关兼容 |
| `DISABLE_INTERLEAVED_THINKING` | 网关兼容 |
| `CLAUDE_CODE_ALWAYS_ENABLE_EFFORT` | 网关兼容 |
| `CLAUDE_CODE_EXTRA_BODY` | 网关兼容 |
| `CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING` | 网关兼容 |
| `CLAUDE_CODE_AUTO_MODE_SERVER` | 网关要设 `0`，否则会卡住 |
| `CLAUDE_CODE_MAX_CONTEXT_TOKENS` | 窗口 |
| `CLAUDE_CODE_AUTO_COMPACT_WINDOW` | 窗口 |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` | 窗口 |
| `CLAUDE_CODE_DISABLE_1M_CONTEXT` | 窗口 |
| `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT` | 窗口 |
| `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY` | 向网关拉模型列表 |

模型档位用 `ANTHROPIC_DEFAULT_{OPUS,FABLE,SONNET,HAIKU}_MODEL`，再加同名的 `_NAME` 和 `_DESCRIPTION`。默认模型是 `ANTHROPIC_MODEL`。密钥统一写 `ANTHROPIC_AUTH_TOKEN`，同时拿掉 `ANTHROPIC_API_KEY`，避免两个都在时 Claude Code 报警。

我们（待确认）只写 `env` 里这些键，顶层 `model` 和上面那批兼容开关都不写：

| 键 | 我们怎么填 |
| --- | --- |
| `ANTHROPIC_BASE_URL` | 母版 `base_url` |
| `ANTHROPIC_AUTH_TOKEN` | 母版密钥 |
| `ANTHROPIC_API_KEY` | 有就删掉 |
| `ANTHROPIC_MODEL` | `claude_model_map.default`，缺了用模型列表第 1 个 |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` 和 `_NAME` | 映射的 opus，缺了用第 1 个模型 |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` 和 `_NAME` | 映射的 sonnet，缺了用第 2 个 |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` 和 `_NAME` | 映射的 haiku，缺了用第 3 个 |

没有 Fable 档。文件不存在时整次跳过，不会新建 `settings.json`。取消分配时，只有当前 `ANTHROPIC_BASE_URL` 能对上母版里某个「没分给 claude」的端点，才清掉上面这张清单。对不上的（订阅登录、用户自己写的）不动。

映射进 Claude 的模型里，只要有一个不是 `claude-` 开头（厂商前缀算在斜杠后面），或者这家的协议不是 Anthropic，`ANTHROPIC_BASE_URL` 就改成 `http://127.0.0.1:15722`。窗口开着时，这个地址把 `/v1/messages` 翻成上游的 Chat Completions 或 Responses，再把回复翻回来。`claude-` 模型仍然原样转发到上游的 `/messages`。上游地址写在 `~/.agents/claude-bridge.json`，密钥仍然只放在 `ANTHROPIC_AUTH_TOKEN`。窗口关掉后转换停止，直连 Anthropic 的那家不受影响。

> 查找：我们 `src-tauri/src/providers/claude.rs` 的 `OWNED_KEYS`、`src-tauri/src/providers/claude_bridge.rs`。CC Switch 的对照仍是 `src-tauri/src/live/floor.rs` 的 `CLAUDE_FLOOR_*`。

## Codex

文件是 `~/.codex/config.toml`。`auth.json` 只留给官方登录。

CC Switch 把第三方统一写成一张表 `[model_providers.custom]`。密钥写进这张表的 `experimental_bearer_token`。Codex 0.149 起自定义 provider 不再读 `auth.json` 里的密钥。`requires_openai_auth = true` 而 `auth.json` 没有登录时，Codex 会停在登录页。

顶层关键字段：`model_provider`、`openai_base_url`、`model`、`review_model`、`model_reasoning_effort`、`plan_mode_reasoning_effort`、`disable_response_storage`、`model_catalog_json`、`experimental_bearer_token`、`base_url`、`wire_api`。

嵌在别的表里、只清这几个键：`agents.default_subagent_model`、`agents.default_subagent_reasoning_effort`、`memories.extract_model`、`memories.consolidation_model`。

供应商独有、切走时要收回的顶层键：`web_search`、`model_context_window`、`model_auto_compact_token_limit`、`model_supports_reasoning_summaries`、`model_verbosity`。

表名 `openai`、`ollama`、`lmstudio` 会让 Codex 0.148 起拒绝加载。不要用这三个当自定义 provider 的 id。

我们（待确认）按母版 id 建 `[model_providers.<id>]`，写 `name`、`base_url`、`env_key`、`wire_api`、`requires_openai_auth = false`。密钥不进 toml，写进 `~/.agents/codex-env.ps1`，变量名是 `<ID>_API_KEY`。顶层 `model_provider` 指向第一个分给 codex 的 provider，`model` 用它的第一个模型，母版填了推理档位才写 `model_reasoning_effort`。`wire_api` 无论母版协议是什么，都写成 `responses`。已有表只改 `name`、`base_url`、`env_key`。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `CODEX_FLOOR_TOP`，`src-tauri/src/live/project/codex.rs` 的 `ROUTE_ID = "custom"`。我们 `src-tauri/src/providers/codex.rs`。

## OpenCode

CC Switch 先找 `~/.config/opencode/opencode.jsonc`，没有再用 `opencode.json`，两个都没有就写 `opencode.json`。多家并存。

两套字段，由条目自己带哪些键决定：

| 套 | 顶层键 | 条目里的键 |
| --- | --- | --- |
| V1（旧） | `provider` | `npm`、`options`、`api` |
| V2（现在的原生） | `providers` | `package`、`settings`、`headers`、`body`、`canonical` |

新的 V1 条目要有 `npm` 和至少一个模型。密钥放在条目里，这次没在 `opencode_config.rs` 里看到它写 `~/.local/share/opencode/auth.json`。

我们（待确认）固定写 `opencode.json`，不读 `opencode.jsonc`。按本机已装主版本选套：主版本号小于 2，或者探测失败，用 V1（`npm` + `options.baseURL`）；否则用 V2（`package` + `settings.baseURL`）。密钥写到 `~/.local/share/opencode/auth.json`，形状是 `{ "<id>": { "type": "api", "key": "..." } }`。模型是 `models.<id>.name`。

V1 的 npm 包：Anthropic 用 `@ai-sdk/anthropic`，OpenAI Responses 用 `@ai-sdk/openai`，其余用 `@ai-sdk/openai-compatible`。V2 的 package：Anthropic 用 `@opencode/ai/providers/anthropic`，其余用 `@opencode/ai/providers/openai-compatible`。

> 查找：CC Switch `src-tauri/src/opencode_config.rs` 的 `resolve_config_path`、`LEGACY_ONLY_KEYS`、`NATIVE_ONLY_KEYS`。我们 `src-tauri/src/providers/opencode.rs`。

## Pi

目录是 `~/.pi/agent/`。

CC Switch 只改 `models.json` 里的 `providers.<id>`。`settings.json` 的当前 provider 和模型归 Pi。`auth.json` 是 Pi 的 `/login`，CC Switch 不读也不写。

我们（待确认）写两个文件，不改 `settings.json`：

| 文件 | 键 |
| --- | --- |
| `auth.json` | `<id>.type = "api_key"`，`<id>.key` 是密钥 |
| `models.json` | `providers.<id>.name`、`baseUrl`、`api`、`models[]` |

`api` 的值：`anthropic-messages`、`openai-completions`、`openai-responses`、`google-generative-ai`。模型条目写 `id`、`name`、`input`、`contextWindow`、`maxTokens`；母版填了才写 `reasoning` 和 `thinkingLevelMap`。

> 查找：CC Switch `src-tauri/src/pi_config/mod.rs` 文首说明。我们 `src-tauri/src/providers/pi.rs`。

## Hermes

文件是 `~/.hermes/config.yaml`。`HERMES_HOME` 非空时用它。

CC Switch 写的是 `custom_providers` 列表，每项有 `name`、`base_url`、`api_key`、`model`、`models.<id>.context_length`。Hermes v12 又有 `providers` 字典。CC Switch 把这个字典当成只读，不往里面写。

我们（待确认）写的是 `providers` 字典，键是母版 id，字段有 `name`、`base_url`、`api_mode`、`transport: openai_chat`、`model`、`models.<id>.context_length`、`models.<id>.supports_tools`、`api_key`。`api_mode` 只有两种：OpenAI Responses 写 `responses`，其余写 `chat_completions`。顶层 `model:` 段不改。这个注入器没有进 `all_injectors()`。

> 查找：CC Switch `src-tauri/src/hermes_config.rs` 的 `PROVIDER_SOURCE_CUSTOM_LIST` 和 `PROVIDER_SOURCE_DICT`。我们 `src-tauri/src/providers/hermes.rs`。

## DeepSeek Harness

家目录是 `~/.dsh`，`DSH_HOME` 非空时用它。各 profile 共用一份 `~/.dsh/.credentials.yaml`，密钥在 `refs` 下。

CC Switch 写这四个 profile 的 `profiles/<名字>/cordis.patch.yml`：`web`、`desktop`、`headless`、`acp`。`sdk` 和 `sdk-minimal` 不写。读的时候把密钥填进 provider 的 `options.baseURL` 和 `options.apiKey`。补丁里的 provider 挂在 `llm-pi-ai` 这一条下。

我们（待确认）只写 `profiles/desktop/cordis.patch.yml` 和 `.credentials.yaml`。provider 块的键是 `displayName`、`apiKeyEnv`、`api`、`baseURL`、`models`。密钥不写进补丁，写进 credentials 的 `refs.<ID>_API_KEY`。`api` 的值和 Pi 一样四种。

> 查找：CC Switch `src-tauri/src/dsh_config.rs` 的 `TARGET_PROFILES`、`credentials_path`。我们 `src-tauri/src/providers/dsh.rs`。

## ZCode

CC Switch 没有这个工具。下面全是我们的注入器，待确认。

两个文件都写。只写 `config.json` 时，设置页的自定义供应商列表不出条目。身份用 `base_url` 对，ZCode 的键是 UUID，对不上母版 id。

| 文件 | 写什么 |
| --- | --- |
| `~/.zcode/v2/config.json` | `provider.<uuid>.name`、`kind`（`anthropic` 或 `openai-compatible`）、`options.apiKey`、`options.baseURL`、`options.apiKeyRequired`、`source: custom`、`models.<id>` |
| `~/.zcode/v2/provider_config.json` | 供应商规则：`config.access.apiKey`、`config.api.type`、`config.api.baseUrl`、`personalModelIds`、`modelOrder`；模型规则：`contextWindow` |

`config.json` 里模型还写 `limit.context`、`limit.output`、`modalities.input`、`modalities.output`，有推理时写 `reasoning.enabled`、`variants`、`defaultVariant`。

`provider_config.json` 的 `api.type`：`anthropic-messages`、`openai-responses`、`openai-chat-completions`。Google 这一类也写成 `openai-chat-completions`。

> 查找：`src-tauri/src/providers/zcode.rs` 的 `to_zcode_provider` 和 `to_personal_provider_rule`。

## WorkBuddy

CC Switch 没有这个工具。下面是我们的注入器，待确认。

文件是 `~/.workbuddy/models.json`，一个扁平数组。一个模型一条，同一家的 `url` 和 `apiKey` 在每条里重复一遍。按模型 `id` 匹配；已有条目只改 `url` 和 `apiKey`。取消分配时按 `url` 删掉能对上母版端点的条目。

新条目的键：`id`、`name`、`vendor: Custom`、`url`、`apiKey`、`supportsToolCall`、`supportsImages`、`supportsReasoning`、`useCustomProtocol`、`maxInputTokens`。母版填了推理档位才写 `reasoning.supportedEfforts`。`useCustomProtocol` 只有 OpenAI Responses 为 true。

> 查找：`src-tauri/src/providers/workbuddy.rs` 的 `make_entry`。

## Gemini CLI

我们没有注入器。下面是 CC Switch。

两个文件，目录都是 `~/.gemini/`：

| 文件 | 改哪些键 |
| --- | --- |
| `.env` | 先删掉下面算关键字段的行，再写目标供应商的。注释和其余行不动 |
| `settings.json` | 只改 `security.auth.selectedType` 和 `model.name` |

`.env` 里算关键字段的：所有 `GOOGLE_*`（包括 `GOOGLE_GEMINI_BASE_URL`、`GOOGLE_API_KEY`、`GOOGLE_GENAI_USE_VERTEXAI`、`GOOGLE_CLOUD_PROJECT`），加上点名的 `GEMINI_API_KEY`、`GEMINI_MODEL`、`GEMINI_API_KEY_AUTH_MECHANISM`、`GEMINI_CLI_CUSTOM_HEADERS`、`GEMINI_DEFAULT_AUTH_TYPE`、`GEMINI_CLI_USE_COMPUTE_ADC`、`CODE_ASSIST_ENDPOINT`、`CODE_ASSIST_API_VERSION`。

`GEMINI_SANDBOX`、`GEMINI_CLI_HOME`、`GEMINI_SYSTEM_MD` 不算，留给用户。

`selectedType`：官方 Google 登录写 `oauth-personal`，第三方写 `gemini-api-key`。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `gemini_floor_env`，`src-tauri/src/gemini_config.rs` 的路径函数。

## Grok Build

我们没有注入器。下面是 CC Switch。

文件是 `~/.grok/config.toml`。写两处：`models.default` 指向表名，以及 `[model."<表名>"]` 整张表。用户自己的其他 `[model.*]` 表不碰。切走时按上次写入记录删表，不按当前 `models.default` 去找，因为 Grok 的 `/settings` 会把它改成内置模型。

表里要有的键：`model`、`base_url`、`name`、`api_key` 或 `env_key`、`api_backend`、`context_window`。还会动 `models.session_summary`（会话标题用的模型）。官方卡不写模型表，Grok 回到自带的 xAI 登录。

> 查找：CC Switch `src-tauri/src/live/project/grok.rs`，`src-tauri/src/grok_config.rs` 的 `GrokModelConfig`。

## OpenClaw

我们没有注入器。下面是 CC Switch。

文件是 `~/.openclaw/openclaw.json`。供应商在 `models.providers.<id>`。条目键（JSON 里是驼峰）：`baseUrl`、`apiKey`、`api`、`models`、`headers`。模型条目有 `id`、`name`、`alias`、`cost.input`、`cost.output`、`contextWindow`。当前模型在 `agents.defaults.model.primary`，备用在 `fallbacks`。

> 查找：CC Switch `src-tauri/src/openclaw_config.rs` 的 `OpenClawProviderConfig`。

## Kimi Code

我们没有注入器。下面是 CC Switch。

文件是 `~/.kimi-code/config.toml`。供应商在 `[providers.<id>]`，模型在 `[models.<id>]`。

`provider.type` 只能是 `kimi`、`openai`、`openai_responses`、`anthropic`、`google-genai`、`vertexai`。还有 `base_url`。密钥三选一：`api_key`、`api_key_env`、`oauth.key`。`api_key_env` 不能和 `api_key` 或 OAuth 同时出现。至少一个模型，模型对象里要有 `model`。

> 查找：CC Switch `src-tauri/src/kimi_config.rs` 的 `validate_provider`。

## MiniMax Code

我们没有注入器。下面是 CC Switch。

文件是 `~/.minimax/config.yaml`。`MINIMAX_DATA_DIR` 或 `MAVIS_DATA_DIR` 非空时用它们作家目录。供应商在 `custom_provider.<id>`。

键：`api`（`anthropic-messages`、`openai-completions`、`openai-responses`）、`options.baseURL`、`options.apiKey`、`models`。`api` 缺省时按 `anthropic-messages` 验证。同一个文件里的 `minimax_api` 和 `defaultModel` 不属于这些条目，写入时保留。

> 查找：CC Switch `src-tauri/src/mcode_config.rs` 的 `config_path` 和 `validate_provider`。

## Claude Desktop

我们没有注入器。CC Switch 写的文件名是 `configLibrary/<profile id>.json`。这个文件所在的目录在 `claude_desktop_config.rs`，这次没有逐行打开那个文件，目录待核对。

已经从关键字段表读到的键：`inferenceProvider`、`inferenceGatewayBaseUrl`、`inferenceGatewayApiKey`、`inferenceGatewayAuthScheme`、`inferenceModels`、`modelDiscoveryEnabled`。缺失时才写、已经有就不动的：`disableDeploymentModeChooser`、`coworkEgressAllowedHosts`。

> 查找：CC Switch `src-tauri/src/live/floor.rs` 的 `DESKTOP_PROFILE_FLOOR`。

## 这次没逐行打开的写入

CC Switch 还管 GitHub Copilot CLI、Antigravity、Oh My Pi。这三个的写入函数这次没读，不列键。只记住从切换入口看到的一句：Copilot 的自带模型卡写进 `providers.json`；Antigravity 的全量同步不写真实的 `settings.json`；Oh My Pi 走自己的供应商服务，不走通用的 live 写入。
