//! 库入口。CLI 和 GUI 共用这套模块。

pub mod agents;
pub mod cli;
pub mod cli_run;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod doc;
pub mod git;
pub mod link;
pub mod mcp;
pub mod providers;
pub mod skills;
pub mod skillscli;
pub mod types;

/// 启动图形界面。
///
/// 只有在没有子命令时才走到这里，由 `main.rs` 负责调用。
pub fn run_gui() {
    // 窗口活着，Claude 才能把 OpenAI 协议的模型翻成 Messages。
    providers::claude_bridge::ensure_started();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_mappings,
            commands::sync_mappings,
            commands::repair_mapping,
            commands::read_doc,
            commands::save_doc,
            commands::create_doc,
            commands::list_skills,
            commands::set_skill_tags,
            commands::scan_orphans,
            commands::collect_skills,
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
        ])
        .run(tauri::generate_context!())
        .expect("启动 agentcenter 窗口失败");
}
