//! 库入口。CLI 和 GUI 共用这套模块。

pub mod agents;
pub mod cli;
pub mod cli_run;
pub mod commands;
pub mod config;
pub mod console;
pub mod credentials;
pub mod doc;
pub mod git;
pub mod link;
pub mod mcp;
pub mod proc;
pub mod providers;
pub mod skills;
pub mod skillscli;
pub mod types;
pub mod usage;

/// 启动图形界面。
///
/// 只有在没有子命令时才走到这里，由 `main.rs` 负责调用。
pub fn run_gui() {
    // 这个 exe 是 windows 子系统，双击启动时系统不分配控制台，
    // 所以没有黑窗口。CLI 分支在 main.rs 里单独把父控制台接回来。
    // 窗口活着，Claude 才能把 OpenAI 协议的模型翻成 Messages。
    providers::claude_bridge::ensure_started();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_mappings,
            commands::sync_mappings,
            commands::repair_mapping,
            commands::unmap,
            commands::remap,
            commands::read_doc,
            commands::save_doc,
            commands::create_doc,
            commands::list_agent_docs,
            commands::read_agent_doc,
            commands::save_agent_doc,
            commands::compose_agent_docs,
            commands::list_skills,
            commands::set_skill_tags,
            commands::scan_orphans,
            commands::collect_skills,
            commands::apply_orphan_action,
            commands::list_skill_mappings,
            commands::sync_skill_mappings,
            commands::remove_skill_mapping,
            commands::list_managed,
            commands::check_updates,
            commands::update_skills,
            commands::git_status,
            commands::git_commit,
            commands::git_push,
            commands::inspect_credentials,
            commands::list_providers,
            commands::save_provider,
            commands::delete_provider,
            commands::diff_providers,
            commands::sync_providers,
            commands::provider_matrix,
            commands::set_provider_row,
            commands::set_provider_assignment,
            commands::backfill_models,
            commands::read_providers_toml,
            commands::write_providers_toml,
            commands::claude_onboarding_status,
            commands::set_claude_onboarding_skip,
            commands::set_codex_requires_openai_auth,
            commands::set_codex_model,
            commands::set_codex_connection,
            commands::codex_connection_status,
            commands::set_codex_connection_direct,
            commands::set_codex_requires_openai_auth_direct,
            commands::cursor_byok_status,
            commands::cursor_byok_plan,
            commands::cursor_byok_apply,
            commands::cursor_byok_remove_models,
            commands::cursor_byok_reset,
            commands::list_mcp_servers,
            commands::save_mcp_server,
            commands::delete_mcp_server,
            commands::diff_mcp,
            commands::sync_mcp,
            commands::read_mcp_toml,
            commands::write_mcp_toml,
            commands::mcp_matrix,
            commands::set_mcp_assignment,
            commands::set_mcp_row,
            commands::import_mcp_from_agents,
            commands::usage_report,
            commands::set_usage_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("启动 agentcenter 窗口失败");
}
