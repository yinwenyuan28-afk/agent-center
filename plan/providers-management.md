# Provider 统一管理与注入

## 背景与目标

现在 agentcenter 只有 API key 的**只读体检**。这一步要往前走到真正改写配置文件。

两个目标：

1. **统一管理所有 provider**：每个 provider 只保留一个唯一的 API key 来源
2. **管理各 agent 里的 provider**：能删除、能新增（把统一管理的 provider 注入进去）

风险等级和只读体检完全不同——写错会让某个 agent 登录失败。

实测现状（2026-11-10，`agentcenter creds inspect`）：

- 19 条凭证，16 个不同 key
- 2 组重复：CommandCode 那把在 claude/zcode/pi 三处；MiniMax CN 那把在 zcode/pi 两处
- 2 个 provider 配了多把 key：`command`、`deepseek`
- 其中 `deepseek` 的 pi `auth.json`（35 字符）与环境变量 `DEEPSEEK_API_KEY`（35 字符）是**两把不同的 key**

---

## 各 agent 的 provider 结构（实测，字段级）

文件和键的现行记录在 [provider-config-keys.md](provider-config-keys.md)。下面这张表是 2026-11-10 的实测快照，后来 ZCode 又多了 `provider_config.json`，以那份记录为准。

| agent | 配置文件 | 多 provider 并存 | key 落在哪 | 当前选中靠什么 |
|---|---|---|---|---|
| **ZCode** | `~/.zcode/v2/config.json` | 支持，`provider` map 平铺 | `provider.<uuid>.options.apiKey` 明文 | 任务级，`tasks-index.sqlite` 的 `tasks.model` |
| **Pi** | `~/.pi/agent/auth.json` + `models.json` | 支持 | `auth.json.<id>.key` 明文 | `settings.json` 的 `defaultProvider` + `defaultModel` |
| **opencode** | `~/.config/opencode/opencode.json` + `~/.local/share/opencode/auth.json` | 支持 | `auth.json.<id>.key` 明文 | 顶层 `model`（`providerID/modelID`） |
| **Codex** | `~/.codex/config.toml` | 支持（`model_providers` + `profiles`） | **不落盘**，`env_key` 指向环境变量 | 顶层 `model_provider` |
| **Claude Code** | `~/.claude/settings.json` | **不支持** | `env.ANTHROPIC_AUTH_TOKEN` | 就是那组 env 变量 |

### 三个关键限制

1. **Claude 没有 provider 列表概念**。它只有一组 `ANTHROPIC_*` 环境变量决定当前端点，`/model` 切的是模型不是服务商。所以只能「把当前选中的 provider 展开成一组 env」。
2. **Codex 的 key 不进配置文件**。`[model_providers.<id>]` 只有 `env_key` 字段存环境变量名，key 本身必须从进程环境读。
3. **写配置前必须先退出 ZCode / Pi / opencode**。它们运行时会用内存态覆盖文件。

### ZCode 的 provider 条目结构

```jsonc
"provider": {
  "<uuid-v4 或 builtin:模板名>": {
    "name": "显示名",
    "kind": "anthropic" | "openai-compatible",
    "options": {
      "apiKey": "明文",
      "baseURL": "https://...",       // 注意是大写 URL
      "apiKeyRequired": true           // 可选
    },
    "source": "custom",
    "enabled": false,                  // 可选，缺省即启用
    "models": {
      "<modelId>": {
        "limit": { "context": 1000000, "output": 128000 },
        "modalities": { "input": ["text"], "output": ["text"] },
        "reasoning": { "enabled": true, "variants": [...], "defaultVariant": "max" },  // 可选
        "zcode": { "modified": true, "modalitiesConfigured": true }                    // 可选
      }
    }
  }
}
```

`models` 不能为空，否则该 provider 在 ZCode 界面里无模型可选。

### Pi 的两个文件

`auth.json`：
```jsonc
{ "<providerId>": { "type": "api_key", "key": "明文" } }
```

`models.json`：
```jsonc
{
  "providers": {
    "<providerId>": {
      "name": "显示名",
      "baseUrl": "https://...",
      "api": "openai-completions" | "openai-responses" | "anthropic-messages" | "google-generative-ai",
      "models": [
        {
          "id": "发给上游的 model id",
          "name": "显示名",
          "reasoning": true,
          "input": ["text", "image"],
          "contextWindow": 1000000,
          "maxTokens": 131072
        }
      ]
    }
  }
}
```

三处用同一个 provider id。`settings.json.defaultModel` 填**裸 id**，不要带 provider 前缀（只有 `visionFallback.model` 才是 `provider/model` 形式）。

### opencode 的两个文件

`opencode.json`：
```jsonc
{
  "providers": {
    "<providerId>": {
      "name": "显示名",
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": { "baseURL": "https://..." },
      "models": { "<modelID>": { "name": "显示名" } }
    }
  },
  "model": "<providerId>/<modelID>"
}
```

`auth.json`（在 `~/.local/share/opencode/`，注意不是 `~/.config/opencode/`）：
```jsonc
{ "<providerId>": { "type": "api", "key": "明文" } }
```

**注意 Pi 的 type 是 `"api_key"`，opencode 的是 `"api"`**，不能跨家复用。

### Codex 的 model_providers

```toml
model_provider = "<id>"      # 顶层，必须写在任何 [table] 之前
model = "<model-id>"

[model_providers.<id>]
name = "显示名"
base_url = "https://..."
env_key = "COMMAND_API_KEY"   # 环境变量名，key 本身不写这
wire_api = "responses"        # 现版本唯一合法值
requires_openai_auth = false
```

schema 是 `additionalProperties: false`，写错键名直接解析失败。

### Claude 的 env

```jsonc
"env": {
  "ANTHROPIC_BASE_URL": "https://...",
  "ANTHROPIC_AUTH_TOKEN": "明文",     // 或 ANTHROPIC_API_KEY，二选一
  "ANTHROPIC_DEFAULT_OPUS_MODEL": "...",
  "ANTHROPIC_DEFAULT_SONNET_MODEL": "...",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL": "..."
}
```

---

## 已确认的三个决定

### 1. Claude 只注入当前选中的那个

符合它本身的能力，不造出做不到的状态。界面上 Claude 栏显示「当前指向 xxx」，切换 provider 会重写那组 env。

想要 Claude 也多 provider 并存，只能靠外部手段（`claude --settings <file>` 或不同 `CLAUDE_CONFIG_DIR`），这不在 `settings.json` 的 schema 里，本项目不做。

### 2. Codex 用 env_key + 生成环境变量文件

同步时额外生成 `~/.agents/codex-env.ps1`：

```powershell
$env:COMMAND_API_KEY = '...'
```

用户在使用 Codex 的终端里需要先 `. ~/.agents/codex-env.ps1`。

不选 `experimental_bearer_token` 的原因：官方标注不推荐，且字段名带 experimental，未来可能移除。

### 3. 现有重复 key 自动以最新为准

按文件 mtime 判断。**这是风险最高的一条**，实现时必须做到：

- 能从文件 mtime 判断的取最新
- 判断不了的（环境变量无时间戳）不盲选，列进「需要人工确认」清单
- 无论哪种情况，写入前完整备份

---

## 母版设计

`~/.agents/providers.toml`，**必须加入 `.gitignore`**（同目录放 `providers.example.toml` 供参考）。

```toml
[[provider]]
id = "command"
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"
kind = "anthropic"              # anthropic / openai-compatible
api_key = "sk-..."              # 唯一来源
models = ["deepseek/deepseek-v4.1-flash"]
agents = ["claude", "zcode", "pi"]
```

`agents` 字段声明这个 provider 要注入到哪些 agent。未列出的 agent 不会被改动。

### 为什么用 TOML 而不是 JSON

- 可写注释，能记录"这个 key 是哪天从哪拿的"
- 比 JSON 少一半引号和逗号
- 项目已经有 `toml` 依赖

---

## 五个注入器

各 agent 结构差异太大，不能用一个通用逻辑套。每个实现统一 trait：

```rust
pub trait Injector {
    /// agent 标识
    fn agent(&self) -> &'static str;
    /// 涉及的所有文件（有些 agent 要改两个）
    fn targets(&self) -> Vec<PathBuf>;
    /// 计算差异，不写盘
    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>>;
}
```

| 注入器 | 目标文件 | 策略 |
|---|---|---|
| `zcode` | `v2/config.json` | 写完整 provider 对象到 `provider` map，含 `models` 子结构 |
| `pi` | `auth.json` + `models.json` | key 进 auth，provider 定义进 models.providers |
| `opencode` | `opencode.json` + `auth.json` | provider 定义进 `providers`，key 进 `auth.json` |
| `codex` | `config.toml` + 生成 `codex-env.ps1` | 只写 `env_key` 引用和 `model_providers` 表 |
| `claude` | `settings.json` 的 `env` | 只注入当前选中 provider，展开成 `ANTHROPIC_*` 组 |

### 字段映射表

| 统一字段 | ZCode | Pi | opencode | Codex | Claude |
|---|---|---|---|---|---|
| id | `provider` 的键 | auth/models 的键 | providers/auth 的键 | `[model_providers.<id>]` 表名 | 无 |
| baseURL | `options.baseURL` | `providers.<id>.baseUrl` | `providers.<id>.settings.baseURL` | `base_url` | `env.ANTHROPIC_BASE_URL` |
| apiKey | `options.apiKey` | `auth.json.<id>.key` | `auth.json.<id>.key` | 不写，只写 `env_key` | `env.ANTHROPIC_AUTH_TOKEN` |
| 协议 | `kind` | `api` | `package` | `wire_api` | 固定 Anthropic |
| 模型 | `models` map | `models` 数组 | `models` map | 无列表 | 4 个别名槽位 |

---

## 五道安全闸

### 为什么必须这么做

CC Switch v4 之前用「全量重写」，出过这些事故：

- `statusLine`、hooks、plugins、MCP 配置全丢（issue #480 定位到根因：**只序列化了它已知的字段**）
- 更隐蔽的：切走时把整份文件存进供应商快照，导致共享设置被「冻结」在某一家供应商上，切到别家就丢
- 用户靠 git 版本控制才救回配置

v4 改成「关键字段替换 + 所有权清单」，才解决。

### 1. 所有权清单

每个 agent 声明「哪些键归我管」，其余一律不碰。

- Claude 只接管 `ANTHROPIC_*`、`AWS_*`、`VERTEX_REGION_*` 前缀
- 但**不能滥用前缀匹配**：`CLAUDE_CODE_USE_` 下有 `USE_POWERSHELL_TOOL` 这种功能开关，不是连接配置
- Codex 只清点名的键（如 `model_provider`、`model`），`model_providers.<id>` 表整体归我管，表里其他键不动

**失败方向必须是安全的**：漏掉的键按用户键保留，而不是被删。

### 2. 解析失败即全停

任何一个目标文件解析失败，整次操作不写任何文件，报错带文件路径和行列号。

**绝不退回空文档再覆盖**。这是 agent-config-sync 的反面教材——它的 `readConfig` 吞掉 parse 错误返回 `{}`，于是一个语法损坏的配置文件会被当成空对象然后整体覆盖。

### 3. hash 复查 + 有限重试

计划时记录每个文件的 hash，写入前重读比对：

- 没变 → 继续
- 变了 → 以新内容为底重新规划，最多 3 次
- 超过 3 次 → 报冲突并中止

**不能用 mtime**：精度、网络文件系统、时钟回拨都不可靠。

### 4. 原子写入

同目录临时文件 → fsync → rename。

Windows 上：
- 优先 `ReplaceFileW`
- 失败且错误为 `NotFound` / `ERROR_NOT_SUPPORTED` 时退回 `fs::rename`
- 写入前解析符号链接的最终目标（最多 40 跳），避免把 symlink 换成普通文件

这条是为了防 CC Switch 4.0.27 那个 bug：把临时文件建在链接旁再 rename 到链接路径，导致 stow / chezmoi 管理的配置文件从符号链接变成普通文件。

### 5. 首写备份

每个文件在**第一次被本工具写入之前**完整复制到 `~/.agents/backup/providers-<时间戳>/`，并用 marker 去重（同一次会话不重复备份）。

备份是**全量文件复制**，不做增量——配置文件才几 KB，省不出什么，却引入重建复杂度。

---

## 格式保留

### JSON

用 `serde_json` 开 `preserve_order`。

**删除一律用 `shift_remove`**：开了 `preserve_order` 后 `Map::remove` 变成 `swap_remove`，会把最后一个键挪到被删位置，打乱顺序。

替换一律原位改值，新增的键追加到所在对象末尾。

### TOML

新增依赖 `toml_edit = "0.25"`（它自述 "Yet another format-preserving TOML parser"）。

**关键陷阱：直接赋值会丢注释。**

```rust
doc["foo"] = toml_edit::value("bar")   // 行尾注释消失
```

正确做法是先克隆新值，再把旧值的 decor 搬过去：

```rust
let decor = slot.decor().clone();
*slot = value.clone();
*slot.decor_mut() = decor;
```

换表时还要额外搬 `position()`，否则表会跑到文件末尾：

```rust
*table.decor_mut() = old.decor().clone();
if let Some(position) = old.position() {
    table.set_position(position);
}
```

### 行尾风格

`toml_edit` **不保留 CRLF**（issue #1213）。需要自己检测原文件是否含 `\r\n`，序列化后恢复；末尾换行同理（`toml_edit` 会补一个 `\n`）。

---

## 实施阶段

| 阶段 | 内容 | 验收 |
|---|---|---|
| 1 | `providers.toml` 读写 + 类型定义 | 能读写母版；`npm run verify` 全绿 |
| 2 | 只读差异对比 | `agentcenter providers diff` 输出差异，**不写盘** |
| 3 | 安全写入骨架 + ZCode 注入器 | 新增/删除 provider 后 ZCode 能用；连续同步两次零改动 |
| 4 | Pi / opencode 注入器 | 同上，两文件都正确 |
| 5 | Claude / Codex 注入器 | 含生成的 `codex-env.ps1`；Claude 只注入选中项 |
| 6 | 界面：Provider 管理页 | 增删改 + 同步预览 + 冲突处理全流程可用 |

---

## 实施完成记录（2026-11-10）

六个阶段全部完成并验证。

### 代码结构

```
src-tauri/src/providers/
  mod.rs         Injector trait + sync 调度（先全 plan 后全写）
  model.rs       Provider / FilePlan / DiffLine / SyncReport
  store.rs       providers.toml 读写 + gitignore 检查 + 测试用 home 重定向
  safe_write.rs  五道安全闸
  scan.rs        从各 agent 现有配置读 provider（只读）
  diff.rs        母版 vs 现状对比
  matrix.rs      provider 池 × agent 分配矩阵（含 Claude 单槽位互斥）
  zcode.rs       ZCode 注入器
  pi.rs          Pi 注入器（auth.json + models.json）
  opencode.rs    opencode 注入器（opencode.json + auth.json）
  codex.rs       Codex 注入器（config.toml + codex-env.ps1）
  claude.rs      Claude 注入器（展开成 ANTHROPIC_* 组）
```

CLI：`agentcenter providers {list,matrix,assign,diff,sync,set,remove,example,doctor}`
界面：`src/pages/Providers.svelte`（单页：池 + 分配 + 应用到各 agent）

### 验证结果

**副本测试（用 `AGENTCENTER_HOME` 重定向，不碰真实配置）**

| 验证项 | 结果 |
|---|---|
| ZCode 新增 provider | ✅ 9 → 10 个，顶层键不变，原有 models 未动 |
| ZCode 幂等 | ✅ 第二次全部跳过 |
| 解析失败中止 | ✅ 写入坏 JSON 后同步报错，文件保持原样不被"修复"成空文档 |
| Pi 更新 baseUrl/api | ✅ auth.json 跳过（key 已一致），models.json 更新 |
| opencode 新增 | ✅ 两文件都新增，顶层键完整 |
| Pi/opencode 幂等 | ✅ 第二次 4 个文件全跳过 |
| Claude 单槽位 | ✅ 只注入第一个 provider，未误注入第二个 |
| Claude 保留无关键 | ✅ `includeCoAuthoredBy`、`model`、`JAVA_HOME` 全部保留 |
| Codex 保留原有表 | ✅ `mcp_servers`/`projects`/`plugins`/`desktop` 全在 |
| Codex 生成 env | ✅ `codex-env.ps1`，变量名正确 |
| **TOML 注释保留** | ✅ 真正触发写入后 3 行注释全部保留 |
| Codex 幂等 | ✅ 第二次全部跳过 |

**真实数据验证**

- `providers diff` 对真实配置跑通：三个 agent 的 command provider 判定「已一致」，其余手动配的 provider 标「保持原样不动」
- 契约检查：15 个类型 84 个字段一致
- 前端构建产物 141.96 kB（gzip 45.86 kB）

### 实现中踩到并修掉的坑

1. **`dirs::home_dir()` 在 Windows 不读 `USERPROFILE`**
   它走 `SHGetKnownFolderPath`，改环境变量是无效的。所以加了 `AGENTCENTER_HOME` 作为测试专用重定向，所有路径解析统一走 `store::home_dir()`。

2. **结构体字面量里 move 后不能再借用**
   `AgentScan { providers, unsupported_reason: if providers.is_empty() ... }` 编译失败。先算好结论再构造。

3. **`Vec::dedup()` 只去相邻重复**
   统计「一个 provider 有几把 key」时用它得出错误的 0，改用 `BTreeSet`。

4. **`toml_edit` 不保留 CRLF**
   自己检测原文件行尾风格和末尾换行，序列化后恢复。

5. **`toml_edit` 直接赋值会丢注释**
   本实现用 `Table` 的字段级 `insert`，不是整表替换，所以注释天然保留（已验证）。

6. **CMD 的 `echo` 写文件是 GBK**
   测试脚本里用 `type`/`echo` 生成的配置文件不是 UTF-8，导致同步报「stream did not contain valid UTF-8」。改用 Node 写文件。

### 当前边界

- Codex 的 provider key 走环境变量，用户需在使用前 `. ~/.agents/codex-env.ps1`
- Claude 只支持单 provider，界面上勾选时会自动从其他 provider 上摘掉 claude
- 不碰 ZCode 的 `credentials.json`（`enc:v1:` 加密）和 `auth.json`（ChatGPT 登录态）
- 母版里没有的 provider 一律不动 —— 用户手动配的不会被抹掉
- 移除只针对「母版里有、但没分配给本 agent」的条目，母版之外的永远不碰
- 没有「从现有配置导入」：扫描只能拿到 key 指纹，拿不到明文，
  硬导入会生成占位 key 的假条目。要纳管已有 provider 请手工新增或用 `providers set`

### 分配矩阵的验证

`scripts/test-provider-matrix.mjs` 在副本上跑 10 项，全通过：

| 验证项 | 结果 |
|---|---|
| 空母版时 `providers list` 不报错 | ✅ |
| 分配的 provider 写进 zcode | ✅ |
| 未分配的 provider 不写进 zcode | ✅ |
| Claude 拿到分配给它的 provider | ✅ |
| 池内条目不会塞进未分配的 codex | ✅ |
| 取消分配后条目从 zcode 移除 | ✅ |
| zcode 原有的 9 个 provider 全部保留 | ✅ |
| 第二次同步零写入（幂等） | ✅ |
| claude 取消分配后清掉 `ANTHROPIC_BASE_URL` | ✅ |
| claude 的非接管键保留 | ✅ |

Claude 单槽位互斥另外用 CLI 双向验证：

```
providers assign beta --agents claude   # alpha 的 claude 自动消失
providers assign alpha --agents claude,pi  # beta 的 claude 自动消失
```

### 实现中踩到并修掉的坑

1. **`dirs::home_dir()` 在 Windows 不读 `USERPROFILE`**
   它走 `SHGetKnownFolderPath`，改环境变量是无效的。所以加了 `AGENTCENTER_HOME` 作为测试专用重定向，所有路径解析统一走 `store::home_dir()`。

2. **结构体字面量里 move 后不能再借用**
   `AgentScan { providers, unsupported_reason: if providers.is_empty() ... }` 编译失败。先算好结论再构造。

3. **`Vec::dedup()` 只去相邻重复**
   统计「一个 provider 有几把 key」时用它得出错误的 0，改用 `BTreeSet`。

4. **`toml_edit` 不保留 CRLF**
   自己检测原文件行尾风格和末尾换行，序列化后恢复。

5. **`toml_edit` 直接赋值会丢注释**
   本实现用 `Table` 的字段级 `insert`，不是整表替换，所以注释天然保留（已验证）。

6. **CMD 的 `echo` 写文件是 GBK**
   测试脚本里用 `type`/`echo` 生成的配置文件不是 UTF-8，导致同步报「stream did not contain valid UTF-8」。改用 Node 写文件。

7. **Codex 在没有 provider 分配时仍会写文件**
   `model_providers` 表不存在时无条件建空表，让「无改动」变成「写入」，
   还会生成一个空的 `codex-env.ps1`。改成：没有要分配的 provider 就不建表、
   不建脚本；已存在的脚本要清成只有注释（留着旧 key 会让 Codex 连错端点）。

8. **`&str` 数组的 `contains` 借用不匹配**
   `unassigned_urls.contains(&normalize_url(...))` 里 `Vec<&str>` 的元素类型是 `&&str`，
   写成 `contains(&&...)` 才编译。另外 `for key in OWNED_KEYS` 拿到的是 `&&str`，
   `env.remove` 需要 `&str`，要写 `OWNED_KEYS.iter()` + `remove(*key)`。

**新增（前端）**

- `src/pages/Providers.svelte` —— 单页：provider 池 + 展开分配 + 应用到各 agent
  （原来的 `ProviderSync.svelte` 已删除，差异对比与注入都收进这一页）

**新增（测试）**

- `scripts/test-provider-matrix.mjs` —— 分配矩阵与移除语义的端到端验证

**修改**

- `src-tauri/Cargo.toml` —— 加 `toml_edit = "0.25"`、`serde_json` 开 `preserve_order`
- `src-tauri/src/types.rs` + `src/lib/types.ts` —— 新增 `ProviderMatrix`，跑 `check:contract`
- `src-tauri/src/cli.rs` / `cli_run.rs` —— 加 `providers` 子命令组（含 `matrix` / `assign`）
- `src/App.svelte` —— 侧边栏从 7 项收到 6 项
- `~/.agents/.gitignore` —— 加 `providers.toml`

---

## 验证方式

### 不动真实配置的验证

- `agentcenter providers diff` 跑一遍，人工核对报告是否准确
- 在 `C:\temp\provider-test\` 下复制一份真实配置文件，对副本跑注入器，`diff` 原文件确认只有目标键变化

### 真实写入的验证（按顺序）

1. 先只对 ZCode 一个 agent 试，写入后打开 ZCode 确认能看到新 provider、能正常对话
2. 连续同步两次，第二次必须零改动（幂等）
3. 手动改一下 `settings.json` 里的 `permissions`，再同步，确认 `permissions` 没被动
4. 删除一个 provider，确认它从所有 agent 里消失，且其他 provider 不受影响

### 边界验证

- 故意写一个语法错误的 `config.toml`，确认同步被拒绝且文件原样未动
- 在 agent 运行时执行同步，确认有明确提示要求先退出

---

## 明确的边界

### 能管理的 agent（13 个）

字段级细节见 [provider-config-keys.md](provider-config-keys.md)。这里只列概览。

| agent | provider 配置位置 | key 位置 | 多 provider 并存 |
|---|---|---|---|
| ZCode | `~/.zcode/v2/config.json` + `provider_config.json` | 同文件 `options.apiKey` | 支持 |
| Pi | `~/.pi/agent/models.json` | 同文件 `providers.<id>.apiKey` | 支持 |
| opencode | `~/.config/opencode/opencode.jsonc`（没有则 `.json`） | 条目内 `options.apiKey` / `settings.apiKey` | 支持 |
| Codex | `~/.codex/config.toml` 的 `[model_providers.custom]` | 表内 `experimental_bearer_token` | **不支持**，只有单槽位 |
| **WorkBuddy** | `~/.workbuddy/models.json`（扁平数组） | 同文件 `apiKey` | 支持 |
| **DSH** | `~/.dsh/profiles/{web,desktop,headless,acp}/cordis.patch.yml` | `~/.dsh/.credentials.yaml` 的 `refs` | 支持 |
| **Hermes** | `$HERMES_HOME/config.yaml` 的 `custom_providers` 列表 | 同文件 `api_key` | 支持 |
| Claude Code | `~/.claude/settings.json` 的 `env` | 同文件 `ANTHROPIC_AUTH_TOKEN` | **不支持**，只有单槽位 |
| Gemini CLI | `~/.gemini/.env` + `settings.json` | `.env` 的 `GEMINI_API_KEY` | **不支持**，只有单槽位；本机未安装 |
| Grok Build | `~/.grok/config.toml` 的 `[model."..."]` | 表内 `api_key` | **不支持**，只有单槽位；本机未安装 |
| OpenClaw | `~/.openclaw/openclaw.json` 的 `models.providers` | 同节点 `apiKey` | 支持；本机未安装 |
| Kimi Code | `~/.kimi-code/config.toml` 的 `[providers.<id>]` | 同表 `api_key` | 支持；本机未安装 |
| MiniMax Code | `~/.minimax/config.yaml` 的 `custom_provider` | 同节点 `options.apiKey` | 支持；本机未安装 |

### 管不了的 agent（明确记录）

| agent | 为什么管不了 | 结论 |
|---|---|---|
| **Qoder** | `~/.qoder/settings.json` 只有 `model.preferences`（`qfmodel` 是它自家模型名），`security.auth.selectedType: "qoder-browser"` 说明走浏览器 OAuth。无 provider 定义、无明文 key | **不支持自动管理**，需在 Qoder 界面里配置 |
| **Qoder CN** | 本机未安装，同 Qoder 架构 | 同上 |
| **WorkBuddy AI 版** | `~/.workbuddy-ai` 不存在 | 未安装，未验证 |
| **Trae CN** | `~/.trae-cn` 不存在 | 未安装，未验证 |
| **Claude Desktop** | 本机 `%LOCALAPPDATA%\Claude-3p` 不存在。它的配置在 `configLibrary/<profile id>.json`，结构已知（见 provider-config-keys.md），但没实测 | 未做注入器 |
| **Cursor** | 密钥存系统凭据库，不在配置文件 | **不支持**，需在 Cursor 设置里配置 |

### 不做的操作

- 不管理 OAuth 令牌（ZCode 的 `credentials.json` 是 `enc:v1:` 加密的；Codex 的 `auth.json` 是 ChatGPT 登录态）
- 不读系统凭据库（Cursor、Windows Credential Manager）
- 不自动重启 agent

### 用户必须配合

- 同步前退出 ZCode / Pi / opencode（它们运行时会用内存态覆盖文件）
- `providers.toml` 加进 `.gitignore`（工具会自动检查提示）

> Codex 不再需要加载环境变量脚本。早期版本用 `env_key` + `~/.agents/codex-env.ps1`，现已改成
> `[model_providers.custom]` 表内 `experimental_bearer_token` 内联，和 cc-switch 一致。

---

## 协议支持（四档）

`ProviderKind` 支持四种业界实际存在的 API 协议，各家 agent 按自己的词表映射：

| 本工具 | ZCode `kind` | Pi `api` | Codex `wire_api` | opencode `package` | DSH `api` | WorkBuddy |
|---|---|---|---|---|---|---|
| `anthropic` | anthropic | anthropic-messages | 强制 responses ⚠ | anthropic | anthropic-messages | 标准 |
| `openai-chat` | openai-compatible | openai-completions | 强制 responses ⚠ | openai-compatible | openai-completions | 标准 |
| `openai-responses` | openai-compatible | openai-responses | responses | openai-compatible | openai-responses | `useCustomProtocol: true` |
| `google-generative-ai` | openai-compatible | google-generative-ai | 强制 responses ⚠ | openai-compatible | google-generative-ai | 标准 |

另外四个工具的词表（都是按 cc-switch 核过的）：

| 本工具 | Hermes `api_mode` | Kimi `type` | Grok `api_backend` | MiniMax `api` |
|---|---|---|---|---|
| `anthropic` | anthropic_messages | anthropic | chat_completions | anthropic-messages |
| `openai-chat` | chat_completions | openai | chat_completions | openai-completions |
| `openai-responses` | codex_responses | openai_responses | responses | openai-responses |
| `google-generative-ai` | chat_completions | google-genai | chat_completions | openai-completions |

⚠ **Codex 只支持 Responses 协议**。母版里标其他协议时会被强制写成 `responses`，并在同步日志里明确警告——不静默处理，否则用户会以为配置对了但请求格式不匹配。

**Claude Code 的协议转换只看 `agent_kinds.claude`**，不看模型名。早期实现按「模型 id 不是 `claude-` 开头」判断，会把 MiniMax 这类 Anthropic 兼容端点错翻成 `/chat/completions`。

---

## 界面：一个页面，池 + 分配

**修正了最初的设计。** 第一版做成两个页面：一页改母版、一页看差异再同步。用户指出应该
和 MCP 一致——统一管理、逐条注入。已合并为一页：

| 区域 | 做什么 | 是否写盘 |
|---|---|---|
| provider 列表 | 增删改 `providers.toml`，编辑 TOML | 只写母版 |
| 每行的「分配」 | 展开 agent 卡片，点整块切换该 provider 注入到哪些 agent | 只写母版 |
| 「应用到各 agent」 | 把所有分配关系落到各家配置文件 | 写各 agent 配置 |

交互要点：

- 默认**不展示**分配矩阵，只列 provider。满屏勾选框太吵，也看不出重点
- 分配面板用卡片按钮代替 checkbox：点整块切换，选中是实心绿 + 对勾
- 未安装的 agent（配置文件不存在）卡片灰掉并标「未安装」，点不动
- 分配改动立即写母版，不存在「改完忘了保存」；真正生效要按「应用到各 agent」
- 每次展开一个 provider，上一行自动收起，避免页面拉太长

### Claude 单槽位互斥

Claude Code 只有一组 `ANTHROPIC_*` 环境变量，同一时刻只能一个 provider 生效。
所以勾某个 provider 的 Claude Code 时，后端会自动把 claude 从其他 provider 的
`agents` 里摘掉（`providers/matrix.rs` 的 `SINGLE_SLOT_AGENTS`）。

不这么做的话，母版里会有多条都声明注入 claude，而注入器只取第一个，
界面上的勾选状态和实际行为就对不上了。

Codex、Gemini CLI、Grok Build 也是单槽位，走**同一段**互斥逻辑
（`matrix.rs` 里 `filter(|a| a.single_slot())`），不是各写一遍。

### 单槽位 agent 的额外配置

Claude Code 的四档模型映射仍在**分配页**里改 —— 它是 provider 级配置，
改哪个 provider 的映射就在那一行点卡片正文。

以下两个设置已经移到侧栏的 **Claude & Codex** 页，因为它们都不是
provider 池的增删改：

| 设置 | 位置 | 性质 |
| --- | --- | --- |
| Claude Code 跳过首次引导 | Claude & Codex 页 | 全局，写 `~/.claude.json` |
| Codex 是否绕过 ChatGPT 后端 | Claude & Codex 页 | 当前分配给 Codex 的那条 provider |

Codex 是单槽位，只有一条 provider 生效，所以它的后端开关放在独立页里
比藏在分配矩阵的卡片弹窗里更好找。

### Claude Code 的首次引导开关

它和 provider 无关 —— 写的是 `~/.claude.json` 的 `hasCompletedOnboarding`，
描述「这台机器上的 Claude Code 是否已完成引导」。所以在侧栏的
**Claude & Codex** 页里，不挂在任何 provider 的分配上。

- 界面：侧栏 Claude & Codex 页的勾选框
- CLI：`agentcenter claude skip` / `restore` / `status`

用第三方中转时，首次启动要求登录 Anthropic 账号是多余的；勾上就直接进主界面。
取消勾选是**删掉那个键**，不是写 `false` —— Claude Code 只认 `true`。

### 取消分配 = 真正移除

这是和第一版最大的语义差别。原来所有注入器只增改不删，所以「取消勾选」没有任何效果。
现在每个注入器都会把「母版里有、但没分配给本 agent」的条目**从配置文件里移除**：

| 注入器 | 怎么识别「池内的条目」 | 移除范围 |
|---|---|---|
| zcode | 按 `options.baseURL` 匹配 | `provider` map 里端点相同的条目 |
| pi | 按 provider id 匹配 | `auth.json` 的键 + `models.json` 的 provider |
| opencode | 按 provider id 匹配 | `providers` 定义 + `auth.json` 的键 |
| codex | 按 provider id 匹配 | `[model_providers.<id>]` 表 + env 脚本里的变量 |
| claude | 按 `ANTHROPIC_BASE_URL` 匹配 | 只清 `OWNED_KEYS` 里那几个键 |
| workbuddy | 按模型记录的 `url` 匹配 | 该端点的全部模型记录 |
| dsh | 按 provider id / `ENV_NAME_API_KEY` | `providers` 段 + `credentials.yaml` 的 `refs` |

**边界不变：只删池子里认得出的。** 用户手动配的 provider、ZCode 的内置模板、
Codex 的 OAuth、DSH 的其他 profile，母版里没有对应条目，一律不动。

Claude 的移除还有个额外保护：只有当当前 `ANTHROPIC_BASE_URL` 能对上母版里某个
**明确没分配给 claude** 的 provider 时才清。对不上任何母版端点（订阅登录、
用户手配的第三方）一律不动——清掉会让 Claude 彻底连不上。

