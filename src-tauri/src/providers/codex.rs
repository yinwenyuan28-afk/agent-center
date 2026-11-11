//! Codex 的 provider 注入器。
//!
//! ## 特殊性：key 不进配置文件
//!
//! Codex 的 `[model_providers.<id>]` 只有 `env_key` 字段存**环境变量名**，
//! key 本身必须从进程环境读。所以这里要做两件事：
//!
//! 1. 改 `~/.codex/config.toml`：写 provider 表 + `env_key`
//! 2. 生成 `~/.agents/codex-env.ps1`：把 key 设进环境变量
//!
//! 用户在使用 Codex 前需要 `. ~/.agents/codex-env.ps1`。
//!
//! ## 为什么不用 experimental_bearer_token
//!
//! 那个字段确实能直接把 key 写进 config.toml，一步到位。但：
//! - 字段名带 `experimental`，未来可能被移除
//! - 官方文档明确不推荐，优先 `env_key`
//! - key 写进 TOML 会被更多工具/备份覆盖，泄露面更大
//!
//! ## TOML 写入的两个坑
//!
//! 1. **`model_provider` / `model` 必须写在任何 `[table]` 之前** ——
//!    一旦落到表头之后，TOML 语义上就变成那个表的字段了
//! 2. **`toml_edit` 不保留 CRLF**，需要自己检测并恢复

use super::model::{DiffAction, DiffLine, FilePlan, Provider, ProviderKind};
use super::safe_write::hash_file;
use super::Injector;
use crate::config::contract_tilde;
use std::path::PathBuf;

pub struct CodexInjector;

impl CodexInjector {
    fn config_path() -> PathBuf {
        super::store::home_dir().join(".codex").join("config.toml")
    }

    fn env_script_path() -> PathBuf {
        super::store::home_dir()
            .join(".agents")
            .join("codex-env.ps1")
    }
}

/// 把 provider id 转成合法的环境变量名。
///
/// 只保留字母数字和下划线，其余转下划线，再补 `_API_KEY` 后缀。
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

impl Injector for CodexInjector {
    fn agent(&self) -> &'static str {
        "codex"
    }

    fn targets(&self) -> Vec<PathBuf> {
        vec![Self::config_path(), Self::env_script_path()]
    }

    fn plan(&self, providers: &[Provider]) -> anyhow::Result<Vec<FilePlan>> {
        let cfg_path = Self::config_path();
        if !cfg_path.exists() {
            return Ok(Vec::new());
        }

        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "codex"))
            .collect();

        // 母版为空时什么都不做，避免把空母版理解成"清空"
        if providers.is_empty() {
            return Ok(Vec::new());
        }

        // 取消分配的：从 model_providers 表和 env 脚本里都摘掉。
        // 按 id 匹配 —— Codex 的表键就是母版 id。
        let unassigned: Vec<&Provider> = providers
            .iter()
            .filter(|p| !p.agents.iter().any(|a| a == "codex"))
            .collect();

        // 检测原文件的行尾风格和末尾换行 —— toml_edit 都不保留
        let orig = std::fs::read_to_string(&cfg_path)?;
        let pre = hash_file(&cfg_path)?;
        let uses_crlf = orig.contains("\r\n");
        let had_trailing_newline = orig.ends_with('\n');

        let mut doc: toml_edit::DocumentMut = orig.parse().map_err(|e| {
            anyhow::anyhow!(
                "{} 解析失败（{}），已中止，不会写入任何文件",
                cfg_path.display(),
                e
            )
        })?;

        let mut summary = Vec::new();
        let mut changed = false;

        // 确保 model_providers 表存在。
        // **只在有东西要写的时候建** —— 一个 provider 都没分配给 codex 时
        // 凭空插一个空表会把文件的 hash 改掉，让"无改动"变成"写入"。
        if !doc.contains_key("model_providers") {
            if wanted.is_empty() {
                // 没事可做：不建表、不写 env 脚本
                return Ok(Vec::new());
            }
            doc["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
            changed = true;
        }

        let providers_tbl = doc["model_providers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{} 的 model_providers 不是表", cfg_path.display()))?;

        // 先移除未分配的 provider 表
        for p in &unassigned {
            if providers_tbl.remove(&p.id).is_some() {
                summary.push(format!("移除 provider 表 {}（未分配给 codex）", p.id));
                changed = true;
            }
        }

        for p in &wanted {
            let env_name = env_var_name(&p.id);

            let is_new = !providers_tbl.contains_key(&p.id);

            if is_new {
                let mut t = toml_edit::Table::new();
                t["name"] = toml_edit::value(p.name.clone());
                t["base_url"] = toml_edit::value(p.base_url.clone());
                t["env_key"] = toml_edit::value(env_name.clone());

                // wire_api：现版本 Codex 只接受 "responses"。
                // 如果母版里标的是别的协议，写 responses 的同时在 summary 里警告 ——
                // 静默写会让用户以为配置对了但实际请求格式不匹配。
                let wire = match p.kind_for("codex") {
                    ProviderKind::OpenaiResponses | ProviderKind::OpenaiChat => "responses",
                    ProviderKind::Anthropic | ProviderKind::GoogleGenAi => "responses",
                };
                t["wire_api"] = toml_edit::value(wire);
                t["requires_openai_auth"] = toml_edit::value(false);

                // 非 responses 协议给个明确提示
                if !matches!(p.kind_for("codex"), ProviderKind::OpenaiResponses) {
                    summary.push(format!(
                        "{} 的协议是 {}，但 Codex 只支持 responses，已强制写成 responses",
                        p.id,
                        p.kind_for("codex").label()
                    ));
                }

                providers_tbl.insert(&p.id, toml_edit::Item::Table(t));
                summary.push(format!("新增 provider 表 {} (env_key={env_name})", p.id));
                changed = true;
            } else {
                // 已存在：只改 name / base_url / env_key，
                // 其他字段（retries / headers 等）不动
                if let Some(t) = providers_tbl.get_mut(&p.id).and_then(|v| v.as_table_mut()) {
                    let mut local_changed = false;

                    let old_name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if old_name != p.name {
                        t["name"] = toml_edit::value(p.name.clone());
                        local_changed = true;
                    }
                    let old_base = t.get("base_url").and_then(|v| v.as_str()).unwrap_or("");
                    if old_base != p.base_url {
                        t["base_url"] = toml_edit::value(p.base_url.clone());
                        local_changed = true;
                    }
                    let old_env = t.get("env_key").and_then(|v| v.as_str()).unwrap_or("");
                    if old_env != env_name {
                        t["env_key"] = toml_edit::value(env_name.clone());
                        local_changed = true;
                    }

                    if local_changed {
                        summary.push(format!("更新 provider 表 {}", p.id));
                        changed = true;
                    } else {
                        summary.push(format!("provider 表 {} 已是最新", p.id));
                    }
                }
            }
        }

        // model_provider 指向第一个声明的 provider。
        // 注意 toml_edit 的根表赋值会把它放在文件顶部区域，
        // 但如果原文件里已经有这个键在表头之后（用户写错），
        // 需要先删再插入到正确位置。
        if let Some(first) = wanted.first() {
            let desired = first.id.clone();
            let current = doc
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if current != desired {
                doc["model_provider"] = toml_edit::value(desired.clone());
                changed = true;
                summary.push(format!("model_provider → {desired}"));
            }

            // 模型：取第一个，并写推理档位
            if let Some(model) = first.models.first() {
                let cur_model = doc.get("model").and_then(|v| v.as_str()).unwrap_or("");
                if cur_model != model.id {
                    doc["model"] = toml_edit::value(model.id.clone());
                    changed = true;
                    summary.push(format!("model → {}", model.id));
                }

                // 重构前不写推理档位。母版填了才写，没填保持原值
                if let Some(effort) = &model.default_effort {
                    let cur_effort = doc
                        .get("model_reasoning_effort")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if cur_effort != effort {
                        doc["model_reasoning_effort"] = toml_edit::value(effort.clone());
                        changed = true;
                        summary.push(format!("model_reasoning_effort → {effort}"));
                    }
                }
            }
        } else {
            // 一个都没分配给 codex：如果 model_provider 指向的正是被我们
            // 刚刚移除的那张表，得一起清掉，否则 Codex 启动会报找不到 provider
            let current = doc
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if unassigned.iter().any(|p| p.id == current) {
                doc.remove("model_provider");
                changed = true;
                summary.push(format!("清除 model_provider（{current} 已取消分配）"));
            }
        }

        // 序列化并恢复行尾风格
        let mut content = doc.to_string();
        if uses_crlf {
            content = content.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if !had_trailing_newline {
            while content.ends_with('\n') || content.ends_with("\r\n") {
                if content.ends_with("\r\n") {
                    content.truncate(content.len() - 2);
                } else {
                    content.truncate(content.len() - 1);
                }
            }
        }

        let content = if changed { content } else { orig };

        let mut plans = vec![FilePlan {
            path: contract_tilde(&cfg_path),
            agent: "codex".to_string(),
            content,
            pre_hash: pre,
            post_hash: String::new(),
            changed,
            summary,
        }];

        // ---- 生成环境变量脚本 ----
        //
        // 没有 provider 分配给 codex 时，脚本要么不建（首次），
        // 要么清成只有注释（用户刚取消全部）。清空是必要的：
        // 留着旧的 `$env:XXX_API_KEY` 会让 Codex 拿一把已经不该用的 key 去连。
        let script_path = Self::env_script_path();
        if wanted.is_empty() && !script_path.exists() {
            return Ok(plans);
        }
        let script_orig = if script_path.exists() {
            std::fs::read_to_string(&script_path)?
        } else {
            String::new()
        };
        let script_pre = hash_file(&script_path)?;

        let mut script = String::from(
            "# 由 agentcenter 生成，请勿手动编辑\n\
             # 使用 Codex 前在 PowerShell 里执行： . ~/.agents/codex-env.ps1\n\
             # 或者在 个人资料 里加上这一行\n\n",
        );
        for p in &wanted {
            // PowerShell 单引号字符串里，单引号要写成两个
            let escaped = p.api_key.replace('\'', "''");
            script.push_str(&format!(
                "$env:{} = '{}'\n",
                env_var_name(&p.id),
                escaped
            ));
        }

        let script_changed = script != script_orig;
        plans.push(FilePlan {
            path: contract_tilde(&script_path),
            agent: "codex".to_string(),
            content: script,
            pre_hash: script_pre,
            post_hash: String::new(),
            changed: script_changed,
            summary: vec![if script_changed {
                format!("生成 {} 个环境变量", wanted.len())
            } else {
                "环境变量脚本已是最新".to_string()
            }],
        });

        Ok(plans)
    }

    fn diff(&self, providers: &[Provider]) -> anyhow::Result<Vec<DiffLine>> {
        let plans = self.plan(providers)?;
        let wanted: Vec<&Provider> = providers
            .iter()
            .filter(|p| p.agents.iter().any(|a| a == "codex"))
            .collect();

        let mut out = Vec::new();
        for plan in &plans {
            if plan.path.ends_with("codex-env.ps1") {
                out.push(DiffLine {
                    agent: "codex".to_string(),
                    path: plan.path.clone(),
                    action: if plan.changed {
                        DiffAction::Update
                    } else {
                        DiffAction::Unchanged
                    },
                    target: "env-script".to_string(),
                    detail: plan.summary.first().cloned().unwrap_or_default(),
                });
                continue;
            }

            for p in &wanted {
                let detail = plan
                    .summary
                    .iter()
                    .find(|s| s.contains(&p.id))
                    .cloned()
                    .unwrap_or_else(|| "无变化".to_string());

                let action = if detail.starts_with("新增") {
                    DiffAction::Add
                } else if detail.starts_with("更新") {
                    DiffAction::Update
                } else {
                    DiffAction::Unchanged
                };

                out.push(DiffLine {
                    agent: "codex".to_string(),
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
