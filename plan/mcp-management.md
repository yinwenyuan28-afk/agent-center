> 给谁：改 MCP 注入器之前，先查该写哪个文件、哪些键。
>
> 一句话：每个工具把 MCP server 记在哪个文件、键路径长什么样、有哪些特殊行为，以及 agentcenter 现在实际写了什么。
>
> 不管：母版 `~/.agents/mcp.toml` 的字段、界面、同步时的备份与哈希复查。那些在 [providers-management.md](providers-management.md) 和代码里。
>
> 状态：2026-11-12。CC Switch 一列从 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的 `src-tauri/src/mcp/` 逐个文件读出（13 个注入器 + `claude_mcp.rs`/`gemini_mcp.rs` 两个落盘模块），没有钉死 commit。「本机」一列是这台机器上实际读到的文件内容和键值。

`~` 是用户主目录。

## 总表

| 工具 | 文件 | 键路径 | 取消分配时 | 我们有没有注入器 | 本机状态 |
| --- | --- | --- | --- | --- | --- |
| Claude Code | `~/.claude.json` | 顶层 `mcpServers` | 删条目 | 有 | 已装，4 个 server |
| Codex | `~/.codex/config.toml` | `[mcp_servers.<id>]` | 删表 | 有 | 已装 0.167.2，5 个 server |
| OpenCode | `~/.config/opencode/` 下三份合并 | `mcp.<id>` | 删条目 | 有 | 已装，4 个 server |
| Pi | `~/.pi/agent/mcp.json` | 顶层 `mcpServers` | 只标 `enabled: false` | 有 | 已装 0.84.2，1 个 server |
| ZCode | `~/.zcode/cli/config.json` | `mcp.servers.<id>` | 删条目 | 有 | 已装，0 个 server |
| WorkBuddy | `~/.workbuddy/connectors/default/mcp.json` | 顶层 `mcpServers` | 删条目 | 有 | 已装，83 个 connector 全停用 |
| Cursor | `~/.cursor/mcp.json` | 顶层 `mcpServers` | 删条目 | 有 | 已装，1 个 server |
| Qoder | `~/.qoder/settings.json` | 顶层 `mcpServers` | 删条目 | 有 | 已装，0 个 server |
| Hermes | `$HERMES_HOME/config.yaml` | `mcp_servers.<id>` | 删条目 | 有 | 没有 `config.yaml` |
| DeepSeek Harness | `~/.dsh/profiles/<profile>/cordis.patch.yml` | patch 数组里 `name: "@deepseek-ai/dsh-mcp-client"` | 删 patch 条目 | **没有** | 已装，0 个 server |
| Gemini CLI | `~/.gemini/settings.json` | 顶层 `mcpServers` | 删条目 | **没有** | 未装 |
| Grok Build | `~/.grok/config.toml` | `[mcp_servers.<id>]` | 删表 | **没有** | 未装 |
| Kimi Code | `~/.kimi-code/mcp.json` | 顶层 `mcpServers` | 删条目 | **没有** | 未装 |
| MiniMax Code | `~/.minimax/mcp.json` | 顶层 `mcpServers` | 删条目 | **没有** | 未装 |
| GitHub Copilot CLI | `~/.copilot/mcp-config.json` | 顶层 `mcpServers` | 删条目 | **没有** | 只有 `logs/` |
| Antigravity | `~/.gemini/config/mcp_config.json` | 顶层 `mcpServers` | 只标 `disabled: true` | **没有** | 未装 |
| Oh My Pi | `~/.omp/agent/mcp.json` | 顶层 `mcpServers` | 只标 `enabled: false` + 写 `disabledServers` | **没有** | 未装 |
| Claude Desktop | `<Claude-3p>/configLibrary/<profile id>.json` | 各家不同 | — | 没有 | 未装 |

CC Switch 的 13 个注入器在 `src-tauri/src/mcp/` 下，一个工具一个文件：`claude.rs`、`codex.rs`、`gemini.rs`、`grokbuild.rs`、`hermes.rs`、`opencode.rs`、`pi.rs`、`agy.rs`、`mcode.rs`、`kimi.rs`、`copilot.rs`、`dsh.rs`、`omp.rs`。

**CC Switch 没有 Cursor、Qoder、WorkBuddy、ZCode 的 MCP 注入器。** 这四个是我们自己核实的。

## 三条跨工具的通则

### 1. Windows 上 npx / npm / node 要包一层 `cmd /c`

`npx`、`npm`、`yarn`、`pnpm`、`node`、`bun`、`deno` 在 Windows 上实际是 `.cmd` 批处理文件，直接当 `command` 写进去，进程起不来。Claude Code 的 `/doctor` 会明确报：

```
Windows requires 'cmd /c' wrapper to execute npx
```

CC Switch 在写 `~/.claude.json` 和 `~/.gemini/settings.json` 时自动转换：

| 母版 | 写进文件 |
| --- | --- |
| `command = "npx"`, `args = ["-y", "pkg"]` | `command = "cmd"`, `args = ["/c", "npx", "-y", "pkg"]` |

规则细节（`claude_mcp.rs:18`）：

- 只处理 `type` 为 stdio（或缺省）的条目，远程型不动
- `command` 已经是 `cmd` / `cmd.exe` 的不重复包
- 命令名取 `file_stem`，所以 `npx.cmd` 也能识别
- 大小写不敏感
- **WSL 路径不包装**（`\\wsl$\...` / `\\wsl.localhost\...`）—— 那里面是 Linux，没有 `cmd`

本机 Claude 已有的 4 个 server 都是这个形态，`command` 全是 `cmd`、`args` 以 `/c` 开头。这说明当前文件就是被包装过的。

**但不是所有工具都需要。** 用 `codex mcp add npxdirect -- npx -y @upstash/context7-mcp` 实地验过：Codex 自己写出来的是 `command = "npx"`，**不包装**。所以包装只在需要它的工具上做（Claude、Gemini、Cursor、Qoder、ZCode、Pi、OpenCode、Kimi、MiniMax），Codex、Grok、DSH、WorkBuddy 不包。

判断依据：那个工具自己的 CLI 在 Windows 上怎么写。写了就照抄，不写就不加 —— 加错了反而让它启动一个多余的 `cmd` 外壳。

### 2. 取消勾选：删条目，还是只标停用

CC Switch 分两派，取决于目标工具认不认「停用」字段：

| 只标停用（保留条目） | 删条目 |
| --- | --- |
| Pi（`enabled: false`） | Claude Code |
| Oh My Pi（`enabled: false` + `disabledServers`） | Codex |
| Antigravity（`disabled: true`） | OpenCode |
| Kimi Code（删条目，`transport` 随条目带走） | Gemini CLI |
| GitHub Copilot CLI（删条目，但先记下 `tools`/`timeout`） | Grok Build |
| | Hermes |
| | MiniMax Code |

为什么要区别对待：这些工具许用户在它们自己的界面里改 `timeout`、工具白名单、OAuth、`exposure` 之类。把条目整个删掉，那些设置就没了；再勾选回来只能重新填。

Pi 的做法最完整（`pi.rs:211`）：停用只写 `enabled: false`，条目和 Pi 自己的字段全留着；重新勾选时把连接字段换掉、`enabled` 改回 `true`，其余原样。万一用户在 Pi 里把条目删了，才从母版里保存的那份 Pi 字段重建。

**我们现在的实现是一律删条目。** 对 Pi 会丢用户设置，要改。

### 3. 各工具自己的字段要保留

母版只有一套字段（`id` / `transport` / `command` / `args` / `cwd` / `url` / `env` / `headers` / `enabled` / `agents`）。各工具在这个基础上还有私有字段，写的时候只能改连接字段，私有字段要原样留下：

| 工具 | 私有字段 |
| --- | --- |
| Codex | `env_vars`（透传的环境变量**名**列表，和 `env` 是两回事）、`startup_timeout_sec`、`tool_timeout_sec`、`bearer_token_env_var`、`env_http_headers` |
| Pi | `timeout`、`description`、`exposure`、`toolExposure`、`oauth`、`auth` |
| Hermes | `enabled`、`timeout`、`connect_timeout`、`tools`、`sampling`、`roots`、`auth` |
| WorkBuddy | `runtime`、`staticEnv`、`staticHeaders`、`npmRegistry`、`disabledTools`、`timeout` |
| Kimi Code | `deferred`、`toolTimeoutMs` |
| GitHub Copilot CLI | `tools`、`timeout` |
| DeepSeek Harness | `toolCallTimeoutMs`、`disabled` |
| Antigravity | `timeout` |
| Oh My Pi | `enabled`、`disabledServers`、`enabledServers` |

Codex 的 `env_vars` 和 `env` 最容易搞混：`env` 是写死的键值，`env_vars` 是「把父进程这几个环境变量透传给孩子进程」的名字列表。本机 `node_repl` 就是 `env_vars = [...]` 加 `[mcp_servers.node_repl.env]` 两张表并存。

## Claude Code

文件是 `~/.claude.json`，**顶层** `mcpServers`。

```json
"mcpServers": {
  "context7": {
    "type": "stdio",
    "command": "cmd",
    "args": ["/c", "npx", "-y", "@upstash/context7-mcp"]
  },
  "remote": {
    "type": "http",
    "url": "https://example.com/mcp"
  }
}
```

三种 `type` 的写法，用 `claude mcp add` 实地验过（2.1.322）：

| 加的方式 | 写出来的条目 |
| --- | --- |
| `claude mcp add probe -- node server.js` | `{"type":"stdio","command":"node","args":["server.js"],"env":{}}` |
| `claude mcp add --transport http <url>` | `{"type":"http","url":"..."}` |
| `claude mcp add --transport sse <url>` | `{"type":"sse","url":"..."}` |

注意 stdio 条目会写一个空 `env: {}` —— 不是省略。我们目前是「env 为空就不写」，读回来没差别，但会让文件的形状和 Claude 自己写的不一致。

**还有一个坑**：同一个文件里 `projects.<项目路径>.mcpServers` 是项目级配置，本机 8 个项目下全是 `{}`。只能改顶层，别碰 `projects`。

取消分配 = 删条目。没有 `enabled` 字段，存在即启用。

## Codex

文件是 `~/.codex/config.toml`，`[mcp_servers.<id>]` 表。

**这个工具我们今天抓出两个真 bug，下面两条都要照做。**

### 不写 `type`

`codex mcp add` 自己写出来的条目里没有 `type`：

```toml
[mcp_servers.probe-stdio]
command = "node"
args = ["server.js"]

[mcp_servers.probe-http]
url = "https://example.com/mcp"
```

传输类型由 `command`（stdio）或 `url`（streamable HTTP）推断。写进去的 `type` 会被当成未知字段：

```
$ codex doctor --json
"startup warning": "Codex is ignoring 4 unrecognized configuration settings.
  user (C:\Users\leaf\.codex\config.toml): `mcp_servers.context7.type` is ignored.
  ...
```

本机这份 `config.toml` 里 5 个 server 有 4 个带着 `type = "stdio"`，每次启动都报 4 条警告。这是我们之前的注入器写进去的。

**现在的 `CodexMcpInjector` 会写 `t["type"] = ...`，要删掉。**

### 请求头字段是 `http_headers`，不是 `headers`

同一份 `doctor` 输出：

```
"startup warning": "Codex is ignoring 1 unrecognized configuration setting.
  user (...): `mcp_servers.h1.headers` is ignored."
```

写 `headers` 不报错，但**被悄悄忽略** —— 远程 MCP 的 `Authorization` 头就丢了，表现是 401。必须写 `http_headers`。

**现在的 `CodexMcpInjector` 写的是 `t["headers"]`，要改成 `http_headers`。** 读取时两个都要认（老文件里可能已经有 `headers`）。

### 其余字段

| 字段 | 用途 |
| --- | --- |
| `command` / `args` / `cwd` / `env` | stdio |
| `env_vars` | 要透传的环境变量**名**列表 |
| `startup_timeout_sec` | 启动超时（秒） |
| `tool_timeout_sec` | 工具调用超时（秒） |
| `url` / `http_headers` | 远程 |
| `bearer_token_env_var` | 从哪个环境变量读 Bearer 令牌 |
| `env_http_headers` | 用环境变量填的头 |

一个传输方式的专属字段不能出现在另一种里。写了 `command` 又写 `url`，整份配置拒绝加载（`url is not supported for stdio`）。

### 历史错误格式

早期版本把 MCP 写到了 `[mcp.servers.<id>]`（多了一层）。CC Switch 读的时候两种都认，写的时候会把 `[mcp.servers]` 清掉并迁移到 `[mcp_servers]`。我们也应该读两种，只写标准的那一种。

## OpenCode

**这里最容易写错，因为 OpenCode 不是一个文件。**

`~/.config/opencode/` 下的配置文件是**依次合并**的，后加载的覆盖先加载的（从 `app.asar` 里的实际代码读出）：

```
config.json  →  opencode.json  →  opencode.jsonc
```

本机正好是分裂的：

| 文件 | 有什么 |
| --- | --- |
| `opencode.jsonc` | `$schema`、`shell`、`provider`（含 apiKey） |
| `opencode.json` | `mcp`（4 个 server）、`provider`（旧的一份） |

`opencode.jsonc` 后加载，所以 provider 用 jsonc 那份、`opencode.json` 里的 provider 是死配置；而 `mcp` 只存在于 `opencode.json`，所以生效。

**这带来两条结论：**

1. 读的时候要读全部三份并合并，只看一份会漏
2. 写的时候要写进**已经有 `mcp` 段的那份**。写到 `.jsonc` 里也能生效，但会把同一段配置拆到两个文件，用户下次打开 `.json` 看不到自己刚加的 server，反而更乱

CC Switch 的 `resolve_config_path` 只挑一个（`.jsonc` 优先），在本机这种分裂配置上会把 MCP 写进 `.jsonc`，然后把 `.json` 里的旧 `mcp` 留在原地 —— 两份都有 `mcp`，覆盖关系变得很隐晦。**这一点我们做得比 CC Switch 对。**

### 条目格式

```json
"mcp": {
  "context7": {
    "type": "local",
    "command": ["cmd", "/c", "npx", "-y", "@upstash/context7-mcp"],
    "enabled": true
  },
  "remote": {
    "type": "remote",
    "url": "https://example.com/mcp",
    "enabled": true
  }
}
```

| 母版 | OpenCode |
| --- | --- |
| `type: "stdio"` | `type: "local"` |
| `command` + `args` | `command` 是**数组**，命令在 `[0]` |
| `env` | `environment` |
| `type: "sse"` / `"http"` | `type: "remote"` |
| `enabled` | `enabled`（不是 disabled） |

读回来时 `command[0]` 当命令、其余当参数。拆不回原始分界（不知道原母版是 `command="npx", args=["-y","pkg"]` 还是 `command="npx -y"`），但业界惯例是 `[0]` 为可执行文件，按这个走。

`.jsonc` 的注释：CC Switch 用 `JsoncDocument` 保留注释；我们用 `serde_json`，写回会丢注释。本机的 `.jsonc` 现在没有注释，但以后可能有。

## Pi

文件是 `~/.pi/agent/mcp.json`，顶层 `mcpServers`。

```json
{
  "imports": ["claude-code"],
  "mcpServers": {
    "context7": {
      "type": "stdio",
      "command": "cmd",
      "args": ["/c", "npx", "-y", "@upstash/context7-mcp"]
    }
  }
}
```

注意顶层还有 `imports` 和 `settings` 两个键 —— **不能整份重写，只改 `mcpServers`**。`imports: ["claude-code"]` 的意思是「自动从 Claude Code 的配置里导入」，本机凭这个把 Claude 的 server 也拉进来了。

### 三条硬约束

1. **不支持 SSE**。`type` 只能是 `stdio` / `http` / `streamable-http`，或省略。写 `sse` 直接拒绝 —— 要改成 HTTP
2. **服务器名字符集**：只能用字母、数字、`_`、`-`。母版里带空格或点的 id 写不进去
3. **`-` 和 `_` 等价**。`dev-tools` 和 `dev_tools` 在 Pi 里是同一个服务器。同名冲突要提前拒绝，否则每次同步都报错

### Pi 自己的字段

`enabled`、`timeout`、`description`、`exposure`、`toolExposure`、`oauth`、`auth`。写入时只替换连接字段（`command`/`args`/`env`/`cwd`/`url`/`headers`/`type`），这些原样保留。

### 停用保留条目

取消分配只写 `enabled: false`，条目留下。重新勾选时换掉连接字段、`enabled` 改回 `true`。用户如果在 Pi 里把条目删了，就从母版里存的那份 Pi 字段重建。

`type: "streamable-http"` 读进来归一成 `http`，写回去用 Pi 认的写法。

## ZCode

文件是 `~/.zcode/cli/config.json`，键路径是 **`mcp.servers.<name>`**，比别家多两层。

```json
{
  "mcp": {
    "servers": {
      "context7": {
        "type": "stdio",
        "command": "cmd",
        "args": ["/c", "npx", "-y", "@upstash/context7-mcp"],
        "enabled": true
      }
    }
  }
}
```

本机这份文件现在只有 `plugins.enabledPlugins`，没有 `mcp` 段。

### 回退文件

`~/.agents/mcp.json` 是**同作用域的回退**：只有 `~/.zcode/cli/config.json` 里没有任何 MCP server 时才会读它，而且是顶层 `mcpServers`（不是 `mcp.servers`）。本机没有这个文件。

同一个作用域里 `.zcode` 只要有 server，`.agents/mcp.json` 就整体被忽略 —— 不是逐条合并。

### schema 严格

ZCode 的键名和别家有四处不同，写错了整条被丢弃：

| 别家写法 | ZCode 要的 |
| --- | --- |
| `environment` | `env` |
| `http_headers` | `headers` |
| `enable` | `enabled` |
| `type: "remote"` | `type: "http"` |

CLI 直接读文件时会自动迁移这些旧写法，但桌面版走的是另一条解析路径，**不迁移**。所以写的时候一律用新写法。

`command` 必须是字符串。写成数组（OpenCode 那种）会让 Settings → MCP 页面崩，报 `command.trim is not a function`。

## WorkBuddy

文件是 `~/.workbuddy/connectors/default/mcp.json`，顶层 `mcpServers`。

本机有 83 个条目，全部 `connector:` 前缀、全部 `disabled: true`。这是内置市场目录，不是用户配置。

```json
"mcpServers": {
  "connector:linear-mcp": {
    "url": "https://mcp.linear.app/mcp",
    "disabled": true
  },
  "connector:weisheng-scrm": {
    "type": "stdio",
    "command": "npx",
    "args": ["--registry=...", "-y", "mcp-server-weisheng-scrm@latest"],
    "env": { "SCRM_APP_KEY": "" },
    "runtime": { "type": "node", "version": ">=18" },
    "disabled": true
  }
}
```

### 与别家的差异

| | WorkBuddy |
| --- | --- |
| 启用字段 | `disabled`（反义），不是 `enabled` |
| `type` 词表 | `stdio` / `sse` / `http` / `streamableHttp` / `streamable-http` 五种都出现过 |
| 请求头 | 两个都见过：`headers` 和 `staticHeaders` |
| 私有字段 | `runtime`、`staticEnv`、`npmRegistry`、`disabledTools`、`timeout` |

**绝不碰 `connector:` 前缀的条目。** 它们不在母版里，注入器只操作母版声明过的 id。

## Cursor / Qoder

两者都是标准 `mcpServers` map，和 Claude 同构，差别只有文件位置：

- `~/.cursor/mcp.json`
- `~/.qoder/settings.json` 的 `mcpServers`

Qoder 的 `settings.json` 还有 `enabledPlugins`、`permissions`、`security`、`model` 四个键，只能改 `mcpServers`。

这两个工具 CC Switch 都没做。

## Hermes

文件是 `$HERMES_HOME/config.yaml`，没有 `HERMES_HOME` 时是 `~/.hermes/config.yaml`。本机 `~/.hermes` 只有 `scripts/`，没有 `config.yaml`。

```yaml
mcp_servers:
  time:
    command: uvx
    args: ["mcp-server-time"]
    timeout: 120
    connect_timeout: 60
  remote:
    url: https://example.com/mcp
    headers:
      Authorization: Bearer xxx
    enabled: true
```

**没有 `type`**，由 `command` / `url` 推断。

私有字段：`enabled`、`timeout`、`connect_timeout`、`tools`、`sampling`、`roots`、`auth`。`auth: "oauth"` 是 Hermes 读的 OAuth 声明，往返时不能丢 —— 丢了就从已授权降级成未认证。

这个文件里还有大段注释和 `skills.disabled` 长数组，**不能用 YAML 库整体重写**，只能行级编辑。我们现在的实现就是行级的。

## DeepSeek Harness

**这是结构最特殊的。** `~/.dsh/profiles/<profile>/cordis.patch.yml` 是一份 YAML **数组**（loader 的 patch 列表），不是映射。

MCP server 是数组里 `name` 为 `@deepseek-ai/dsh-mcp-client` 的一个条目：

```yaml
- id: cc-switch-mcp-context7
  name: "@deepseek-ai/dsh-mcp-client"
  config:
    serverName: context7
    transport: stdio
    command: cmd
    args: ["/c", "npx", "-y", "@upstash/context7-mcp"]
    env:
      KEY: value
```

| 字段 | 值 |
| --- | --- |
| `config.serverName` | 服务器名，就是母版的 id |
| `config.transport` | `stdio` 或 `streamable-http`（**不是** `http`） |
| `config.command` / `args` / `env` / `cwd` | stdio |
| `config.url` / `headers` | 远程 |
| `disabled` | 在 `config` 外面，条目同级 |

### 四个 profile 都要写

`~/.dsh/profiles/` 下的 `web`、`desktop`、`headless`、`acp` 是四套独立配置。**同一个 MCP server 要写进每一个已存在的 profile**，否则换个 profile 启动就没了。

这四个 profile 之间必须先比对：如果某个 server 在几个 profile 里的连接方式不一致，拒绝写入并报错（`dsh.mcp_diverged`）。本机只有 `web` 和 `desktop`，各一份 `cordis.patch.yml`，里面都没有 MCP 条目。

### 三条校验

1. **不支持 SSE**，只能用 `streamable-http`
2. 名字 1–32 字符，只能 `[A-Za-z0-9_-]`
3. 条目可以有 `insert:` 分组（`- insert: [ ... ]`），MCP 条目可能嵌在分组里，删的时候要从数组里摘掉而不是删整个 patch

### 动态 YAML 会拒绝

文件里允许 `!!js process.env.X` 这类表达式。如果某个要复制的条目里含动态节点，拒绝复制并提示用户在 DSH 里改 —— serde_yaml 往返会把它变成普通字符串，等于改坏。

## Gemini CLI

文件是 `~/.gemini/settings.json`，顶层 `mcpServers`。

```json
"mcpServers": {
  "stdio-one": {
    "command": "cmd",
    "args": ["/c", "npx", "-y", "@upstash/context7-mcp"],
    "timeout": 60000
  },
  "http-one": {
    "httpUrl": "https://example.com/mcp",
    "timeout": 60000
  },
  "sse-one": {
    "url": "https://example.com/sse",
    "timeout": 60000
  }
}
```

### 三个特殊点

1. **没有 `type` 字段**，靠字段名推断：有 `command` 是 stdio，有 `httpUrl` 是 HTTP，有 `url` 是 SSE
2. **HTTP 用 `httpUrl`，不是 `url`**。写 `url` 会被当成 SSE
3. **`timeout` 只收毫秒，且只有一个**。CC Switch 从 `startup_timeout_sec` / `startup_timeout_ms` / `tool_timeout_sec` / `tool_timeout_ms` 四个字段换算，取最大值。都没设时用默认：启动 10 000 ms、工具 60 000 ms，所以最终写 60 000

本机未装。

## Grok Build

文件是 `~/.grok/config.toml`，`[mcp_servers.<id>]`，结构几乎和 Codex 一样。

**和 Codex 的关键差异：请求头字段是 `headers`，不是 `http_headers`。** 两个工具共用同一份转换函数时，Grok 要把 `http_headers` 改回 `headers`。

同样不写 `type`，由 `command` / `url` 推断。

本机未装。

## Kimi Code

文件是 `~/.kimi-code/mcp.json`，顶层 `mcpServers`。

```json
"mcpServers": {
  "stdio-one": {
    "command": "node",
    "args": ["server.js"],
    "enabled": true
  },
  "sse-one": {
    "transport": "sse",
    "url": "https://example.com/sse",
    "enabled": true
  }
}
```

SSE 的写法是 `transport: "sse"` 而不是 `type`；stdio 和 HTTP 都不写 `type`。

私有字段：`deferred`、`toolTimeoutMs`。停用时**删条目**（不像 Pi 那样保留）。

本机未装。

## MiniMax Code

文件是 `~/.minimax/mcp.json`，顶层 `mcpServers`。和 Claude 同构，多一个 `enabled: true`。

私有字段：条目里其他键原样保留（比如 `timeout`）。停用时删条目。

本机未装。

## GitHub Copilot CLI

文件是 `~/.copilot/mcp-config.json`，顶层 `mcpServers`。

```json
"mcpServers": {
  "stdio-one": {
    "type": "local",
    "command": "node",
    "args": ["server.js"],
    "env": { "K": "v" },
    "tools": ["*"]
  },
  "remote-one": {
    "type": "http",
    "url": "https://example.com/mcp"
  }
}
```

| 母版 | Copilot |
| --- | --- |
| `stdio` | `type: "local"`，`args` 必须存在（空则写 `[]`） |
| `http` / `sse` | 同名 |
| 无 | `tools` 默认 `["*"]` |

### 停用要额外记东西

Copilot 停用一个 server 的方式是**从 `mcp-config.json` 删掉条目**，并在 `~/.copilot/settings.json` 的 `disabledMcpServers` 数组里记名字。

删条目前要把 `tools`、`timeout` 两个 Copilot 私有字段存到 `~/.copilot/mcp-options.json`（CC Switch 存在 `~/.cc-switch/copilot-mcp-options.json`），重新启用时还回去 —— 否则工具白名单会从精心裁剪的几个被放宽成 `["*"]`。

本机 `~/.copilot` 只有 `logs/`，未初始化。

## Antigravity

文件是 `~/.gemini/config/mcp_config.json` —— 注意是 `~/.gemini/config/`，和 Gemini CLI 的 `~/.gemini/settings.json` 不同目录。

顶层 `mcpServers`，用 `disabled`（反义）。`type` 不写。老的 `serverUrl` 字段读进来要归一成 `url`。

不支持 SSE。

首次启动时 Antigravity 会建一个 0 字节的 `mcp_config.json`，要当成「没有配置」而不是解析失败。

本机未装。

## Oh My Pi

文件是 `~/.omp/agent/mcp.json`，顶层 `mcpServers`。

结构同 Pi（`type` / `enabled` / 各私有字段），但停用要**同时**做两件事：

1. 条目里写 `enabled: false`
2. 把 id 写进文件顶层的 `disabledServers` 数组，并从 `enabledServers` 里移除

只写 `enabled: false` 不够 —— OMP 会从项目级目录发现同名 server 并把它重新激活。`disabledServers` 是用户的拒绝名单，优先级最高。

本机未装。

## 我们现在的实现和核实结果的差距

### 必须改的（会写坏配置）

| 问题 | 影响 | 位置 |
| --- | --- | --- |
| Codex 写 `type` | Codex 每次启动报「忽略未知字段」，本机已有 4 条 | `mcp/inject.rs` `CodexMcpInjector` |
| Codex 写 `headers` | 远程 MCP 的 `Authorization` 被静默忽略，表现为 401 | 同上 |
| 没有 Windows `cmd /c` 包装 | npx / npm / node 型的 server 在 Claude、Gemini、Cursor、Qoder、ZCode 里起不来 | `to_standard_entry` |
| OpenCode 只读 `opencode.json` | 本机 `.jsonc` 里的 provider 读不到；MCP 写在 `.json` 里恰好读对了，但换台机器就可能读漏 | `OpencodeMcpInjector` / `scan_opencode` |
| Pi 取消分配时删条目 | 丢用户设的 `timeout` / `exposure` / `oauth` | `McpServersMapInjector`（pi 用了它） |
| Pi 不拒 SSE、不校验名字 | 写进去 Pi 整条不认，每次同步报错 | 同上 |

### 缺的（7 个工具没有注入器）

Gemini CLI、Grok Build、Kimi Code、MiniMax Code、GitHub Copilot CLI、Antigravity、Oh My Pi，还有 DeepSeek Harness。

矩阵现在有 9 列（`agents::mcp_capable()`），扫描有 10 项（`scan_all` 里多了个 dsh 的占位）。DSH 的 MCP 位置这一轮已经探明，要把占位换成真实现。

### 附带的一个问题

`McpServer` 模型只有一套字段，装不下各工具的私有字段。这意味着：即使注入器改成「保留原条目其余字段」，母版里也没地方存 `env_vars`、`tools`、`exposure` 这些。

两个选择：

- **母版加一张 `extra` 表**（`BTreeMap<String, String>` 之类），存各工具的私有字段。缺点是类型弱，嵌套值（`runtime: {type, version}`）没法表达
- **写的时候从现有文件里读回来合并**（CC Switch 的做法）。母版不管私有字段，注入器读现有条目 → 只替换连接字段 → 保留其余。缺点是母版不是唯一真相来源

CC Switch 选的是后者。**我们也应该选后者**：母版只表达「连接方式 + 分配给谁」，各工具自己的设置由各工具管，我们只保证往返不丢。这样母版保持干净，也不用为每个工具加一堆字段。

## 明确的边界

**能管理**：Claude Code、Codex、OpenCode、Pi、ZCode、WorkBuddy、Cursor、Qoder、Hermes、DeepSeek Harness

**暂不管理**（工具未装，实现照 CC Switch 移植但没实测）：Gemini CLI、Grok Build、Kimi Code、MiniMax Code、GitHub Copilot CLI、Antigravity、Oh My Pi

**管不了**：

- **Claude Desktop**：配置在 Electron 的 `configLibrary/<profile id>.json` 里，每个 profile 一套，路径随安装位置变。本机未装，没实现
- **WorkBuddy 的 83 个 connector**：内置市场目录，全部 `disabled: true`，是产品自带的清单而不是用户配置。注入器只操作母版声明过的 id，这批永远不碰
- **ZCode 的 `.agents/mcp.json` 回退文件**：只在主文件完全没有 MCP 时才生效。我们只写主文件，不去猜这个回退状态

**不做**：

- 不执行 MCP server（只管理配置，不验证连通性）
- 不做 MCP 网关代理
- 不管理 OAuth 型 MCP 的授权流程
- 不同步项目级配置（Claude 的 `projects.*.mcpServers`、OpenCode 的 `.opencode/`、Pi 的 `.pi/mcp.json`）

## 实施记录

### 2026-11-10 第一版

最初做成「同步」——把母版 server 全量推给声明的 agent。改成**分配矩阵**：所有 server 在一个池子里，每个 agent 独立勾选。

两者的实质差异：

| | 同步 | 分配矩阵 |
| --- | --- | --- |
| 模型 | 母版声明 agents → 推给它 | server 池 × agent 列，格子是开关 |
| 取消勾选 | 不写入（但也不删旧的） | 从该 agent 配置里移除 |
| 视图 | 按 agent 分组列差异 | 行列矩阵，一眼看全 |

注入器从「只增改」改成「先移除未分配的，再写入已分配的」。移除时**只操作池子里的 id** —— 用户手动配的、WorkBuddy 的 83 个市场连接器都不在池子里，永远不被碰。

### 2026-11-12 按核实结果对齐

把 CC Switch 的 13 个注入器逐个读完，逐一核对本机文件，抓到两个真 bug（Codex 的 `type` 和 `headers`），都当场用 `codex doctor --json` 验证过。OpenCode 的三份文件合并顺序是从 `app.asar` 的实际代码里读出来的，不是猜的。

这一轮还确认了一件事：**母版不应该存各工具的私有字段**。注入器读现有条目、只替换连接字段、保留其余，这是 CC Switch 的做法，也是唯一能让「用户在工具里改的 timeout 不被我们洗掉」成立的做法。

改完之后的状态：

| | 之前 | 现在 |
| --- | --- | --- |
| 注入器 | 7 个，全在一个 `inject.rs`（1173 行） | 17 个，一个工具一个文件 |
| 扫描 | 10 项（含 DSH 占位） | 17 项，DSH 换成真实现 |
| 矩阵列 | 9 | 17 |
| Codex `type` | 写，本机每次启动报 4 条未知字段 | 不写，`codex doctor` 报 0 条 |
| Codex 远程头 | 写 `headers`（被静默忽略） | 写 `http_headers` |
| Windows `cmd /c` | 不做 | 按工具分别处理，且**幂等** |
| opencode 读 | 只读 `opencode.json` | 三份合并读，写进已有 `mcp` 段的那份 |
| Pi 取消分配 | 删条目（丢用户设置） | 只标 `enabled: false` |

### 这一轮被测试逼出来的三个 bug

**1. Windows 包装被重复应用。** 文件里已经是 `cmd /c npx`，母版里也是（本机母版就是包装过的），注入器再包一层就成了 `cmd /c cmd /c npx`。表现是每次同步都说「有改动」，越同步越深。修法：比较前把两侧都还原到最内层（`shared::unwrap_windows_command`），只比命令和参数。

**2. Codex 的 `env_vars` 被洗掉。** 更新已有条目时我把「两种传输方式的专属字段」全部清掉，结果 stdio 条目上的 `env_vars`（透传的环境变量名列表）和 `startup_timeout_sec` 一起没了。本机 `node_repl` 就有这两个字段。修法：只清**另一种**传输方式的专属字段。

**3. Codex 的旧 `headers` 子表残留。** `headers` 是远程字段，写远程时按「不清本方字段」的逻辑留了下来，于是 `headers` 和 `http_headers` 两个表同时存在，Codex 只认后者、把前者报成未知字段。修法：`headers` 无论什么传输都清掉。

三条都有测试守着（`mcp/inject/codex.rs` 的单测 + `scripts/verify-codex-mcp.mjs` 用 `codex doctor` 实测）。

### 一个改过的判断

**不是所有工具都要包 `cmd /c`。** 最初打算照 CC Switch 给所有 stdio 条目都包，实际用 `codex mcp add npxdirect -- npx -y pkg` 验了一下：Codex 自己写出来的是 `command = "npx"`，不包。所以按工具分别处理：

| 要包 | 不包 |
| --- | --- |
| Claude、Gemini、Cursor、Qoder、ZCode、Pi、OpenCode、Kimi、MiniMax | Codex、Grok、DSH、WorkBuddy |

判断依据是「那个工具自己的 CLI 在 Windows 上怎么写」。加错了会让它平白多起一个 `cmd` 外壳。

### 验证

- `cargo test --lib`：41 项
- `npm run verify`：契约 30 类型 / 167 字段、svelte-check 0 错误、构建通过
- 16 个测试脚本全绿，其中新增 `verify-codex-mcp.mjs`（用 codex doctor 断言 0 条未知字段）
- 真实环境：本机 `mcp sync` 之后 `mcp diff` 报 0 处改动，第二次同步 0 写入
- 真实配置完好：Claude 的 8 个 `projects` 段未动、Pi 的 `imports` 保留、WorkBuddy 83 个 connector 一个不少、opencode 的 `.jsonc` 里 `provider.apiKey` 还在
