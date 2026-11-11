# 模型元数据管理与 Agent 接口化

> 落地记录。设计和取舍都在这里，代码注释里只留「为什么这么写」。

## 背景

两个问题。

**问题一：模型只有 id，能力元数据写死在代码里。**

`providers/zcode.rs` 里 `limit.context = 1_000_000`、`limit.output = 128_000`，
`providers/pi.rs` 里 `contextWindow = 1_000_000`、`maxTokens = 128_000`，
`providers/workbuddy.rs` 里 `maxInputTokens = 1_000_000`。

而真实配置差异很大：

| 来源 | 模型 | 上下文 | 输出 | 输入模态 |
|---|---|---|---|---|
| `~/.pi/agent/models.json` | wangxin / insigma-general-30b | 32768 | 8192 | text |
| `~/.pi/agent/models.json` | opencode-go / muse-spark-1.2 | 1048576 | 131072 | text+image |
| `~/.zcode/v2/config.json` | GLM-5.3 | 1000000 | 128000 | text |
| `~/.zcode/v2/config.json` | GLM-5.3-Flash | 1000000 | 128000 | text+image+video |

写死就把这些抹平成同一个值。

更严重的是**已有条目永不更新 models**：ZCode 只在 models 为空时补齐，
Pi / opencode / DSH 完全不覆盖。母版里改了模型参数，对已存在的 provider 零效果。

**问题二：新增一个 agent 要改 10 个文件。**

`registry.rs:53` 的注释写着「新增 agent 只需在这里加一条」，实际清单是：

`registry.rs`、`providers/mod.rs`、`providers/<agent>.rs`、`providers/matrix.rs`、
`providers/scan.rs`、`providers/diff.rs`、`mcp/inject.rs`、`mcp/matrix.rs`、
`mcp/scan.rs`、`mcp/diff.rs`、`credentials.rs`。

而且已经漂移：

- `providers/diff.rs` 的路径表漏了 workbuddy / dsh
- `mcp/matrix` 只有 7 列，`mcp/scan` 有 9 项
- `credentials.rs` 把 `gemini` 列进「不支持」，`registry.rs` 里根本没它

## 做法

### 一、Agent trait

`src-tauri/src/agents/mod.rs` 定义 trait，每个能力都是**带默认实现的选项**。
不支持的 agent 不写空函数，默认实现就是「没有这项能力」。

```rust
pub trait Agent: Send + Sync {
    // 身份
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn order(&self) -> u32 { 100 }

    // AGENTS 文档映射 + skills
    fn config_dir(&self) -> PathBuf;
    fn instruction_file(&self) -> Option<PathBuf> { None }
    fn skills_dir(&self) -> PathBuf;
    fn reads_shared_skills(&self) -> bool { false }
    fn installed(&self) -> bool { self.config_dir().exists() }

    // provider
    fn supports_provider(&self) -> bool { false }
    fn provider_ready(&self) -> bool { false }
    fn single_slot(&self) -> bool { false }
    fn provider_targets(&self) -> Vec<PathBuf> { Vec::new() }
    fn provider_scan(&self) -> Option<AgentScan> { None }
    fn provider_plan(&self, p: &[Provider]) -> anyhow::Result<Vec<FilePlan>> { Ok(vec![]) }

    // MCP
    fn supports_mcp(&self) -> bool { false }
    fn mcp_ready(&self) -> bool { false }
    fn mcp_scan(&self) -> Option<McpAgentScan> { None }
    fn mcp_plan(&self, s: &[&McpServer]) -> anyhow::Result<Vec<McpFilePlan>> { Ok(vec![]) }

    // 凭证体检
    fn credential_entries(&self) -> Vec<RawEntry> { Vec::new() }
    fn credential_unsupported(&self) -> Option<&'static str> { None }
}
```

### `supports_provider` 和 `provider_ready` 是两件事

这是本轮最重要的设计修正。回填测试时发现：用 `provider_ready()`
（配置文件在不在）校验母版里的分配关系，会把「配置给一个当前没装的 agent」
静默丢掉。

- `supports_provider()` 是「我实现了 provider 管理」，恒真
- `provider_ready()` 是「这台机器现在能注入吗」，看文件

母版校验用前者，注入调度用后者。用户可以在没装某个 agent 的机器上
配好分配，换台机器同步过去。

### 文件布局

```
agents/
  mod.rs        trait + all()/find()/ids()/provider_capable()/mcp_capable()
  model.rs      RawEntry、指纹、服务名归一化、JSON 读取
  claude.rs     单槽位 provider + mcp + 凭证 + CLAUDE.md
  codex.rs      provider + mcp + 凭证
  zcode.rs      provider + 凭证
  pi.rs         provider + mcp + 凭证
  opencode.rs   provider + mcp
  workbuddy.rs  provider + mcp
  dsh.rs        provider
  cursor.rs     mcp
  qoder.rs      mcp
  qoder_cn.rs / codebuddy.rs / trae_cn.rs / hermes.rs / zed.rs   路径与 skills
  gemini.rs     只有不支持声明（收编 credentials.rs 里游离的那条）
```

**agent 实现委托给原有的注入器**，不搬 4000 行已验证代码。
比如 `agents/zcode.rs` 的 `provider_plan()` 就一行：

```rust
fn provider_plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
    use crate::providers::Injector;
    crate::providers::zcode::ZCodeInjector.plan(providers)
}
```

`providers/` 和 `mcp/` 保留具体的配置格式处理（它们是「怎么改这个文件」，
不是「这个 agent 支持什么」）。`registry.rs` 删除。

### 调用点改造

| 位置 | 改成 |
|---|---|
| `providers/mod.rs::sync()` | 遍历 `agents::provider_capable()` 调 `provider_plan()` |
| `providers/matrix.rs::build()` | 列从 `agents::provider_capable()` 生成 |
| `providers/matrix.rs::set_row()` | 用 `provider_capable()` 的 id 过滤 |
| `providers/diff.rs::agent_config_path()` | 改用 `provider_targets()` |
| `mcp/inject.rs::sync()` | 遍历 `agents::mcp_capable()` 调 `mcp_plan()` |
| `mcp/matrix.rs::build()` | 同上 |
| `mcp/diff.rs::agent_path()` | 从 `agents::find()` 取 label |
| `credentials.rs::inspect()` | 遍历 `agents::all()` 收集 |
| `config.rs` / `skills.rs` | 改调 `agents::all()` |

## 二、模型元数据

### 母版结构

`Provider.models` 从 `Vec<String>` 变成 `Vec<ModelSpec>`：

```rust
pub struct ModelSpec {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub input_modalities: Vec<String>,     // text / image / video / pdf
    pub output_modalities: Vec<String>,
    pub reasoning: Option<bool>,
    pub reasoning_efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub tool_call: Option<bool>,
    pub cost: Option<ModelCost>,
}
```

除 `id` 外全部可选。留空 = 「不指定，沿用 agent 配置里的现有值或内置默认」。

### TOML 两种写法都认

`de_models` / `ser_models` 自定义 serde：

- **读**：`models = ["a", "b"]` 和 `models = [{id = "a", ...}]` 都能读
- **写**：整个 provider 里的模型全都没填元数据 → 写纯字符串数组（存量形状不变）；
  有任一模型带元数据 → 整组写内联表（TOML 不允许数组内混类型）

### 字段名两套

`ModelSpec` 加了 `rename_all = "camelCase"`（前端契约要 camelCase），
同时每个 snake_case 字段加了 `alias`（母版 TOML 惯例是 snake_case）。

这个坑在测试时踩到了：手写 `context_window = 2000000` 读进来是
`None`，注入出去变成默认的 1000000。加 alias 后两种写法都能读。

### 字段级合并

改「已存在的 provider」时**只更新母版里非 None 的字段**。
母版没填的保持 agent 配置里的原值。这是 `safe_write.rs` 的所有权清单
原则用在模型粒度上。

各注入器的合并实现：

| 注入器 | 函数 | 模型标识 |
|---|---|---|
| ZCode | `merge_models_into` | `models` map 的键 |
| Pi | `merge_pi_models` | `models[]` 数组里 `id` 匹配 |
| opencode | 只合并 name | map 的键 |
| WorkBuddy | 内联在 plan 里 | 扁平数组的 `id` |
| DSH | `provider_yaml` | 只在新 provider 时写 |
| Codex | 顶层 `model` + `model_reasoning_effort` | 取第一个 |
| Claude | 三个别名环境变量 | 取前三个 |

### 模型级不删

母版列出的模型 → 新增或更新；agent 里有、母版没列的 → **保留**。

理由：Pi 的 `models-store.json` 会从远端目录发现模型，删掉会误伤；
用户也可能故意让某个 provider 下多带几个模型。

provider 级的删除照旧：母版里有、但没分配给某个 agent 的整个 provider 会移除。

### 回填

`agentcenter models backfill [--dry-run]` + Tauri `backfill_models()`：

1. 遍历 `agents::all()` 调 `provider_scan()`
2. 按 `base_url` 把扫到的模型对回母版（provider id 各家不统一，端点才稳定）
3. **只填母版里空着的字段**，已有的一个都不动
4. 输出逐条报告

`FoundProvider.models` 同步从 `Vec<String>` 改成 `Vec<ModelSpec>`，
各 agent 的扫描函数补上元数据抽取。

## 三、界面

provider 的编辑表单里加模型编辑器：

```
模型（2）                        + 添加模型
┌ ▸ deepseek/deepseek-v4.1-flash   1M ctx · 128K out · text · reasoning ┐
┌ ▾ xiaomi/mimo-v2.5               1M ctx · 131K out · text             ┐
│   展示名 [        ]   默认推理档位 [high]
│   上下文窗口 [1000000]  输出上限 [131072]
│   输入模态 [text] [image] [video] [pdf]
│   推理 ☑   档位 [minimal] [low] [medium] [high] [xhigh] [max]
└──────────────────────────────────────────────────────────────────────┘
```

- 折叠态显示能力摘要，未填时显示「未填元数据」
- 展开态是字段输入框，留空表示「不指定」
- 「+ 添加模型」追加一行；垃圾桶移除该模型

其它界面改动：

- `AgentColumn` 加 `singleSlot`，前端删掉 5 处 `agent === 'claude'` 写死
- 「已分配」标签从裸 id 改成从 `matrix.columns` 反查 `label`
- 顶部加「从配置回填模型」按钮
- 边界说明文案改成「单槽位的 agent 只有一个生效」，不再写死 Claude 和具体 agent 名

## 四、Claude / Codex 的模型映射修复

**Claude**：重构前 opus / sonnet / haiku 三个别名全指向 `models[0]`，
`/model` 里三个档位是同一个模型。改成按序分配：

```
models[0] → OPUS
models[1] → SONNET     （不足时回落到 models[0]）
models[2] → HAIKU      （不足时回落到 models.last()）
```

`_MODEL_NAME` 用 `ModelSpec.name`，没填才用 id。

**Codex**：补写 `model_reasoning_effort`（母版填了 `default_effort` 时）。
`wire_api` 的「强制 responses」警告改成每个 provider 一次，不再每模型重复。

## 验证

### 自动化测试

`scripts/test-models.mjs` — 15 项全过：

| 场景 | 验证项 |
|---|---|
| 存量纯字符串 models 往返 | 能读、写回仍是字符串数组、没变成内联表 |
| 带元数据的 models 读写 | 能读出来 |
| 元数据下发 | provider 写进 pi、contextWindow/maxTokens/input 用母版值、推理档位写进 thinkingLevelMap |
| 母版留空 | agent 里已有的 contextWindow/maxTokens **不被覆盖** |
| backfill | 有输出、手填的 999999 被保留、第二个模型被回填 |

`scripts/test-provider-matrix.mjs` — 10 项全过（重构没改行为）
`scripts/test-mcp-matrix.mjs` — 全部符合预期

### 契约与类型

- `check-contract.mjs`：27 个类型、151 个字段一致
- `svelte-check`：0 error 0 warning

### 真实数据

`agentcenter models backfill --dry-run` 在真实配置副本上跑通，
补 81 个字段。抓到的真实差异：

- wangxin 是 `32768 / 8192`（不是 1M/128K）
- opencode-go 是 `1048576 / 131072` 且带 cost
- `mimo-v2.5` 的推理档位在用户配置里是 `["enabled","off"]`（不是标准档位名）

### 界面

浏览器实测：

- Provider 页默认无模型编辑器
- 点「编辑」后出现「模型（2）」+「+ 添加模型」
- 摘要行 `1M ctx · 128K out · text · reasoning`
- 展开后有 1 个 checkbox、4 个模态 chip、6 个档位 chip、2 个数字输入框
- 分配面板 7 张卡片，label 用后端展示名
- DSH 因配置文件不存在灰掉标「未安装」

## 踩到的坑

1. **`ModelSpec` 的字段名被子母版和前端各要一套**
   `rename_all = "camelCase"` 让手写的 `context_window` 读不进来，
   静默变成 `None`，注入时用默认值。加 `alias` 让两种写法都能读。

2. **`supports_provider` 和 `provider_ready` 必须分开**
   用后者校验分配关系，会把「配给未安装 agent」的值静默丢掉。

3. **`mcp/inject.rs` 的注入器 struct 没有公开 `plan`**
   `McpFilePlan::plan` 是 trait 方法，直接 `CodexMcpInjector.plan(...)`
   编译不过。加 `plan_codex()` / `plan_opencode()` / `plan_workbuddy()`
   三个包装函数。

4. **`toml::to_string_pretty` 丢头部注释**
   保存前把原文件第一个 `[[provider]]` 之前的内容原样接回去。
   表后面的注释不保留（位置和具体 provider 绑定，重排后会错位）。

5. **测试脚本的 `execFileSync` 失败时静默返回空串**
   白的 false failure 查了很久。改成 `try/catch` 同时返回 stdout 和 stderr。

## 已知限制

- DSH 的 `cordis.patch.yml` 实际是 patch-entry 数组，`providers:` 缩进 4
  而不是 0。现有注入器按缩进 0 找，写的结构也和真实语义不同。未在本轮修复。
- opencode 的模型定义以它的模型目录为准，我们只写 `name`。
- WorkBuddy 只有 `supportsImages` 一个布尔，模态列表靠反推，
  写不出 `video` / `pdf` 的区别。
- 模型级只增改不删。
- **Cursor 的 provider 管不了**。`~/.cursor/cli-config.json` 里没有任何
  base_url / api_key 字段；`%APPDATA%\Cursor\User\globalStorage\state.vscdb`
  的 `aiSettings.modelConfig` 只有 `modelId` / `maxMode` 这类选择偏好。
  它的模型走 Cursor 自己的服务端订阅，没有可写的第三方端点配置。
- **从母版彻底删掉一条 MCP server 时，agent 配置里的残留不会被清理**。
  注入器只能看到「当前池子里有、但没分配给我」的 id；池子里没有的 id
  一律不动（这是为了不误删用户手配的）。要清理请先在界面上取消分配、
  应用一次，再从池子里删。

---

## 第二轮补充：Hermes 与 ZCode MCP

用户指出「provider 管理缺少对 cursor、Hermes 的支持」「mcp 管理缺少对
zcode、Hermes 的支持」。逐个核实后的结论：

| agent | provider | MCP | 结论 |
|---|---|---|---|
| Hermes | ✅ `$HERMES_HOME/config.yaml` 的 `providers` | ✅ 同文件 `mcp_servers` | 两项都加上了 |
| ZCode | ✅ 已支持 | ✅ `~/.zcode/cli/config.json` 的 `mcp.servers` | 补上了 MCP |
| Cursor | ❌ 无端点配置 | ✅ 已支持 | provider 确实管不了 |

### ZCode 的 MCP 位置之前判断错了

原注释写着「未探明（`cli/config.json` 只有插件开关）」。当时只查了
`v2/config.json`（只有 `provider`）和 `cli/config.json`（本机只有 `plugins`），
没找到就下了结论。实际 ZCode 官方文档明确写的是 `mcp.servers`。

三处易错点：

- 键路径是 `mcp.servers.<name>`（两层），不是顶层 `mcpServers`
- `.agents/mcp.json` 是**同 scope 的回退**，只在 `.zcode` 里没有 MCP 时才读
- 文件 schema 严格：多一个未知键整个 server 被丢弃；`command` 必须是字符串，
  不能学 opencode 写数组

### Hermes 的路径要读环境变量

「Hermes」在本机对应两个东西：

1. `~/.hermes/` —— 空目录，只有两个备份脚本
2. `C:\dev\ide\hermes-agent\Hermes Agent CN Desktop` —— 真正在用的桌面版，
   `HERMES_HOME` 指向它的 `data\hermes-home`

所以 `Hermes::home()` 优先读 `HERMES_HOME`，取不到才回落到 `~/.hermes`。
这和 `AGENTCENTER_HOME` 是两套独立的覆盖机制，测试时两个都要设。

Hermes 的 `config.yaml` 结构：

```yaml
model:
  provider: opencode-go          # 当前在用
  default: deepseek-v4-flash
providers:
  opencode-go:
    name: OpenCode Go
    base_url: https://opencode.ai/zen/go/v1
    api_mode: chat_completions
    transport: openai_chat
    model: deepseek-v4-flash
    models:
      glm-5.2:
        context_length: 204800
        supports_tools: true
    api_key: sk-xxx
mcp_servers:
  time:
    command: uvx
    args: ["mcp-server-time"]
```

字段映射：

| 本工具 | Hermes |
|---|---|
| `anthropic` / `openai-chat` / `google` | `api_mode: chat_completions` |
| `openai-responses` | `api_mode: responses` |
| `transport`（固定） | `openai_chat` |
| `ModelSpec.context_window` | `models.<id>.context_length` |
| `ModelSpec.tool_call` | `models.<id>.supports_tools` |
| `transport: stdio` | `command` + `args` + `env` |
| `transport: http` / `sse` | `url` + `headers` |

`model.provider` 只在它指向一个**已经被移除**的 provider 时才纠正 ——
那是用户当前在用的选择，主动替用户切换是越权。

### 两个注入器都做行级 YAML 编辑

Hermes 的 `config.yaml` 有 40 行注释（安全说明、fallback 模型清单）
和 `skills.disabled` 这种 39 项长数组。任何 YAML 库「解析成结构体再
序列化」都会把这些全丢掉。

所以只动目标行：找 `providers:` / `mcp_servers:` 段的边界，在段内做
插入、删除、替换。测试里专门验证了「原有注释保留」和
「原有 skills.disabled 保留」两项。

### 测试

`scripts/test-hermes-zcode.mjs` — 20 项全过：

| 场景 | 验证项 |
|---|---|
| Hermes provider | 写入、base_url、api_mode、模型、context_length、注释保留、skills 保留、原有 provider 保留 |
| Hermes MCP | mcp_servers 段创建、stdio 写入、args 数组格式、remote url |
| ZCode MCP | plugins 段保留、mcp.servers 两层结构、command 是字符串、args 是数组 |
| 取消分配 | 条目移除、新 server 写入、plugins 仍保留 |

回归：`test-provider-matrix` 10 项、`test-models` 15 项、`test-mcp-matrix` 全部通过。

---

## 第三轮：协议从 provider 级挪到 provider × agent 级

用户指出：「API 的格式是不是不应该放在 provider 层，而是放在具体的 agent 层。
现在 provider 中显示 command 是 anthropic 格式，但我在 zcode 中使用的是
openai response 格式」。

**判断完全正确，而且比这更严重。**

### 实测证据

| 端点 | 母版写的 | ZCode 实际 | Pi 实际 |
|---|---|---|---|
| `api.commandcode.ai/provider/v1` | `anthropic` | `openai-compatible` | `openai-completions` |
| `opencode.ai/zen/go` | `openai-chat` | **`anthropic`** | **`openai-responses`** |

`opencode-go` 这一行是关键证据：**同一个端点，同一份机器上，
ZCode 用 anthropic、Pi 用 openai-responses**。协议根本不是 provider 的属性。

### 为什么原来是错的

`Provider.kind` 只有一个值，8 个注入器各自读它再映射到自己的词表。
所以一个 provider 只能有一种协议，必然错一半。

### 改法

`Provider` 加 `agent_kinds`（`BTreeMap<String, ProviderKind>`）：

```toml
[[provider]]
id = "multi"
kind = "anthropic"          # 默认值，agent_kinds 里没写的 agent 用它
agents = ["zcode", "pi"]

[provider.agent_kinds]
pi = "openai-responses"     # 只有 Pi 用另一种
```

- `Provider::kind_for(agent)` 先查 `agent_kinds`，没有就回落到 `kind`
- 8 个注入器全部从 `p.kind` 改成 `p.kind_for("<agent-id>")`
- `kind` 保留为默认值：新增 provider 时不用逐个 agent 填

### 存量母版不用迁移

只写 `kind` 的存量文件行为完全不变 —— `agent_kinds` 是空的，
`kind_for()` 恒返回 `kind`。要精细控制才加覆盖。

### 界面

provider 编辑表单里，「协议」拆成两层：

```
默认协议  [Anthropic Messages        ]  （未单独指定的 agent 用它）

各 agent 实际用的协议
  Claude Code  [Anthropic Messages  ]  默认
  pi           [OpenAI Responses    ]  恢复默认
  ZCode        [Anthropic Messages  ]  默认
```

选中默认值时删掉覆盖项，母版保持简洁。列表行上如果存在覆盖，
显示一个「N 个 agent 用别的协议」的黄色徽标，hover 看明细。

### 回填会报告不一致

`agentcenter models backfill --dry-run` 现在多一段输出：

```
[注意] 这些 agent 里实际用的协议和母版不同：
  opencode-go / zcode：母版 openai-chat → 实际 anthropic
  minimax-cn / zcode：母版 openai-chat → 实际 anthropic
  command / zcode：母版 anthropic → 实际 openai-chat
  command / pi：母版 anthropic → 实际 openai-chat
  command / workbuddy：母版 anthropic → 实际 openai-chat
```

**只报告，不自动回填。** 理由：扫描到的值只对那一个 agent 成立，
不能拿来覆盖 provider 级默认值。但用户需要知道 —— 母版和实际不一致时，
注入会按母版写回去，可能把本来能用的配置改坏。

真实数据上跑出的 5 处不一致里，`opencode-go` 和 `minimax-cn` 是
**反方向**的（母版写 openai-chat，ZCode 里实际是 anthropic），
说明母版里的 `kind` 从一开始就没跟实际配置对齐。

### 扫描器补了 `api_kind`

`FoundProvider` 加 `api_kind: Option<ProviderKind>`，各 agent 按自己的
词表读：

| agent | 读哪里 |
|---|---|
| ZCode | `provider.<id>.kind` |
| Pi | `providers.<id>.api` |
| opencode | `providers.<id>.package`（含 anthropic 就是 Anthropic） |
| Codex | `model_providers.<id>.wire_api` |
| Hermes | `providers.<id>.api_mode` |
| DSH | `providers.<id>.api` |
| WorkBuddy | `useCustomProtocol` 反推 |
| Claude | 恒为 Anthropic（只有 `ANTHROPIC_*` 一套） |

### 测试

`scripts/test-agent-kinds.mjs` — 9 项全过：

| 场景 | 验证项 |
|---|---|
| 同一 provider 不同协议 | ZCode 拿到 anthropic、Pi 拿到 openai-responses |
| 母版往返 | `agent_kinds` 保留 |
| 去掉覆盖 | Pi 回到默认的 anthropic-messages |
| 扫描回读 | backfill 报告「母版 anthropic → 实际 openai-chat」 |

### 顺手修的测试隔离问题

Hermes 的配置目录读 `HERMES_HOME`，`AGENTCENTER_HOME` 管不到它。
`test-provider-matrix.mjs` 之前会去读真实的
`C:\dev\ide\hermes-agent\...\config.yaml`（只是「跳过无变化」，
不写盘，但也不该碰）。三个测试脚本现在都显式设了
`HERMES_HOME: join(TEST, 'hermes-not-installed')`。

---

## 第四轮：界面两处修正

用户反馈两点：

1. 「在 provider 层应该只显示和配置支持的协议」
2. 「在 provider 下看不到对应的模型列表」

### 第二点：模型列表被上一轮改掉了

上一轮加「N 个 agent 用别的协议」徽标时，把原来那行「N 个模型」
连同位置一起替换掉了。现在改成直接列模型名：

```
command   Anthropic Messages   [1 个 agent 用别的协议]
https://api.commandcode.ai/provider/v1
deepseek/deepseek-v4.1-flash  xiaomi/mimo-v2.5  2 个模型
```

最多显示 4 个，超出的折叠成 `+N`（hover 看完整列表）。每个标签的
`title` 是能力摘要。

### 第一点：协议要按 agent 的能力过滤

原来每个 agent 的下拉里都是完整四种，但 Codex 只接受 `responses`、
Claude 只认 `anthropic`。选了做不到的协议会被注入器静默改写 ——
等于骗用户。

`Agent` trait 加 `supported_kinds()`：

```rust
fn supported_kinds(&self) -> &'static [ProviderKind] {
    &[]   // 默认空 = 支持全部四种
}
```

只声明受限的两个：

| agent | supported_kinds | 原因 |
|---|---|---|
| Codex | `[OpenaiResponses]` | `wire_api` 只接受 responses，其他会被强制改写 |
| Claude | `[Anthropic]` | 只有 `ANTHROPIC_*` 一套环境变量 |
| 其余 13 个 | `[]`（全部） | 走 OpenAI 兼容层，四种都能表达 |

界面上，只有一种选择的 agent 显示成**不可编辑的文字**并标「固定」，
不给下拉 —— 免得看着像能选：

```
各 agent 实际用的协议
  Claude Code   Anthropic Messages                    固定
  Codex CLI     OpenAI Responses                      固定
  pi            [Anthropic Messages ▾]                 默认
  opencode      [Anthropic Messages ▾]                 默认
```

另外，这一组下拉原来只在 `editing` 存在且 `editing.agents` 非空时才渲染，
所以**新增 provider 时完全看不到**。现在改成列所有支持 provider 的
agent，未分配的灰掉并注明「协议可以预先设好，分配后生效」。

### 测试

`scripts/test-protocol-e2e.mjs` — 11 项全过，验证「应用里改设置 →
agent 配置真的变」这条完整链路：

| 场景 | 验证项 |
|---|---|
| 不设覆盖 | ZCode/Pi/Hermes 三家都用默认协议 |
| 只给 Pi 设 responses | Pi 变了，ZCode 和 Hermes 没变 |
| 给 ZCode 设 openai-chat | ZCode 写成 openai-compatible |
| CLI `--agent-kind` | 写进母版，注入后 Hermes 变 responses |
| 给未分配的 agent 设协议 | 不写入该 agent，但母版保留设置 |
