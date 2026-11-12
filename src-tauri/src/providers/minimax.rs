//! MiniMax Code（MCode）的 provider 注入器。
//!
//! ## 按 cc-switch 移植，本机未安装未实测
//!
//! 本机没有 `~/.minimax`。照着 cc-switch 的 `mcode_config.rs` 写。
//!
//! ## 文件与结构
//!
//! `~/.minimax/config.yaml`（`MINIMAX_DATA_DIR` 或 `MAVIS_DATA_DIR`
//! 非空时改用它们）：
//!
//! ```yaml
//! custom_provider:
//!   <key>:
//!     name: "显示名"
//!     kind: "custom"                # 不是 custom 的节点归 MCode 自己管
//!     enabled: true
//!     api: "openai-completions"     # 三选一，缺省按 anthropic-messages
//!     options:
//!       baseURL: "https://..."
//!       apiKey: "sk-..."
//!     models:
//!       <modelId>:
//!         name: "..."
//!         reasoning: true
//!         modalities: { input: [...], output: [...] }
//!         limit: { context: 400000, output: 128000 }
//! ```
//!
//! ## 三条约束（cc-switch 的 validate_provider）
//!
//! 1. provider key 只允许 ASCII 字母数字和 `-` / `_`
//! 2. `options.baseURL` 必须是 http/https 且带 host
//! 3. `options.apiKey` 非空，`models` 非空
//!
//! ## 不碰 MCode 自己的账号
//!
//! `kind` 不为 `"custom"` 的节点是 MCode 的登录账号，拒绝覆盖。
//! `minimax_api` 和 `defaultModel` 也不归本工具管 —— 后者是用户在
//! MCode 里选的模型，主动改是越权。
//!
//! ## 注释会丢
//!
//! cc-switch 整文件重序列化，注释也会丢。本工具同样用标准 YAML 输出，
//! 但只做**段级文本替换**：把 `custom_provider:` 整段换掉，其他段
//! 和注释保留。这比 cc-switch 更保守。

use super::model::{DiffAction, DiffLine, FilePlan, ModelSpec, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct MinimaxInjector;

impl MinimaxInjector {
    fn dir() -> PathBuf {
        for var in ["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"] {
            if let Ok(v) = std::env::var(var) {
                let v = v.trim();
                if !v.is_empty() {
                    return PathBuf::from(v);
                }
            }
        }
        super::store::home_dir().join(".minimax")
    }

    fn config_path() -> PathBuf {
        Self::dir().join("config.yaml")
    }
}

/// 协议 → MCode 的 `api`。缺省是 `anthropic-messages`。
fn mcode_api(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChat => "openai-completions",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GoogleGenAi => "openai-completions",
    }
}

/// YAML 标量；只在真的会解析错时才加双引号。
///
/// `https://x` 里的冒号后面是 `/`，属于合法 plain scalar，不加引号。
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('#')
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~"
        );
    if needs_quote {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// 生成 `custom_provider.<id>` 这一段（缩进 2 起）。
fn provider_yaml(p: &Provider) -> String {
    let mut out = String::new();
    out.push_str(&format!("  {}:\n", yaml_scalar(&p.id)));
    out.push_str(&format!("    name: {}\n", yaml_scalar(&p.name)));
    out.push_str("    kind: \"custom\"\n");
    out.push_str("    enabled: true\n");
    out.push_str(&format!(
        "    api: {}\n",
        mcode_api(p.kind_for("minimax"))
    ));
    out.push_str("    options:\n");
    out.push_str(&format!("      baseURL: {}\n", yaml_scalar(&p.base_url)));
    out.push_str(&format!("      apiKey: {}\n", yaml_scalar(&p.api_key)));

    out.push_str("    models:\n");
    let specs: Vec<ModelSpec> = if p.models.is_empty() {
        vec![ModelSpec::new(p.id.clone())]
    } else {
        p.models.clone()
    };
    for spec in &specs {
        out.push_str(&format!("      {}:\n", yaml_scalar(&spec.id)));
        out.push_str(&format!(
            "        name: {}\n",
            yaml_scalar(spec.display_name())
        ));
        if let Some(r) = spec.reasoning {
            out.push_str(&format!("        reasoning: {r}\n"));
        }
        if !spec.input_modalities.is_empty() {
            // MCode 的 preset 只用 text / image。原样写 video / pdf 会和
            // 它自己的 modalities 枚举对不上。
            let kept = spec.input_within(&["text", "image"]);
            if !kept.is_empty() {
                out.push_str("        modalities:\n");
                out.push_str("          input:\n");
                for m in &kept {
                    out.push_str(&format!("            - {m}\n"));
                }
                out.push_str("          output:\n");
                let outputs: Vec<String> = if spec.output_modalities.is_empty() {
                    vec!["text".to_string()]
                } else {
                    spec.output_modalities.clone()
                };
                for m in &outputs {
                    out.push_str(&format!("            - {m}\n"));
                }
            }
        }
        out.push_str("        limit:\n");
        out.push_str(&format!(
            "          context: {}\n",
            spec.context_or_default()
        ));
        out.push_str(&format!(
            "          output: {}\n",
            spec.output_or_default()
        ));
    }

    out
}

/// 找顶层（缩进 0）的 `key:` 区段。
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
        if l.len() - l.trim_start().len() == 0 {
            end = i;
            break;
        }
    }
    Some((start, end))
}

impl Injector for MinimaxInjector {
    fn agent(&self) -> &'static str {
        "minimax"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "minimax"))
            .collect();
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "minimax"))
            .collect();

        let orig = std::fs::read_to_string(&path)?;
        let pre = hash_file(&path)?;

        let mut lines: Vec<String> = orig.lines().map(|s| s.to_string()).collect();
        let mut summary = Vec::new();

        // 只删/写池内条目，池外条目保留原行。
        let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let block = find_top_block(&snapshot, "custom_provider");

        // 先删除未分配的池内条目
        for p in &unassigned {
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            let Some(b) = find_top_block(&snapshot, "custom_provider") else {
                break;
            };
            if let Some((from, to)) = find_sub_entry(&snapshot, b, &p.id) {
                lines.drain(from..to);
                summary.push(format!("移除 {}（未分配给 minimax）", p.id));
            }
        }

        // 再写/覆盖已分配的
        for p in &wanted {
            let snippet = provider_yaml(p);
            let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            match find_top_block(&snapshot, "custom_provider") {
                Some(b) => {
                    if let Some((from, to)) = find_sub_entry(&snapshot, b, &p.id) {
                        drop(snapshot);
                        let new_lines: Vec<String> =
                            snippet.lines().map(|s| s.to_string()).collect();
                        lines.splice(from..to, new_lines);
                        summary.push(format!("更新 {}", p.id));
                    } else {
                        let insert_at = b.1;
                        for (offset, line) in snippet.lines().enumerate() {
                            lines.insert(insert_at + offset, line.to_string());
                        }
                        summary.push(format!("新增 {}", p.id));
                    }
                }
                None => {
                    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                        lines.push(String::new());
                    }
                    lines.push("custom_provider:".to_string());
                    for line in snippet.lines() {
                        lines.push(line.to_string());
                    }
                    summary.push(format!("创建 custom_provider 段并新增 {}", p.id));
                }
            }
        }

        // 整体比对：只有最终文本和原文不同才算改动。
        // 「写入就置 changed」会让第二次同步先删后写同样的内容，
        // 永远报"有改动"，幂等就废了。
        let new_text = {
            let mut text = lines.join("\n");
            if orig.ends_with('\n') || !text.ends_with('\n') {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            text
        };
        let changed = new_text != orig;
        let content = if changed {
            new_text
        } else {
            summary.clear();
            summary.push("已是最新".to_string());
            orig
        };

        let _ = block;
        Ok(vec![FilePlan {
            path: contract_tilde(&path),
            agent: "minimax".to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }])
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        Ok(plans
            .into_iter()
            .map(|plan| DiffLine {
                agent: "minimax".to_string(),
                path: plan.path.clone(),
                action: if plan.changed {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                },
                target: "custom_provider".to_string(),
                detail: plan.summary.join("；"),
            })
            .collect())
    }
}

/// 在 `custom_provider:` 段里找某个 id 的二级键，返回整块行范围。
fn find_sub_entry(
    lines: &[&str],
    block: (usize, usize),
    id: &str,
) -> Option<(usize, usize)> {
    let (start, end) = block;
    let from = (start + 1..end).find(|&i| {
        let indent = lines[i].len() - lines[i].trim_start().len();
        indent == 2 && lines[i].trim_start().starts_with(&format!("{id}:"))
    })?;

    let mut stop = end;
    for i in from + 1..end {
        if lines[i].trim().is_empty() {
            continue;
        }
        if lines[i].len() - lines[i].trim_start().len() <= 2 {
            stop = i;
            break;
        }
    }
    Some((from, stop))
}

/// 供测试用：暴露协议映射。
pub fn api_of(kind: ProviderKind) -> &'static str {
    mcode_api(kind)
}
