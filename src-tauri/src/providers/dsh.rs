//! DSH（DeepSeek Harness）的 provider 注入器。
//!
//! 涉及两个文件，结构都是 YAML：
//!
//! ```yaml
//! # ~/.dsh/profiles/desktop/cordis.patch.yml
//! providers:
//!   command:
//!     apiKeyEnv: COMMAND_API_KEY
//!     api: openai-completions
//!     baseURL: https://api.commandcode.ai/provider/v1
//!     models:
//!       - id: deepseek/deepseek-v4.1-flash
//!         name: ...
//!
//! # ~/.dsh/.credentials.yaml
//! version: 1
//! records:
//!   ...
//! refs:
//!   COMMAND_API_KEY: user_4Aufqvs...
//! ```
//!
//! ## 为什么不引 YAML 库
//!
//! YAML 库（serde_yaml / yaml-rust）都会「解析成结构体再序列化回去」，
//! 那样注释、缩进风格、键顺序全丢。而这两个文件里 `cordis.patch.yml`
//! 是用户手写的补丁文件，`cordis.yml` 可能有注释。
//!
//! 所以这里做**行级文本编辑**：只替换/插入目标行，其余原样保留。
//!
//! ## 两条路径的契约
//!
//! `cordis.patch.yml` 里写 `apiKeyEnv: XXX`，`credentials.yaml` 里必须有
//! `refs.XXX: 真实key`。两个文件必须同时改成功，否则 provider 有定义无 key
//! 或者有 key 无定义。

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct DshInjector;

impl DshInjector {
    fn profile_dir() -> PathBuf {
        super::store::home_dir()
            .join(".dsh")
            .join("profiles")
            .join("desktop")
    }

    fn patch_path() -> PathBuf {
        Self::profile_dir().join("cordis.patch.yml")
    }

    fn cred_path() -> PathBuf {
        super::store::home_dir()
            .join(".dsh")
            .join(".credentials.yaml")
    }
}

/// 把 provider id 转成环境变量名。
fn env_var_name(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    if s.is_empty() {
        s = "PROVIDER".to_string();
    }
    format!("{s}_API_KEY")
}

/// DSH 的 api 词表。
///
/// 从实测的 cordis.patch.yml 看，它用 `openai-completions` 这种写法，
/// 和 Pi 同一套词表。
fn dsh_api(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChat => "openai-completions",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GoogleGenAi => "google-generative-ai",
    }
}

/// 在 YAML 文本里找到某个顶层键所在区段的结束位置。
///
/// 返回 (该键的行号, 该区段最后一行的行号)。
fn find_block(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    // 找 `key:` 这一行（顶层，无缩进）
    let start = lines.iter().position(|l| {
        let t = l.trim_end();
        t == format!("{key}:") || t.starts_with(&format!("{key}:"))
    })?;

    // 往后找到第一个非空且缩进为 0 的行
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

/// 在 `providers:` 区段下生成一个 provider 的 YAML 片段。
fn provider_yaml(id: &str, p: &Provider) -> String {
    let env = env_var_name(id);
    let mut out = String::new();

    // providers 下的二级键，缩进 2
    out.push_str(&format!("  {id}:\n"));
    out.push_str(&format!("    apiKeyEnv: {env}\n"));
    out.push_str(&format!("    api: {}\n", dsh_api(p.kind_for("dsh"))));
    out.push_str(&format!("    baseURL: {}\n", p.base_url));

    if !p.models.is_empty() {
        out.push_str("    models:\n");
        for spec in &p.models {
            out.push_str(&format!("      - id: {}\n", spec.id));
            out.push_str(&format!("        name: {}\n", spec.display_name()));
            // DSH 的模型字段里明确支持 contextWindow 和 input
            if let Some(ctx) = spec.context_window {
                out.push_str(&format!("        contextWindow: {ctx}\n"));
            }
            if !spec.input_modalities.is_empty() {
                out.push_str("        input:\n");
                for m in &spec.input_modalities {
                    out.push_str(&format!("          - {m}\n"));
                }
            }
            // 推理档位：DSH 用 reasoningEfforts 映射，和 Pi 的 thinkingLevelMap 同构
            if !spec.reasoning_efforts.is_empty() {
                out.push_str("        reasoningEfforts:\n");
                for level in ["off", "low", "medium", "high"] {
                    let val = if spec.reasoning_efforts.iter().any(|e| e == level) {
                        level.to_string()
                    } else {
                        "null".to_string()
                    };
                    out.push_str(&format!("          {level}: {val}\n"));
                }
            }
        }
    } else {
        out.push_str(&format!("    models:\n      - id: {id}\n        name: {id}\n"));
    }

    out
}

/// 检查 `providers:` 区段里是否已有某个 id。
fn has_provider(lines: &[&str], block: (usize, usize), id: &str) -> Option<usize> {
    let (start, end) = block;
    for i in start + 1..end {
        let l = lines[i];
        let indent = l.len() - l.trim_start().len();
        // 二级键缩进 2
        if indent == 2 && l.trim_start().starts_with(&format!("{id}:")) {
            return Some(i);
        }
    }
    None
}

/// 删掉 `providers:` 区段下的一个 provider 整块。
///
/// 块的边界：二级键（缩进 2）那一行起，到下一个缩进 ≤ 2 的非空行止。
/// 返回被删掉的行数；0 表示没找到。
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

impl Injector for DshInjector {
    fn agent(&self) -> &'static str {
        "dsh"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::patch_path(), Self::cred_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let patch_path = Self::patch_path();
        let cred_path = Self::cred_path();

        if !patch_path.exists() && !cred_path.exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "dsh"))
            .collect();

        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 取消分配的：从 providers 段和 refs 段里都摘掉
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "dsh"))
            .collect();

        let mut plans = Vec::new();

        // ---- cordis.patch.yml ----
        let patch_orig = if patch_path.exists() {
            std::fs::read_to_string(&patch_path)?
        } else {
            String::new()
        };
        let patch_pre = hash_file(&patch_path)?;

        let mut patch_lines: Vec<String> =
            patch_orig.lines().map(|s| s.to_string()).collect();
        let mut patch_summary = Vec::new();
        let mut patch_changed = false;

        // 先移除未分配的 provider 块。
        // 每删一块后行号会变，所以每次重新定位 providers 段。
        for p in &unassigned {
            let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_block(&snapshot, "providers") {
                let removed = remove_provider_block(&mut patch_lines, b, &p.id);
                if removed > 0 {
                    patch_summary.push(format!("移除 provider {}（未分配给 dsh）", p.id));
                    patch_changed = true;
                }
            }
        }

        for p in &wanted {
            let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
            let block = find_block(&snapshot, "providers");

            match block {
                Some(b) => {
                    if has_provider(&snapshot, b, &p.id).is_some() {
                        // 已有该 provider：这里不做逐字段更新。
                        // 原因：YAML 行级编辑改嵌套结构很容易出错，
                        // 而 provider 的 baseURL/api 变更很少见。
                        // 只报告"已存在"，需要改请用界面直接编辑。
                        patch_summary.push(format!("provider {} 已存在，跳过（如需修改请手工编辑）", p.id));
                    } else {
                        // 在 providers 区段末尾插入
                        let insert_at = b.1;
                        let snippet = provider_yaml(&p.id, p);
                        for (offset, line) in snippet.lines().enumerate() {
                            patch_lines.insert(insert_at + offset, line.to_string());
                        }
                        patch_summary.push(format!("新增 provider {}", p.id));
                        patch_changed = true;
                    }
                }
                None => {
                    // 没有 providers 区段，追加到文件末尾
                    if !patch_lines.is_empty() && !patch_lines.last().unwrap().trim().is_empty() {
                        patch_lines.push(String::new());
                    }
                    patch_lines.push("providers:".to_string());
                    for line in provider_yaml(&p.id, p).lines() {
                        patch_lines.push(line.to_string());
                    }
                    patch_summary.push(format!("创建 providers 区段并新增 {}", p.id));
                    patch_changed = true;
                }
            }
        }

        if patch_changed {
            let mut new_text = patch_lines.join("\n");
            if patch_orig.ends_with('\n') {
                new_text.push('\n');
            }
            plans.push(FilePlan {
                path: contract_tilde(&patch_path),
                agent: "dsh".to_string(),
                content: new_text,
                pre_hash: patch_pre,
                post_hash: String::new(),
                changed: true,
                summary: patch_summary,
            });
        } else {
            plans.push(FilePlan {
                path: contract_tilde(&patch_path),
                agent: "dsh".to_string(),
                content: patch_orig,
                pre_hash: patch_pre,
                post_hash: String::new(),
                changed: false,
                summary: patch_summary,
            });
        }

        // ---- .credentials.yaml ----
        let cred_orig = if cred_path.exists() {
            std::fs::read_to_string(&cred_path)?
        } else {
            "version: 1\nrefs:\n".to_string()
        };
        let cred_pre = hash_file(&cred_path)?;

        let mut cred_lines: Vec<String> = cred_orig.lines().map(|s| s.to_string()).collect();
        let mut cred_summary = Vec::new();
        let mut cred_changed = false;

        // 先移除未分配的 refs 条目
        for p in &unassigned {
            let env = env_var_name(&p.id);
            let snapshot: Vec<&str> = cred_lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_block(&snapshot, "refs") {
                if let Some(i) = (b.0 + 1..b.1).find(|&i| {
                    cred_lines[i]
                        .trim_start()
                        .starts_with(&format!("{env}:"))
                }) {
                    cred_lines.remove(i);
                    cred_summary.push(format!("移除 {env}（未分配给 dsh）"));
                    cred_changed = true;
                }
            }
        }

        for p in &wanted {
            let env = env_var_name(&p.id);
            let snapshot: Vec<&str> = cred_lines.iter().map(|s| s.as_str()).collect();

            // 找 refs: 区段
            if let Some(b) = find_block(&snapshot, "refs") {
                // 在 refs 下找 `ENV: value` 形式的行
                let existing = (b.0 + 1..b.1).find(|&i| {
                    let l = cred_lines[i].trim_start();
                    l.starts_with(&format!("{env}:"))
                });

                match existing {
                    Some(i) => {
                        let indent = cred_lines[i].len() - cred_lines[i].trim_start().len();
                        let new_line = format!("{}{}: {}", " ".repeat(indent), env, p.api_key);
                        if cred_lines[i] != new_line {
                            cred_lines[i] = new_line;
                            cred_summary.push(format!("更新 {env}"));
                            cred_changed = true;
                        } else {
                            cred_summary.push(format!("{env} 已一致"));
                        }
                    }
                    None => {
                        let insert_at = b.1;
                        cred_lines.insert(insert_at, format!("  {env}: {}", p.api_key));
                        cred_summary.push(format!("新增 {env}"));
                        cred_changed = true;
                    }
                }
            } else {
                // 没有 refs 段
                cred_lines.push("refs:".to_string());
                cred_lines.push(format!("  {env}: {}", p.api_key));
                cred_summary.push(format!("创建 refs 段并新增 {env}"));
                cred_changed = true;
            }
        }

        let cred_content = if cred_changed {
            let mut s = cred_lines.join("\n");
            if cred_orig.ends_with('\n') || cred_orig.is_empty() {
                s.push('\n');
            }
            s
        } else {
            cred_orig
        };

        plans.push(FilePlan {
            path: contract_tilde(&cred_path),
            agent: "dsh".to_string(),
            content: cred_content,
            pre_hash: cred_pre,
            post_hash: String::new(),
            changed: cred_changed,
            summary: cred_summary,
        });

        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let mut out = Vec::new();
        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "dsh"))
            .collect();

        for plan in &plans {
            for p in &wanted {
                let detail = plan
                    .summary
                    .iter()
                    .find(|s| s.contains(&p.id) || s.contains(&env_var_name(&p.id)))
                    .cloned()
                    .unwrap_or_else(|| "无变化".to_string());

                let action = if !plan.changed {
                    DiffAction::Unchanged
                } else if detail.starts_with("新增") || detail.starts_with("创建") {
                    DiffAction::Add
                } else if detail.starts_with("更新") {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                };

                out.push(DiffLine {
                    agent: "dsh".to_string(),
                    path: plan.path.clone(),
                    action,
                    target: p.id.clone(),
                    detail,
                });
            }
        }
        Ok(out)
    }
}
