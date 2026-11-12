//! Hermes Agent 的 provider 注入器。
//!
//! 目标是 `$HERMES_HOME/config.yaml`（默认 `~/.hermes`）的
//! **`custom_providers:` 列表**。实测 + cc-switch 核对后的结构：
//!
//! ```yaml
//! model:
//!   provider: command          # 当前在用，本工具只在它指向被删条目时纠正
//!   default: deepseek-v4-flash
//! custom_providers:
//!   - name: command
//!     base_url: https://api.commandcode.ai/provider/v1
//!     api_key: sk-xxx
//!     api_mode: chat_completions
//!     model: deepseek/deepseek-v4.1-flash
//!     models:
//!       deepseek/deepseek-v4.1-flash:
//!         context_length: 1000000
//!         supports_tools: true
//! providers:                   # v12+ 的字典形，Hermes 自己持有，只读
//!   ...
//! ```
//!
//! ## 为什么写 `custom_providers` 而不是 `providers`
//!
//! v12 之前 `providers:` 是可写的字典；v12 起 Hermes 自己接管了这个
//! 字典（Web UI 里配的账号）。cc-switch 的代码明确把它当只读、写入时
//! 拒绝，并提示用户去 Hermes Web UI 改。
//!
//! 早期实现写的是 `providers:` 字典 —— 在 v12 上会被 Hermes 覆盖，
//! 也会破坏用户在 Web UI 里的配置。现在改为 `custom_providers:` 列表，
//! `providers:` 一概不碰。
//!
//! ## 为什么不引 YAML 库
//!
//! `config.yaml` 里有几十行注释（安全说明、fallback 模型清单）和
//! `skills.disabled` 这种长数组。任何 YAML 库「解析成结构体再序列化」
//! 都会把这些全丢掉。所以做**行级文本编辑**：只动 `custom_providers:`
//! 段。cc-switch 也是段级替换（保留未涉及的段和注释）。

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
    ///
    /// 和 cc-switch 的差异：cc-switch 在 Windows 上默认读
    /// `%LOCALAPPDATA%\hermes`，这里保持 `~/.hermes` —— 本机实测
    /// 用的就是 `HERMES_HOME` 显式指定，两种默认值都到不了。
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
/// cc-switch 的前端约束是四档：`chat_completions`、`anthropic_messages`、
/// `codex_responses`、`bedrock_converse`。本工具只表达前三个（没有
/// Bedrock 这一档），其中 Responses 对应 `codex_responses`。
///
/// 注意和早期实现的差异：以前把 Anthropic 也写成 `chat_completions`，
/// 那会让 Hermes 用 OpenAI 格式请求 Anthropic 端点。
fn api_mode(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic_messages",
        ProviderKind::OpenaiResponses => "codex_responses",
        ProviderKind::OpenaiChat | ProviderKind::GoogleGenAi => "chat_completions",
    }
}

/// 找顶层（缩进 0）的 `key:` 区段，返回 `(起始行, 结束行)`。
fn find_top_block(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| {
        let t = l.trim_end();
        t == format!("{key}:") || t.starts_with(&format!("{key}:"))
    })?;

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

/// 生成一个 provider 的列表项（YAML 序列，缩进 2/4/6）。
///
/// 字段顺序和 cc-switch 写出来的一致：`name` / `base_url` / `api_key` /
/// `api_mode` / `model` / `models`。`api_key` 紧跟 `base_url` 是因为
/// 这两者都属于"连接"信息，人读的时候挨着。
fn provider_yaml(p: &Provider) -> String {
    let mut out = String::new();
    out.push_str(&format!("  - name: {}\n", yaml_scalar(&p.id)));
    out.push_str(&format!("    base_url: {}\n", yaml_scalar(&p.base_url)));
    out.push_str(&format!("    api_key: {}\n", yaml_scalar(&p.api_key)));
    out.push_str(&format!(
        "    api_mode: {}\n",
        api_mode(p.kind_for("hermes"))
    ));

    if let Some(first) = p.models.first() {
        out.push_str(&format!("    model: {}\n", yaml_scalar(&first.id)));
    }

    if !p.models.is_empty() {
        out.push_str("    models:\n");
        for spec in &p.models {
            out.push_str(&format!("      {}:\n", yaml_scalar(&spec.id)));
            if let Some(ctx) = spec.context_window {
                out.push_str(&format!("        context_length: {ctx}\n"));
            }
            if let Some(out_tokens) = spec.max_output_tokens {
                out.push_str(&format!("        max_tokens: {out_tokens}\n"));
            }
            if let Some(tc) = spec.tool_call {
                out.push_str(&format!("        supports_tools: {tc}\n"));
            }
        }
    }

    out
}

/// YAML 标量。只在真的会解析错时才加引号。
///
/// YAML 的 plain scalar 只对「`: ` 后跟空格」敏感，`https://x` 里的冒号
/// 后面是 `/`，是合法 plain scalar。早期实现见到任何 `:` 都加引号，
/// 结果所有 URL 都被写成 `"https://..."`，和用户手写的文件不一致。
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('-')
        || s.starts_with('*')
        || s.starts_with('&')
        || s.starts_with('!')
        || s.starts_with('{')
        || s.starts_with('[')
        || s.starts_with('#')
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
        );
    if needs_quote {
        // 双引号转义反斜杠和双引号本身
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// 判断一行是不是 `custom_providers:` 列表里某一项的开始。
///
/// 序列项形如 `  - name: xxx`；缩进 2 + `- `。
fn is_list_item(line: &str) -> bool {
    let indent = line.len() - line.trim_start().len();
    indent == 2 && line.trim_start().starts_with("- ")
}

/// 从列表项的 `- name:` 行读出名字。
fn item_name(line: &str) -> Option<String> {
    let t = line.trim_start().strip_prefix("- ")?;
    let rest = t.strip_prefix("name:")?;
    Some(unquote(rest.trim()))
}

/// 去掉 YAML 引号（只处理本工具会写的那两种）。
fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return t[1..t.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        return t[1..t.len() - 1].replace("''", "'");
    }
    t.to_string()
}

/// 在 `custom_providers:` 段内找某个 name 的列表项，返回整项的行范围。
fn find_item_range(lines: &[&str], block: (usize, usize), name: &str) -> Option<(usize, usize)> {
    let (start, end) = block;
    let from = (start + 1..end).find(|&i| {
        is_list_item(lines[i]) && item_name(lines[i]).as_deref() == Some(name)
    })?;

    let mut stop = end;
    for i in from + 1..end {
        if lines[i].trim().is_empty() {
            continue;
        }
        // 下一个列表项开始 → 本项结束
        if is_list_item(lines[i]) {
            stop = i;
            break;
        }
        // 回到顶层或 outer 层级 → 本项结束
        if lines[i].len() - lines[i].trim_start().len() <= 2
            && !lines[i].trim_start().starts_with("- ")
        {
            stop = i;
            break;
        }
    }
    Some((from, stop))
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

        // ---- 1. 先删未分配的（按 name 匹配，只删母版里认得出的）----
        for p in &unassigned {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            let Some(block) = find_top_block(&snapshot, "custom_providers") else {
                break;
            };
            if let Some((from, to)) = find_item_range(&snapshot, block, &p.id) {
                lines.drain(from..to);
                summary.push(format!("移除 provider {}（未分配给 hermes）", p.id));
                changed = true;
            }
        }

        // ---- 2. 写入已分配的 ----
        for p in &wanted {
            let snippet = provider_yaml(p);
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();

            match find_top_block(&snapshot, "custom_providers") {
                Some(block) => {
                    if let Some((from, to)) = find_item_range(&snapshot, block, &p.id) {
                        // 整项替换。字段归本工具管，不再逐字段合并 ——
                        // 旧实现看到已存在就跳过，结果母版改了参数永远不生效。
                        let new_lines: Vec<String> =
                            snippet.lines().map(|s| s.to_string()).collect();
                        drop(snapshot);
                        lines.splice(from..to, new_lines);
                        summary.push(format!("更新 provider {}", p.id));
                    } else {
                        // 追加到列表末尾（block.1 是下一个顶层键或文件末尾）
                        let insert_at = block.1;
                        for (offset, line) in snippet.lines().enumerate() {
                            lines.insert(insert_at + offset, line.to_string());
                        }
                        summary.push(format!("新增 provider {}", p.id));
                    }
                    changed = true;
                }
                None => {
                    // 没有 custom_providers 段：在文件末尾追加。
                    // 先补一个空行，避免和上一段的最后一行粘连。
                    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push("custom_providers:".to_string());
                    for line in snippet.lines() {
                        lines.push(line.to_string());
                    }
                    summary.push(format!("创建 custom_providers 段并新增 {}", p.id));
                    changed = true;
                }
            }
        }

        // ---- 3. 只在 model.provider 指向被删条目时纠正 ----
        //
        // `model:` 段是用户当前在用的选择，主动替用户切换是越权。
        if let Some(first) = wanted.first() {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_top_block(&snapshot, "model") {
                let current = (b.0 + 1..b.1).find_map(|i| {
                    lines[i]
                        .trim_start()
                        .strip_prefix("provider:")
                        .map(|v| unquote(v.trim()))
                });
                if let Some(cur) = current {
                    if unassigned.iter().any(|p| p.id == cur) {
                        if let Some(i) = (b.0 + 1..b.1).find(|&i| {
                            lines[i].trim_start().starts_with("provider:")
                        }) {
                            let indent = lines[i].len() - lines[i].trim_start().len();
                            lines[i] = format!("{}provider: {}", " ".repeat(indent), first.id);
                            changed = true;
                            summary.push(format!("model.provider → {}（原指向已取消分配的 {cur}）", first.id));
                        }
                    }
                }
            }
        }

        let content = if changed {
            let mut text = lines.join("\n");
            if orig.ends_with('\n') || !text.ends_with('\n') {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            text
        } else {
            if summary.is_empty() {
                summary.push("已是最新".to_string());
            }
            orig
        };

        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: self.agent().to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
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
    if let Some(max) = spec.max_output_tokens {
        out.push(("max_tokens", max.to_string()));
    }
    out.push((
        "supports_tools",
        spec.tool_call.unwrap_or(true).to_string(),
    ));
    out
}
