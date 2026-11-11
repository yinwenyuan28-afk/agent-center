# MCP 统一管理

## 背景

本机 7 个 agent 各自维护 MCP 配置，同一批 server 重复配了十几遍：

| agent | 位置 | 格式 | 当前 server 数 |
|---|---|---|---|
| Claude Code | `~/.claude.json` 顶层 `mcpServers` | JSON | 4 |
| Codex | `~/.codex/config.toml` 的 `[mcp_servers.*]` | TOML | 6 |
| Cursor | `~/.cursor/mcp.json` | JSON | 1 |
| pi | `~/.pi/agent/mcp.json` | JSON | 3 |
| opencode | `~/.config/opencode/opencode.json` 的 `mcp` | JSON（数组式 command） | 4 |
| Qoder | `~/.qoder/settings.json` 的 `mcpServers` | JSON | 1 |
| WorkBuddy | `~/.workbuddy/connectors/default/mcp.json` | JSON（83 个 connector） | 83 |
| ZCode | — | 未找到独立 MCP 配置 | 0 |
| DSH | — | `cordis.patch.yml` 里没有 mcp 段 | 0 |

`context7`、`fetch`、`time`、`sequential-thinking` 这四个在 Claude、Codex、opencode 三处都配了，`context7` 在 pi 里还有第四份。

## 各家结构差异

### Claude Code

```jsonc
// ~/.claude.json
"mcpServers": {
  "context7": {
    "type": "stdio",
    "command": "cmd",
    "args": ["/c", "npx", "-y", "@upstash/context7-mcp"]
  },
  "remote-example": {
    "type": "sse",
    "url": "..."
  }
}
```

**注意**：`~/.claude.json` 里还有 `projects` 段，每个项目下也可有 `mcpServers`。本机实测项目级都是空的。只改顶层。

### Codex

```toml
[mcp_servers.context7]
type = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]

[mcp_servers.node_repl]
command = "..."
env_vars = ["VAR1", "VAR2"]        # 透传的环境变量**名**
startup_timeout_sec = 120

[mcp_servers.node_repl.env]         # 子表，实际值
KEY = "value"
```

**注意**：`env_vars` 是环境变量名列表（从父进程透传），`env` 是子表写死值。这两个要区分。

### opencode

```jsonc
"mcp": {
  "context7": {
    "type": "local",
    "command": ["cmd", "/c", "npx", "-y", "@upstash/context7-mcp"],  // 数组！
    "enabled": true
  },
  "remote": {
    "type": "remote",
    "url": "...",
    "enabled": true
  }
}
```

**关键差异**：`command` 是**数组**（command + args 合并），且用 `enabled` 而不是 `disabled`。

### pi

```jsonc
"mcpServers": {
  "campus-hub": {
    "type": "stdio",
    "command": "node",
    "args": ["src/mcp.ts"],
    "cwd": "...",
    "directTools": false
  }
}
```

基本和 Claude 一致，多一个 `directTools` 字段。

### WorkBuddy

```jsonc
"mcpServers": {
  "connector:linear-mcp": {
    "url": "https://mcp.linear.app/mcp",
    "disabled": true
  },
  "connector:weisheng-scrm": {
    "type": "stdio",
    "command": "npx",
    "args": [...],
    "env": { "SCRM_APP_KEY": "" },
    "runtime": { "type": "...", "version": "..." },
    "disabled": true
  }
}
```

**特殊点**：
- 名字全部带 `connector:` 前缀（内置市场连接器）
- 用 `disabled`（默认禁用），不是 `enabled`
- 有 WorkBuddy 特有的 `runtime`、`staticEnv`、`npmRegistry`、`disabledTools` 字段
- 83 个 connector 全是 `disabled: true`，说明用户没在用

### Qoder

```jsonc
"mcpServers": {
  "campus-hub": {
    "type": "stdio",
    "command": "cmd",
    "args": ["/c", "npm", "run", "mcp"],
    "cwd": "...",
    "directTools": true
  }
}
```

和 Claude / pi 同构。

## 统一数据模型

母版 `~/.agents/mcp.toml`：

```toml
[[server]]
id = "context7"
name = "Context7"
# stdio / sse / http
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
# 工作目录，可选
cwd = ""
# 环境变量，可选
[server.env]
FOO = "bar"
# 是否启用
enabled = true
# 注入到哪些 agent
agents = ["claude", "codex", "pi", "opencode"]

[[server]]
id = "some-remote"
name = "远程示例"
transport = "http"
url = "https://example.com/mcp"
enabled = true
agents = ["claude", "cursor"]
```

**为什么 transport 分 stdio / sse / http 三档**：三种协议的字段完全不同——
- `stdio` 用 `command` + `args` + `env` + `cwd`
- `sse` / `http` 用 `url` + `headers`

### 各 agent 的字段映射

| 统一字段 | Claude | Codex | pi | opencode | WorkBuddy | Qoder |
|---|---|---|---|---|---|---|
| id | map 键 | `[mcp_servers.<id>]` | map 键 | `mcp` 的键 | map 键 | map 键 |
| transport | `type` | `type` | `type` | `type`: local/remote | `type` | `type` |
| command | `command` | `command` | `command` | `command[0]` | `command` | `command` |
| args | `args` | `args` | `args` | `command[1..]` | `args` | `args` |
| url | `url` | `url` | `url` | `url` | `url` | `url` |
| env | `env` | `[mcp_servers.<id>.env]` | `env` | `environment` | `env` | `env` |
| enabled | （无，存在即启用） | （无） | （无） | `enabled` | `disabled`（反义） | （无） |

**opencode 的 `command` 数组**是最容易出错的地方：要把 `command` 和 `args` 拼起来写，读回来时要拆开（但拆不回来——不知道哪部分是 args）。

**解决方式**：读 opencode 时把 `command` 数组整体当作 `command` + `args`，`command[0]` 是命令、其余是参数。这与业界惯例一致（第一个元素总是可执行文件）。

## 五个安全问题（沿用 provider 模块的做法）

1. **所有权清单**：每个 agent 只接管 `mcpServers` / `mcp` 段，其余键不碰
2. **解析失败即全停**：一个文件坏了，整次不写
3. **hash 复查**：写入前重读比对，变了就中止
4. **原子写**：同目录临时文件 + fsync + rename
5. **首写备份**：每个文件第一次被改前完整复制

**额外一条**：**不删未声明的 server**。用户手动配的（尤其 WorkBuddy 那 83 个 connector）一律不动。这和 provider 注入器的原则一致。

## 实施记录（2026-11-10）

**修正了最初的设计误判。** 第一版做成了「同步」——把母版 server 全量推给声明的 agent。
用户指出真正要的是**分配矩阵**：所有 server 在一个池子里，每个 agent 独立勾选用哪些。

两者的实质差异：

| | 同步 | 分配矩阵 |
|---|---|---|
| 模型 | 母版声明 agents → 推给它 | server 池 × agent 列，格子是开关 |
| 取消勾选 | 不写入（但也不删旧的） | **从该 agent 配置里移除** |
| 视图 | 按 agent 分组列差异 | 行列矩阵，一眼看全 |

**关键实现改动**：注入器从「只增改」改成「先移除未分配的，再写入已分配的」。
移除时**只操作池子里的 id**——用户手动配的、WorkBuddy 的 83 个市场连接器都不在池子里，永远不被碰。

已验证（副本测试）：

| 场景 | 结果 |
|---|---|
| `context7` 只分配给 claude | ✅ Codex 里被移除 |
| 改成 `context7` 给 codex | ✅ Codex 里新增 |
| 取消 `fetch` 的 claude 分配 | ✅ Claude 里被移除 |
| 用户手动的 `time` / `sequential-thinking` | ✅ 始终保留 |

界面：矩阵表，行=server，列=7 个 agent，格子是开关。点列名可整列全选/全不选。
点格子只改母版分配，点「应用到各 agent」才写配置文件。

## 实施阶段

| 阶段 | 内容 | 验收 |
|---|---|---|
| 1 | `mcp.toml` 母版读写 + 类型 + 契约 | `npm run verify` 全绿 |
| 2 | 只读扫描 + 差异对比 | `agentcenter mcp diff`，不写盘 |
| 3 | Claude / pi / Qoder 注入器（三者同构，一起做） | 副本上验证 |
| 4 | Codex / opencode 注入器（TOML 与数组 command） | 副本上验证 |
| 5 | WorkBuddy 注入器（保留 runtime 等特有字段） | 副本上验证 |
| 6 | 界面：MCP 管理页 + MCP 同步页 | 全流程可用 |

## 抄了什么，没抄什么

参考了这些产品的设计（不是代码）：

| 产品 | 借鉴点 |
|---|---|
| **McpMux** | 「一份配置，多客户端同步」的核心模型；差异预览后才同步的两步流程 |
| **MCP Palette** | Profile 概念（本实现简化为单母版）、启用/禁用开关保留用户定制 |
| **MCP Toolkit** | 按项目分组、「配置与密钥分离」 |
| **mcpx** | 网关思路（本实现不采用，但保留扩展空间） |

**没抄代码**的原因：
- 它们多为 TypeScript/Electron，我们是 Rust + Svelte，直接移植成本高于重写
- 许可证不一，部分为 GPL
- 它们的 agent 覆盖不含国产工具，核心适配层本来就要重写

## 明确的边界

**能管理**：Claude Code、Codex、Cursor、pi、opencode、Qoder、WorkBuddy

**管不了**：
- **ZCode**：未找到独立的 MCP 配置位置。`~/.zcode/cli/config.json` 只有插件开关
- **DSH**：`cordis.patch.yml` 里没有 mcp 段，可能有其他机制未探明
- **WorkBuddy 的 83 个 connector**：全部 `disabled: true`，属于内置市场目录而非用户配置。工具只处理用户主动声明的 server，不动这批

**不做**：
- 不执行 MCP server（只管理配置）
- 不做 MCP 网关代理
- 不管理 OAuth 型 MCP 的授权流程
