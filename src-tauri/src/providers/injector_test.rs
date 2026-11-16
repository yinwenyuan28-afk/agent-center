//! provider 注入器的回归测试。
//!
//! 这些测试只覆盖**纯逻辑**部分（关键字段判定、格式化、协议映射），
//! 不读写真实文件 —— 落盘行为在 `scripts/test-*.mjs` 里用临时副本验证。
//!
//! 为什么要有这批测试：本轮按 cc-switch 核对后，好几个地方从"看着对"
//! 改成了"真的对"，这些边界几乎都是靠对照源码才发现的。写成单测
//! 是为了下一次改注入器时不会悄悄退回去。

use crate::providers::model::ProviderKind;

// ---------------------------------------------------------------- Claude 关键字段

#[test]
fn claude_floor_env_covers_connection_keys() {
    use crate::providers::claude::is_owned_env_key;
    for key in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_CUSTOM_HEADERS",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
        "AWS_REGION",
        "AWS_BEARER_TOKEN_BEDROCK",
        "VERTEX_REGION_CLAUDE_4_5_SONNET",
        "CLOUD_ML_REGION",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_SUBAGENT_MODEL",
    ] {
        assert!(is_owned_env_key(key), "{key} 应该是关键字段");
    }
}

/// 功能开关不能按前缀误伤。
///
/// 这是 cc-switch 踩过的坑：`CLAUDE_CODE_USE_` 下面既有协议选择器，
/// 也有 `USE_POWERSHELL_TOOL` 这类纯功能开关。按前缀匹配会把用户
/// 的功能设置当连接配置删掉。
#[test]
fn claude_floor_env_leaves_feature_switches_alone() {
    use crate::providers::claude::is_owned_env_key;
    for key in [
        "CLAUDE_CODE_USE_POWERSHELL_TOOL",
        "CLAUDE_CODE_USE_NATIVE_FILE_SEARCH",
        "CLAUDE_CODE_SKIP_PROMPT_HISTORY",
        "API_TIMEOUT_MS",
        "DISABLE_TELEMETRY",
        "JAVA_HOME",
        "CLAUDE_CODE_DISABLE_ARTIFACT",
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
    ] {
        assert!(!is_owned_env_key(key), "{key} 必须留给用户");
    }
}

#[test]
fn claude_floor_top_excludes_user_sections() {
    use crate::providers::claude::owned_top_keys;
    let owned = owned_top_keys();
    for key in ["model", "apiKey", "apiKeyHelper", "modelOverrides"] {
        assert!(owned.contains(&key), "{key} 应该在顶层关键字段里");
    }
    for key in ["permissions", "hooks", "statusLine", "enabledPlugins", "env"] {
        assert!(!owned.contains(&key), "{key} 是用户键，不能碰");
    }
}

/// 供应商独有字段不能混进关键字段 —— 两者的清理时机不同。
#[test]
fn claude_exclusive_fields_are_not_floor_fields() {
    use crate::providers::claude::{exclusive_env_keys, is_owned_env_key};
    for key in exclusive_env_keys() {
        assert!(!is_owned_env_key(key), "{key} 不能同时算关键字段");
    }
}

// ---------------------------------------------------------------- 代理判定

/// Anthropic 协议永远直连，不看模型名。
///
/// 早期实现会把「模型 id 不是 claude- 开头」当成需要转换，结果
/// MiniMax 这类 Anthropic 兼容端点被错误地翻成 /chat/completions。
#[test]
fn anthropic_never_needs_bridge() {
    use crate::providers::claude_bridge::needs_bridge;
    assert!(!needs_bridge(
        ProviderKind::Anthropic,
        &["MiniMax-M3".to_string()]
    ));
    assert!(!needs_bridge(
        ProviderKind::Anthropic,
        &["deepseek/deepseek-v4.1-flash".to_string()]
    ));
    assert!(!needs_bridge(ProviderKind::Anthropic, &[]));
}

#[test]
fn other_protocols_always_need_bridge() {
    use crate::providers::claude_bridge::needs_bridge;
    for kind in [
        ProviderKind::OpenaiChat,
        ProviderKind::OpenaiResponses,
        ProviderKind::GoogleGenAi,
    ] {
        assert!(needs_bridge(kind, &["claude-opus-5".to_string()]));
        assert!(needs_bridge(kind, &[]));
    }
}

// ---------------------------------------------------------------- 协议词表

#[test]
fn hermes_api_mode_matches_cc_switch() {
    use crate::providers::hermes::api_mode_of;
    assert_eq!(api_mode_of(ProviderKind::Anthropic), "anthropic_messages");
    assert_eq!(
        api_mode_of(ProviderKind::OpenaiChat),
        "chat_completions"
    );
    assert_eq!(
        api_mode_of(ProviderKind::OpenaiResponses),
        "codex_responses"
    );
}

#[test]
fn kimi_type_matches_cc_switch() {
    use crate::providers::kimi::kimi_type_of;
    assert_eq!(kimi_type_of(ProviderKind::Anthropic), "anthropic");
    assert_eq!(kimi_type_of(ProviderKind::OpenaiChat), "openai");
    assert_eq!(kimi_type_of(ProviderKind::OpenaiResponses), "openai_responses");
    assert_eq!(kimi_type_of(ProviderKind::GoogleGenAi), "google-genai");
}

#[test]
fn grok_api_backend_only_two_values() {
    use crate::providers::grok::api_backend_of;
    assert_eq!(api_backend_of(ProviderKind::OpenaiResponses), "responses");
    assert_eq!(api_backend_of(ProviderKind::OpenaiChat), "chat_completions");
    assert_eq!(api_backend_of(ProviderKind::Anthropic), "chat_completions");
}

#[test]
fn minimax_api_values() {
    use crate::providers::minimax::api_of;
    assert_eq!(api_of(ProviderKind::Anthropic), "anthropic-messages");
    assert_eq!(api_of(ProviderKind::OpenaiChat), "openai-completions");
    assert_eq!(api_of(ProviderKind::OpenaiResponses), "openai-responses");
}

// ---------------------------------------------------------------- Gemini .env

#[test]
fn gemini_floor_env_scopes_google_prefix_only() {
    use crate::providers::gemini::is_owned_env_key;
    for key in [
        "GOOGLE_API_KEY",
        "GOOGLE_GEMINI_BASE_URL",
        "GOOGLE_GENAI_USE_VERTEXAI",
        "GOOGLE_CLOUD_PROJECT",
        "GEMINI_API_KEY",
        "GEMINI_MODEL",
        "GEMINI_CLI_CUSTOM_HEADERS",
    ] {
        assert!(is_owned_env_key(key), "{key} 应该是关键字段");
    }
    // CLI 自己的设置不能动
    for key in ["GEMINI_SANDBOX", "GEMINI_CLI_HOME", "GEMINI_SYSTEM_MD", "DEBUG"] {
        assert!(!is_owned_env_key(key), "{key} 必须留给用户");
    }
}

// ---------------------------------------------------------------- 输入模态收敛

/// Pi 的 `input` 只认 `text` / `image`，写了别的会让整份文件校验失败。
///
/// 这个坑是实际踩到的：母版里两条模型带 `video` / `pdf`，注入后 Pi 报
/// `Invalid models.json schema`，**整份自定义 provider 全部消失**，
/// 只剩 Pi 内置的那几家。用户看到的表象是「启用的不见了、关了的还在」。
#[test]
fn model_spec_input_within_drops_unsupported_modalities() {
    use crate::providers::model::ModelSpec;

    let mut spec = ModelSpec::new("m");
    spec.input_modalities = vec![
        "text".to_string(),
        "image".to_string(),
        "video".to_string(),
        "pdf".to_string(),
    ];

    assert_eq!(
        spec.input_within(&["text", "image"]),
        vec!["text".to_string(), "image".to_string()],
        "video / pdf 必须被剔除"
    );

    // 全部被过滤掉时返回空数组，调用方据此决定不写该字段
    let mut only_pdf = ModelSpec::new("m");
    only_pdf.input_modalities = vec!["pdf".to_string()];
    assert!(only_pdf.input_within(&["text", "image"]).is_empty());

    // 没填模态时按纯文本算，收敛后仍是 text
    let bare = ModelSpec::new("m");
    assert_eq!(bare.input_within(&["text", "image"]), vec!["text".to_string()]);
}

// ---------------------------------------------------------------- Codex 的模型与档位

/// `codex_model` 优先，没填才回退到 models 的第一个。
#[test]
fn codex_model_prefers_explicit_pick() {
    use crate::providers::model::{ModelSpec, Provider};

    let mut p = Provider {
        id: "p".into(),
        name: "p".into(),
        base_url: "https://example.com".into(),
        kind: ProviderKind::OpenaiResponses,
        agent_kinds: Default::default(),
        api_key: "k".repeat(32),
        models: vec![ModelSpec::new("first"), ModelSpec::new("second")],
        agents: vec!["codex".into()],
        claude_model_map: Default::default(),
        codex_requires_openai_auth: None,
        codex_model: None,
        note: String::new(),
    };

    // 没指定 → 第一个
    assert_eq!(p.codex_model_or_first(), Some("first"));

    // 指定了 → 用指定的
    p.codex_model = Some("second".into());
    assert_eq!(p.codex_model_or_first(), Some("second"));

    // 空白串当成没填，不许覆盖掉第一个
    p.codex_model = Some("   ".into());
    assert_eq!(p.codex_model_or_first(), Some("first"));

    // 一个模型都没有 → None，注入器据此跳过写 model 键
    p.models.clear();
    p.codex_model = None;
    assert_eq!(p.codex_model_or_first(), None);
}

/// Codex 只接受这七个档位。
///
/// 这些值会被原样放进请求体的 `reasoning.effort`，上游是封闭枚举。
/// 实测传母版里的 `enabled` 直接 400：
/// `Invalid option: expected one of "none"|"minimal"|...`。
/// 所以注入器必须只写白名单里的值，并清掉文件里残留的不合法值。
#[test]
fn codex_effort_whitelist_matches_upstream_enum() {
    use crate::providers::codex::is_codex_effort;

    for ok in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        assert!(is_codex_effort(ok), "{ok} 应该在白名单里");
    }
    // 各家自己的写法不在 Codex 词表里
    for bad in ["enabled", "off", "disabled", "true", ""] {
        assert!(!is_codex_effort(bad), "{bad} 不该被写进 config.toml");
    }
}
