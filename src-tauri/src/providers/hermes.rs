//! Hermes Agent 的 provider 注入器。
//!
//! 目标是 `$HERMES_HOME/config.yaml`（默认 `~/.hermes/config.yaml`）的
//! `providers` 段。实测结构：
//!
//! ```yaml
//! model:
//!   provider: opencode-go
//!   default: deepseek-v4-flash
//!   base_url: https://opencode.ai/zen/go/v1
//!   api_mode: chat_completions
//!   api_key: sk-xxx
//! providers:
//!   opencode-go:
//!     name: OpenCode Go
//!     base_url: https://opencode.ai/zen/go/v1
//!     api_mode: chat_completions
//!     transport: openai_chat
//!     model: deepseek-v4-flash
//!     models:
//!       glm-5.2:
//!         context_length: 204800
//!         supports_tools: true
//!     api_key: sk-xxx
//! ```
//!
//! ## 为什么不引 YAML 库
//!
//! `config.yaml` 里有 40 行注释（安全说明、fallback 模型清单）和
//! `skills.disabled` 这种长数组。任何 YAML 库「解析成结构体再序列化」
//! 都会把这些全丢掉。所以做**行级文本编辑**：只动 `providers` 段下的目标行。
//!
//! ## 字段映射
//!
//! | 本工具 | Hermes |
//! |---|---|
//! | Anthropic / OpenAI Chat / Google | `api_mode: chat_completions`, `transport: openai_chat` |
//! | OpenAI Responses | `api_mode: responses`, `transport: openai_chat` |
//! | ModelSpec.context_window | `models.<id>.context_length` |
//! | ModelSpec.tool_call | `models.<id>.supports_tools` |

use super::model::ModelSpec;
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use crate::providers::model::{FilePlan, Provider, ProviderKind};
use crate::providers::store::home_dir;
use std::path::PathBuf;

pub struct HermesInjector;

impl HermesInjector {
    /// Hermes 的 home。优先读环境变量，因为它允许装在任意位置。
    fn home() -> PathBuf {
        if let Ok(v) = std::env::var("HERMES_HOME") {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
        home_dir().join(".hermes")
    }

    pub fn config_path() -> PathBuf {
        Self::home().join("config.yaml")
    }
}

/// Hermes 的 `api_mode` 词表。
///
/// 实测只有两种：`chat_completions` 和 `responses`。
/// Anthropic 和 Google 的原生协议它也走 OpenAI 兼容层（`transport: openai_chat`）。
fn api_mode(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenaiResponses => "responses",
        _ => "chat_completions",
    }
}

/// 在 YAML 文本里找某个顶层键所在区段。
///
/// 返回 `(该键的行号, 区段结束行号)`。缩进 0 的键才算顶层。
fn find_block(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|l| l.trim_end() == format!("{key}:") || l.starts_with(&format!("{key}:")))?;

    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim().is_empty() {
            continue;
        }
        let indent = l.len() - l.trim_start().len();
        if indent == 0 {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// 生成一个 provider 的 YAML 片段（缩进 2 起）。
fn provider_yaml(id: &str, p: &Provider) -> String {
    let mut out = String::new();
    out.push_str(&format!("  {id}:\n"));
    out.push_str(&format!("    name: {}\n", p.name));
    out.push_str(&format!("    base_url: {}\n", p.base_url));
    out.push_str(&format!("    api_mode: {}\n", api_mode(p.kind_for("hermes"))));
    out.push_str("    transport: openai_chat\n");

    if let Some(first) = p.models.first() {
        out.push_str(&format!("    model: {}\n", first.id));
    }

    if !p.models.is_empty() {
        out.push_str("    models:\n");
        for spec in &p.models {
            out.push_str(&format!("      {}:\n", spec.id));
            if let Some(ctx) = spec.context_window {
                out.push_str(&format!("        context_length: {ctx}\n"));
            }
            out.push_str(&format!(
                "        supports_tools: {}\n",
                spec.tool_call.unwrap_or(true)
            ));
        }
    }

    out.push_str(&format!("    api_key: {}\n", p.api_key));
    out
}

/// 删掉 `providers:` 段下的一个 provider 整块。
///
/// 边界：二级键（缩进 2）起，到下一个缩进 ≤ 2 的非空行止。
fn remove_provider_block(lines: &mut Vec<String>, block: (usize, usize), id: &str) -> usize {
    let (start, end) = block;
    let Some(key_line) = (start + 1..end).find(|&i| {
        let l = &lines[i];
        let indent = l.len() - l.trim_start().len();
        indent == 2 && l.trim_start().starts_with(&format!("{id}:"))
    }) else {
        return 0;
    };

    let mut stop = end;
    for i in key_line + 1..end {
        let l = &lines[i];
        if l.trim().is_empty() {
            continue;
        }
        let indent = l.len() - l.trim_start().len();
        if indent <= 2 {
            stop = i;
            break;
        }
    }

    let removed = stop - key_line;
    lines.drain(key_line..stop);
    removed
}

/// 检查 `providers:` 段里是否已有某个 id，返回它的行号。
fn has_provider(lines: &[&str], block: (usize, usize), id: &str) -> Option<usize> {
    let (start, end) = block;
    (start + 1..end).find(|&i| {
        let l = lines[i];
        let indent = l.len() - l.trim_start().len();
        indent == 2 && l.trim_start().starts_with(&format!("{id}:"))
    })
}

impl Injector for HermesInjector {
    fn agent(&self) -> &'static str {
        "hermes"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "hermes"))
            .collect();
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "hermes"))
            .collect();

        let mut lines: Vec<String> = orig.lines().map(|s| s.to_string()).collect();
        let mut summary = Vec::new();
        let mut changed = false;

        // ---- 先移除未分配的 ----
        for p in &unassigned {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_block(&snapshot, "providers") {
                let removed = remove_provider_block(&mut lines, b, &p.id);
                if removed > 0 {
                    summary.push(format!("移除 provider {}（未分配给 hermes）", p.id));
                    changed = true;
                }
            }
        }

        // ---- 再写入已分配的 ----
        for p in &wanted {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            match find_block(&snapshot, "providers") {
                Some(b) => {
                    if has_provider(&snapshot, b, &p.id).is_some() {
                        // 已存在：Hermes 的 YAML 层级较深，逐字段行级改容易错。
                        // 这里只报告存在，需要改参数的场景请用界面直接编辑。
                        summary.push(format!(
                            "provider {} 已存在，跳过（如需改字段请手工编辑 config.yaml）",
                            p.id
                        ));
                    } else {
                        let insert_at = b.1;
                        let snippet = provider_yaml(&p.id, p);
                        for (offset, line) in snippet.lines().enumerate() {
                            lines.insert(insert_at + offset, line.to_string());
                        }
                        summary.push(format!("新增 provider {}", p.id));
                        changed = true;
                    }
                }
                None => {
                    // 没有 providers 段，追加到文件末尾
                    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push("providers:".to_string());
                    for line in provider_yaml(&p.id, p).lines() {
                        lines.push(line.to_string());
                    }
                    summary.push(format!("创建 providers 段并新增 {}", p.id));
                    changed = true;
                }
            }
        }

        // ---- model 段：指向第一个分配的 provider ----
        //
        // Hermes 用 `model.provider` + `model.default` 决定当前用哪个。
        // 这是「用户当前在用」的选择，替用户改是越权 —— 所以只在它
        // 指向一个已经被移除的 provider 时才纠正，其余不动。
        if let Some(first) = wanted.first() {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_block(&snapshot, "model") {
                let current = (b.0 + 1..b.1).find_map(|i| {
                    let t = lines[i].trim_start();
                    t.strip_prefix("provider:")
                        .map(|v| v.trim().to_string())
                });
                match current {
                    Some(cur) if unassigned.iter().any(|p| p.id == cur) => {
                        // 当前指向的 provider 被取消了，改指向第一个分配的
                        if let Some(i) = (b.0 + 1..b.1).find(|&i| {
                            lines[i].trim_start().starts_with("provider:")
                        }) {
                            let indent = lines[i].len() - lines[i].trim_start().len();
                            lines[i] = format!(
                                "{}provider: {}",
                                " ".repeat(indent),
                                first.id
                            );
                            changed = true;
                            summary.push(format!("model.provider → {}", first.id));
                        }
                    }
                    _ => {}
                }
            }
        }

        if !changed {
            return Ok(vec![FilePlan {
                path: contract_tilde(&path),
                agent: self.agent().to_string(),
                content: orig,
                pre_hash: pre,
                post_hash: String::new(),
                changed: false,
                summary,
            }]);
        }

        let mut text = lines.join("\n");
        if orig.ends_with('\n') {
            text.push('\n');
        }

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: self.agent().to_string(),
            content: text,
            pre_hash: pre,
            post_hash: String::new(),
            changed: true,
            summary,
        }])
    }
}

/// 供测试用：暴露配置路径。
pub fn config_path() -> PathBuf {
    HermesInjector::config_path()
}

/// 供测试用：暴露 api_mode 映射。
pub fn api_mode_of(kind: ProviderKind) -> &'static str {
    api_mode(kind)
}

/// 供测试用：模型规格在 Hermes 里能表达哪些字段。
pub fn model_fields(spec: &ModelSpec) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(ctx) = spec.context_window {
        out.push(("context_length", ctx.to_string()));
    }
    out.push((
        "supports_tools",
        spec.tool_call.unwrap_or(true).to_string(),
    ));
    out
}
