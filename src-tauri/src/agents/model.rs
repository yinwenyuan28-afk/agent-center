//! agent 相关的公共数据结构。
//!
//! 目前只有 [`RawEntry`]（凭证体检的临时记录）。模型元数据
//! [`ModelSpec`] 定义在 `providers/model.rs`，因为它是母版
//! `providers.toml` 的一部分。

/// 一条临时凭证记录，收集完再统一分组。
///
/// 密钥原文不在这里 —— 只有指纹、长度和末尾 4 位。
pub struct RawEntry {
    /// 哪个 agent 在用
    pub agent: String,
    /// 归一化后的服务商标识
    pub provider: String,
    /// api_key 或 oauth
    pub kind: String,
    /// 具体落在哪个文件、哪个字段
    pub location: String,
    /// 密钥指纹前 8 位
    pub fingerprint: String,
    /// 密钥字符数
    pub length: usize,
    /// 密钥末尾 4 位，帮助人辨认
    pub tail: String,
}

/// 算密钥指纹。取 SHA-256 前 8 位十六进制。
///
/// 8 位十六进制 = 32 位空间。对本地几十条凭证来说碰撞概率可以忽略，
/// 又足够短，界面上放得下。
pub fn fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    format!("{:x}", hasher.finalize()).chars().take(8).collect()
}

/// 从字符串取一个子串做展示用的「尾巴」。用于让用户认得出是哪把 key。
///
/// 只露最后 4 位。前缀通常含 provider 信息、中间部分才是熵，
/// 露尾巴足够辨认又不至于泄露。
pub fn tail(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    let t: String = chars[chars.len() - 4..].iter().collect();
    format!("…{t}")
}

/// 把各种写法的服务名归一到同一个标识。
///
/// 同一个服务在不同 agent 里名字不一样：
/// 环境变量叫 `DEEPSEEK_API_KEY`，pi 的 auth.json 里叫 `deepseek`，
/// 有的还用 base URL。不归一化的话，界面上会把同一服务拆成好几组，
/// 用户就看不到「这个服务我配了两把钥匙」这个关键事实。
pub fn normalize_provider(raw: &str) -> String {
    let low = raw.to_lowercase();

    // 环境变量名 → 服务名
    let env_map = [
        ("deepseek_api_key", "deepseek"),
        ("command_api_key", "command"),
        ("openai_api_key", "openai"),
        ("anthropic_api_key", "anthropic"),
        ("zcode_official_endpoint_token", "zcode-official"),
    ];
    for (from, to) in env_map {
        if low == from {
            return to.to_string();
        }
    }

    // base URL → 服务名。按域名片段匹配，不依赖完整 URL，
    // 因为同一家可能有多条不同路径的端点。
    let url_map = [
        ("commandcode.ai", "command"),
        ("api.minimax.cn", "minimax-cn"),
        ("minimax.chat", "minimax"),
        ("open.bigmodel.cn", "bigmodel"),
        ("zcode.z.ai", "zcode-plan"),
        ("api.z.ai", "zai"),
        ("opencode.ai", "opencode"),
        ("api.deepseek.com", "deepseek"),
        ("api.openai.com", "openai"),
        ("api.anthropic.com", "anthropic"),
        ("dashscope.aliyuncs.com", "qwen"),
        ("api.moonshot.cn", "moonshot"),
    ];
    for (frag, to) in url_map {
        if low.contains(frag) {
            return to.to_string();
        }
    }

    // 认不出来的原样返回，至少分组是对的
    raw.to_string()
}

/// 把一条凭证收进临时列表。太短的值直接丢掉（多半是空串或占位符）。
pub fn collect(
    out: &mut Vec<RawEntry>,
    agent: &str,
    provider: &str,
    kind: &str,
    location: &str,
    secret: Option<&str>,
) {
    let Some(s) = secret else { return };
    let s = s.trim();
    if s.len() < 16 {
        return;
    }
    out.push(RawEntry {
        agent: agent.to_string(),
        // 存归一化后的名字用于分组，原始写法留在 location 里给用户看
        provider: normalize_provider(provider),
        kind: kind.to_string(),
        location: location.to_string(),
        fingerprint: fingerprint(s),
        length: s.len(),
        tail: tail(s),
    });
}

/// 安全读一个 JSON 文件。不存在、读不了、格式不对都返回 None。
pub fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}
