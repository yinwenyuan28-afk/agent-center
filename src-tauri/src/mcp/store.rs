//! MCP 母版 `~/.agents/mcp.toml` 的读写。
//!
//! 和 provider 母版一样的设计：一份文件是唯一真相来源，
//! 各 agent 的配置由它注入生成。

use super::model::{McpFile, McpServer};
use std::path::PathBuf;

/// 母版路径。
pub fn path() -> PathBuf {
    super::super::providers::store::home_dir()
        .join(".agents")
        .join("mcp.toml")
}

/// 读取母版。文件不存在返回空列表。
pub fn load() -> anyhow::Result<Vec<McpServer>> {
    let p = path();
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p)?;
    let file: McpFile = toml::from_str(&text)?;
    Ok(file.servers)
}

/// 保存母版。
pub fn save(servers: &[McpServer]) -> anyhow::Result<()> {
    // 校验：id 唯一 + 必填字段
    let mut seen = std::collections::BTreeSet::new();
    for s in servers {
        if let Some(err) = s.validate() {
            anyhow::bail!("{err}");
        }
        if !seen.insert(s.id.clone()) {
            anyhow::bail!("server id 重复：{}", s.id);
        }
    }

    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = McpFile {
        servers: servers.to_vec(),
    };
    let text = toml::to_string_pretty(&file)?;
    std::fs::write(&p, text)?;
    Ok(())
}

/// 检查 `.gitignore` 是否忽略了 mcp.toml。
///
/// mcp.toml 可能含 API key（远程型 MCP 的 headers、stdio 型的 env），
/// 所以和 providers.toml 一样需要隔离。
pub fn check_gitignore() -> Option<String> {
    let agents = super::super::providers::store::home_dir().join(".agents");
    if !agents.join(".git").exists() {
        return None;
    }
    let gi = agents.join(".gitignore");
    let text = std::fs::read_to_string(&gi).unwrap_or_default();
    let ignored = text.lines().any(|l| {
        let t = l.trim();
        t == "mcp.toml" || t == "/mcp.toml"
    });
    if ignored {
        None
    } else {
        Some(
            "~/.agents/mcp.toml 可能含 API key（远程 MCP 的 headers、stdio 的 env），但 .gitignore 里没有忽略它。请加一行 `mcp.toml`。"
                .to_string(),
        )
    }
}

/// 示例母版。
pub fn example_content() -> String {
    r#"# MCP server 统一管理母版
#
# 这里只放你主动维护的 server。各 agent 内置的、市场里的连接器不会被碰。
# 如果某个 server 的 env 或 headers 里有 API key，记得把 mcp.toml 加进 .gitignore。

# ---- stdio 型：本地进程 ----
[[server]]
id = "context7"
name = "Context7"
transport = "stdio"
command = "cmd"
args = ["/c", "npx", "-y", "@upstash/context7-mcp"]
enabled = true
agents = ["claude", "codex", "pi", "opencode"]

# ---- stdio 型带环境变量 ----
[[server]]
id = "my-server"
name = "我的服务"
transport = "stdio"
command = "node"
args = ["src/mcp.js"]
cwd = "C:/dev/my-project"
enabled = true
agents = ["claude"]
[server.env]
API_KEY = "替换成真实值"

# ---- 远程型 ----
[[server]]
id = "remote-example"
name = "远程示例"
transport = "http"
url = "https://example.com/mcp"
enabled = false
agents = ["claude", "cursor"]
[server.headers]
Authorization = "Bearer 替换成真实值"
"#
    .to_string()
}
