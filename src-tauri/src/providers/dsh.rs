//! DSH（DeepSeek Harness）的 provider 注入器。
//!
//! 涉及两个文件：
//!
//! ```yaml
//! # ~/.dsh/profiles/desktop/cordis.patch.yml
//! # 顶层是 patch-entry 数组，providers 嵌在 llm-pi-ai 条目的 config 下：
//! - id: llm-pi-ai
//!   name: "@deepseek-ai/dsh-llm-pi-ai"
//!   config:
//!     providers:
//!       command:
//!         displayName: CommandCode
//!         apiKeyEnv: COMMAND_API_KEY
//!         api: openai-completions
//!         baseURL: https://...
//!         models:
//!           - id: ...
//!             name: ...
//!             contextWindow: 1000000
//!             maxTokens: 128000
//!             input: [text]
//!
//! # ~/.dsh/.credentials.yaml
//! version: 1
//! refs:
//!   COMMAND_API_KEY: ...
//! ```
//!
//! ## 写哪些 profile
//!
//! cc-switch 写四个 profile：`web`、`desktop`、`headless`、`acp`
//! （`sdk` / `sdk-minimal` 属嵌入用途，不写）。其中 `web` / `desktop`
//! 总是写，`headless` / `acp` 只在目录已存在时写。
//!
//! 早期实现只写 `desktop`，其它 profile 里的 provider 列表会和
//! `desktop` 不一致 —— DSH 各 profile 共用一份 `.credentials.yaml`，
//! 但每个 profile 有自己的 `cordis.patch.yml`。
//!
//! ## 为什么不引 YAML 库
//!
//! YAML 库（serde_yaml / yaml-rust）都会「解析成结构体再序列化回去」，
//! 那样注释、缩进风格、键顺序全丢。`cordis.patch.yml` 是用户手写的补丁文件。
//!
//! 所以这里做**行级文本编辑**：只替换/插入目标行，其余原样保留。
//!
//! ## 历史坑
//!
//! 早期实现按「顶层 `providers:`」写，会在数组文档末尾追加非法节点，
//! 导致 DSH 启动 YAMLException。真实结构是缩进 4 的嵌套 `providers:`。

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct DshInjector;

const LLM_PI_AI_ID: &str = "llm-pi-ai";
const LLM_PI_AI_NAME: &str = "@deepseek-ai/dsh-llm-pi-ai";

impl DshInjector {
    /// 家目录。优先 `DSH_HOME`，否则 `~/.dsh`（和 cc-switch 一致）。
    fn home() -> PathBuf {
        if let Ok(v) = std::env::var("DSH_HOME") {
            let v = v.trim();
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
        super::store::home_dir().join(".dsh")
    }

    /// 要写的 profile 列表。
    ///
    /// `web` / `desktop` 总是写（目录不存在就创建），`headless` / `acp`
    /// 只在已存在时写 —— 没在用这些 profile 的机器不该被凭空造目录。
    /// `sdk` / `sdk-minimal` 是嵌入用途，DSH 自己托管，不写。
    fn target_profile_dirs() -> Vec<PathBuf> {
        let home = Self::home();
        let profiles = home.join("profiles");
        let mut out = Vec::new();
        for name in ["web", "desktop", "headless", "acp"] {
            let dir = profiles.join(name);
            if dir.exists() || matches!(name, "web" | "desktop") {
                out.push(dir);
            }
        }
        out
    }

    fn patch_path_for(dir: &std::path::Path) -> PathBuf {
        dir.join("cordis.patch.yml")
    }

    fn cred_path() -> PathBuf {
        Self::home().join(".credentials.yaml")
    }
}

fn env_var_name(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s = "PROVIDER".to_string();
    }
    format!("{s}_API_KEY")
}

fn dsh_api(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic-messages",
        ProviderKind::OpenaiChat => "openai-completions",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GoogleGenAi => "google-generative-ai",
    }
}

fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// 找顶层 patch entry：`- id: xxx`。
fn find_entry_range(lines: &[&str], entry_id: &str) -> Option<(usize, usize)> {
    let needle = format!("id: {entry_id}");
    let start = lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with("- ") && t[2..].trim_start().starts_with(&needle)
    })?;

    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.trim().is_empty() {
            continue;
        }
        // 下一个顶层数组项
        if l.starts_with("- ") {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// 在指定范围内找缩进为 `indent` 的 `key:` 区段。
fn find_indented_block(
    lines: &[&str],
    range: (usize, usize),
    key: &str,
    indent: usize,
) -> Option<(usize, usize)> {
    let (from, to) = range;
    let start = (from..to).find(|&i| {
        let l = lines[i];
        if line_indent(l) != indent {
            return false;
        }
        let t = l.trim_start().trim_end();
        t == format!("{key}:") || t.starts_with(&format!("{key}:"))
    })?;

    let mut end = to;
    for i in start + 1..to {
        let l = lines[i];
        if l.trim().is_empty() {
            continue;
        }
        if line_indent(l) <= indent {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// 找顶层（缩进 0）的 `key:` 区段，供 credentials.yaml 使用。
fn find_top_block(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    find_indented_block(lines, (0, lines.len()), key, 0)
}

/// 生成嵌套在 `config.providers` 下的 provider YAML（缩进 6/8/10...）。
fn provider_yaml(id: &str, p: &Provider) -> String {
    let env = env_var_name(id);
    let mut out = String::new();

    out.push_str(&format!("      {id}:\n"));
    out.push_str(&format!("        displayName: {}\n", yaml_scalar(&p.name)));
    out.push_str(&format!("        apiKeyEnv: {env}\n"));
    out.push_str(&format!("        api: {}\n", dsh_api(p.kind_for("dsh"))));
    out.push_str(&format!("        baseURL: {}\n", p.base_url));

    if !p.models.is_empty() {
        out.push_str("        models:\n");
        for spec in &p.models {
            out.push_str(&format!("          - id: {}\n", spec.id));
            out.push_str(&format!(
                "            name: {}\n",
                yaml_scalar(&spec.display_name())
            ));
            if let Some(ctx) = spec.context_window {
                out.push_str(&format!("            contextWindow: {ctx}\n"));
            }
            if let Some(max) = spec.max_output_tokens {
                out.push_str(&format!("            maxTokens: {max}\n"));
            }
            if !spec.input_modalities.is_empty() {
                // DSH 的 llm-pi-ai 只认 text / image（和 Pi 同一套 schema）。
                // 原样写 video / pdf 会让这条模型被拒，和 cc-switch 的行为不一致。
                let kept = llm_pi_ai_modalities(&spec.input_modalities);
                if !kept.is_empty() {
                    out.push_str("            input:\n");
                    for m in &kept {
                        out.push_str(&format!("              - {m}\n"));
                    }
                }
            }
            if !spec.reasoning_efforts.is_empty() {
                out.push_str("            reasoningEfforts:\n");
                for level in ["off", "low", "medium", "high"] {
                    let val = if spec.reasoning_efforts.iter().any(|e| e == level) {
                        level.to_string()
                    } else {
                        "null".to_string()
                    };
                    out.push_str(&format!("              {level}: {val}\n"));
                }
            }
        }
    } else {
        out.push_str(&format!(
            "        models:\n          - id: {id}\n            name: {}\n",
            yaml_scalar(&p.name)
        ));
    }

    out
}

fn yaml_scalar(s: &str) -> String {
    if s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('#')
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// 收敛模型输入模态到 DSH 的 `llm-pi-ai` 认识的两种。
///
/// DSH 这一层和 Pi 共用同一套模型 schema，`input` 只接受 `text` / `image`。
/// cc-switch 在 `dsh_config.rs` 里做同样的过滤，并且**全部被过滤掉时
/// 整条 `input` 字段都不写** —— 所以这里返回空数组表示「不写」。
fn llm_pi_ai_modalities(input: &[String]) -> Vec<String> {
    input
        .iter()
        .filter(|m| *m == "text" || *m == "image")
        .cloned()
        .collect()
}

fn has_provider(lines: &[&str], block: (usize, usize), id: &str) -> Option<usize> {
    let (start, end) = block;
    for i in start + 1..end {
        let l = lines[i];
        // providers 下二级键缩进 6
        if line_indent(l) == 6 && l.trim_start().starts_with(&format!("{id}:")) {
            return Some(i);
        }
    }
    None
}

fn remove_provider_block(lines: &mut Vec<String>, block: (usize, usize), id: &str) -> usize {
    let (start, end) = block;
    let Some(key_line) = (start + 1..end).find(|&i| {
        let l = &lines[i];
        line_indent(l) == 6 && l.trim_start().starts_with(&format!("{id}:"))
    }) else {
        return 0;
    };

    let mut stop = end;
    for i in key_line + 1..end {
        let l = &lines[i];
        if l.trim().is_empty() {
            continue;
        }
        if line_indent(l) <= 6 {
            stop = i;
            break;
        }
    }

    let removed = stop - key_line;
    lines.drain(key_line..stop);
    removed
}

/// 删掉文件末尾误写的顶层 `providers:` 残骸（历史 bug）。
fn strip_illegal_top_level_providers(lines: &mut Vec<String>) -> bool {
    let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    // 顶层数组文档里，合法内容都是 `- ...` 或注释/空行。
    // 若出现缩进 0 的 `providers:`，整段都是非法的，删掉。
    let Some(start) = snapshot.iter().position(|l| {
        let t = l.trim_end();
        *l == "providers:" || t == "providers:"
    }) else {
        return false;
    };
    // 确认不是嵌套（缩进应为 0）
    if line_indent(lines[start].as_str()) != 0 {
        return false;
    }
    let mut end = lines.len();
    for i in start + 1..lines.len() {
        let l = &lines[i];
        if l.trim().is_empty() {
            continue;
        }
        if line_indent(l) == 0 && l.starts_with("- ") {
            end = i;
            break;
        }
        if line_indent(l) == 0 && !l.trim_start().starts_with('#') {
            // 继续吞掉同层非法键，直到文件结束或下一个数组项
            continue;
        }
    }
    lines.drain(start..end);
    // 去掉末尾多余空行
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    true
}

/// 确保存在 llm-pi-ai 条目，并返回其范围。
fn ensure_llm_pi_ai_entry(lines: &mut Vec<String>) -> (usize, usize) {
    let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    if let Some(r) = find_entry_range(&snapshot, LLM_PI_AI_ID) {
        return r;
    }

    if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
        lines.push(String::new());
    }
    let start = lines.len();
    lines.push(format!("- id: {LLM_PI_AI_ID}"));
    lines.push(format!("  name: \"{LLM_PI_AI_NAME}\""));
    lines.push("  config:".to_string());
    lines.push("    providers:".to_string());
    (start, lines.len())
}

/// 在 llm-pi-ai 条目内确保 `config.providers` 存在，返回 providers 区段范围。
fn ensure_providers_block(lines: &mut Vec<String>, entry: (usize, usize)) -> (usize, usize) {
    let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();

    if let Some(b) = find_indented_block(&snapshot, entry, "providers", 4) {
        return b;
    }

    // 没有 providers：在 entry 末尾插入 config/providers
    // 若已有 config:，插到 config 段末；否则补 config + providers
    let snapshot: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    if let Some((_cfg_start, cfg_end)) = find_indented_block(&snapshot, entry, "config", 2) {
        lines.insert(cfg_end, "    providers:".to_string());
        (cfg_end, cfg_end + 1)
    } else {
        let insert_at = entry.1;
        lines.insert(insert_at, "  config:".to_string());
        lines.insert(insert_at + 1, "    providers:".to_string());
        (insert_at + 1, insert_at + 2)
    }
}

impl Injector for DshInjector {
    fn agent(&self) -> &'static str {
        "dsh"
    }

    fn targets(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Self::target_profile_dirs()
            .iter()
            .map(|d| Self::patch_path_for(d))
            .collect();
        out.push(Self::cred_path());
        out
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // DSH 没装就整体跳过
        if !Self::home().exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "dsh"))
            .collect();

        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "dsh"))
            .collect();

        let mut plans = Vec::new();

        // ---- 每个 profile 的 cordis.patch.yml ----
        //
        // 各 profile 共用一份 .credentials.yaml，但 patch 文件各自独立。
        // 只写 desktop 会让其它 profile 的 provider 列表和 desktop 不一致。
        for dir in Self::target_profile_dirs() {
            let patch_path = Self::patch_path_for(&dir);
            let profile = dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            plans.push(plan_profile(&patch_path, &profile, &wanted, &unassigned)?);
        }

        // ---- .credentials.yaml（各 profile 共用） ----
        let cred_path = Self::cred_path();
        let cred_orig = if cred_path.exists() {
            std::fs::read_to_string(&cred_path)?
        } else {
            "version: 1\nrefs:\n".to_string()
        };
        let cred_pre = hash_file(&cred_path)?;

        let mut cred_lines: Vec<String> = cred_orig.lines().map(|s| s.to_string()).collect();
        let mut cred_summary = Vec::new();
        let mut cred_changed = false;

        for p in &unassigned {
            let env = env_var_name(&p.id);
            let snapshot: Vec<&str> = cred_lines.iter().map(|s| s.as_str()).collect();
            if let Some(b) = find_top_block(&snapshot, "refs") {
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

            if let Some(b) = find_top_block(&snapshot, "refs") {
                let existing = (b.0 + 1..b.1).find(|&i| {
                    let l = cred_lines[i].trim_start();
                    l.starts_with(&format!("{env}:"))
                });

                match existing {
                    Some(i) => {
                        let indent = line_indent(&cred_lines[i]);
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
                } else if detail.contains("新增") || detail.contains("创建") {
                    DiffAction::Add
                } else {
                    DiffAction::Update
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

/// 单个 profile 的 `cordis.patch.yml` 计划。
///
/// 从 [`DshInjector::plan`] 里拆出来，是因为要写多个 profile，
/// 每个文件的编辑流程完全一致，只有路径和报告里的 profile 名不同。
fn plan_profile(
    patch_path: &PathBuf,
    profile: &str,
    wanted: &[&Provider],
    unassigned: &[&Provider],
) -> anyhow::Result<FilePlan> {
    let patch_orig = if patch_path.exists() {
        std::fs::read_to_string(patch_path)?
    } else {
        String::new()
    };
    let patch_pre = hash_file(patch_path)?;

    let mut patch_lines: Vec<String> = patch_orig.lines().map(|s| s.to_string()).collect();
    let mut patch_summary = Vec::new();
    let mut patch_changed = false;

    if strip_illegal_top_level_providers(&mut patch_lines) {
        patch_summary.push(format!("[{profile}] 清理非法顶层 providers 段"));
        patch_changed = true;
    }

    // 只有存在想写/想删的池内 provider 时，才确保 llm-pi-ai 结构
    if !wanted.is_empty() || !unassigned.is_empty() {
        let entry = ensure_llm_pi_ai_entry(&mut patch_lines);
        let before_len = patch_lines.len();
        let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
        let entry = find_entry_range(&snapshot, LLM_PI_AI_ID).unwrap_or(entry);
        drop(snapshot);
        let _providers_block = ensure_providers_block(&mut patch_lines, entry);
        if patch_lines.len() != before_len {
            patch_changed = true;
            patch_summary.push(format!("[{profile}] 确保 llm-pi-ai.config.providers 存在"));
        }
    }

    for p in unassigned {
        let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
        let Some(entry) = find_entry_range(&snapshot, LLM_PI_AI_ID) else {
            continue;
        };
        let Some(b) = find_indented_block(&snapshot, entry, "providers", 4) else {
            continue;
        };
        let removed = remove_provider_block(&mut patch_lines, b, &p.id);
        if removed > 0 {
            patch_summary.push(format!("[{profile}] 移除 provider {}（未分配给 dsh）", p.id));
            patch_changed = true;
        }
    }

    for p in wanted {
        let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
        let entry = find_entry_range(&snapshot, LLM_PI_AI_ID)
            .ok_or_else(|| anyhow::anyhow!("{profile}: 找不到 llm-pi-ai 条目"))?;
        let b = find_indented_block(&snapshot, entry, "providers", 4)
            .ok_or_else(|| anyhow::anyhow!("{profile}: 找不到 llm-pi-ai.config.providers"))?;

        if has_provider(&snapshot, b, &p.id).is_some() {
            // 已有：整块替换，保证 baseURL/api/models 与母版一致
            let removed = remove_provider_block(&mut patch_lines, b, &p.id);
            let snapshot: Vec<&str> = patch_lines.iter().map(|s| s.as_str()).collect();
            let entry = find_entry_range(&snapshot, LLM_PI_AI_ID).unwrap();
            let b = find_indented_block(&snapshot, entry, "providers", 4).unwrap();
            let insert_at = b.1;
            let snippet = provider_yaml(&p.id, p);
            for (offset, line) in snippet.lines().enumerate() {
                patch_lines.insert(insert_at + offset, line.to_string());
            }
            let verb = if removed > 0 { "更新" } else { "新增" };
            patch_summary.push(format!("[{profile}] {verb} provider {}", p.id));
            patch_changed = true;
        } else {
            let insert_at = b.1;
            let snippet = provider_yaml(&p.id, p);
            for (offset, line) in snippet.lines().enumerate() {
                patch_lines.insert(insert_at + offset, line.to_string());
            }
            patch_summary.push(format!("[{profile}] 新增 provider {}", p.id));
            patch_changed = true;
        }
    }

    let content = if patch_changed {
        let mut text = patch_lines.join("\n");
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    } else {
        if patch_summary.is_empty() {
            patch_summary.push(format!("[{profile}] 已是最新"));
        }
        patch_orig
    };

    Ok(FilePlan {
        path: contract_tilde(patch_path),
        agent: "dsh".to_string(),
        content,
        pre_hash: patch_pre,
        post_hash: String::new(),
        changed: patch_changed,
        summary: patch_summary,
    })
}
