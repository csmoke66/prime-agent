//! Compiled fallback catalog ported from the TS `packages/ai/src/models.generated.ts`.
//!
//! The full catalog is fetched at runtime (no-cold-start chain: validated disk cache ->
//! packaged bundled snapshot -> this compiled fallback). Hand-maintained; there is no generator.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::types::{
    CompatKind, JsNumber, MaxTokensField, Model, ModelCompat, ModelCost, ModelInput,
    ModelThinkingLevel, OpenAiCompletionsCompat, ThinkingFormat,
};

type ProviderCatalog = HashMap<String, HashMap<String, Model>>;

/// The compat fields the fallback table carries: the
/// [`OpenAiCompletionsCompat`] subset the TS fallback file uses. Absent
/// fields stay `None` on the wire.
struct RawCompat {
    supports_store: Option<bool>,
    supports_developer_role: Option<bool>,
    supports_reasoning_effort: Option<bool>,
    max_tokens_field: Option<MaxTokensField>,
    requires_reasoning_content_on_assistant_messages: Option<bool>,
    thinking_format: Option<ThinkingFormat>,
    zai_tool_stream: Option<bool>,
    send_session_affinity_headers: Option<bool>,
    supports_strict_mode: Option<bool>,
}

/// One entry of the compiled fallback table, in the TS `MODELS` layout.
struct RawModel {
    provider: &'static str,
    id: &'static str,
    name: &'static str,
    api: &'static str,
    base_url: &'static str,
    reasoning: bool,
    /// The `thinkingLevelMap` pairs, in the TS level order; `None` marks a
    /// level as unsupported, an absent level is simply not listed.
    thinking_levels: &'static [(ModelThinkingLevel, Option<&'static str>)],
    input: &'static [ModelInput],
    /// (`input`, `output`, `cacheRead`, `cacheWrite`) in $/million tokens.
    cost: (f64, f64, f64, f64),
    context_window: u64,
    max_tokens: u64,
    featured: Option<bool>,
    headers: &'static [(&'static str, &'static str)],
    compat: Option<RawCompat>,
}

static RAW_MODELS: &[RawModel] = &[
    RawModel {
        provider: "amazon-bedrock",
        id: "amazon.nova-2-lite-v1:0",
        name: "Nova 2 Lite",
        api: "bedrock-converse-stream",
        base_url: "https://bedrock-runtime.us-east-1.amazonaws.com",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.33, 2.75, 0.0, 0.0),
        context_window: 128_000,
        max_tokens: 4_096,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "amazon-bedrock",
        id: "au.anthropic.claude-haiku-4-5-20251001-v1:0",
        name: "Claude Haiku 4.5 (AU)",
        api: "bedrock-converse-stream",
        base_url: "https://bedrock-runtime.ap-southeast-2.amazonaws.com",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.0, 5.0, 0.1, 1.25),
        context_window: 200_000,
        max_tokens: 64_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "amazon-bedrock",
        id: "eu.anthropic.claude-fable-5",
        name: "Claude Fable 5 (EU)",
        api: "bedrock-converse-stream",
        base_url: "https://bedrock-runtime.eu-central-1.amazonaws.com",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (11.0, 55.0, 1.1, 13.75),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "amazon-bedrock",
        id: "jp.anthropic.claude-haiku-4-5-20251001-v1:0",
        name: "Claude Haiku 4.5 (JP)",
        api: "bedrock-converse-stream",
        base_url: "https://bedrock-runtime.ap-northeast-1.amazonaws.com",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.0, 5.0, 0.1, 1.25),
        context_window: 200_000,
        max_tokens: 64_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "anthropic",
        id: "claude-fable-5",
        name: "Claude Fable 5",
        api: "anthropic-messages",
        base_url: "https://api.anthropic.com",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (10.0, 50.0, 1.0, 12.5),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "azure-openai-responses",
        id: "gpt-4",
        name: "GPT-4",
        api: "azure-openai-responses",
        base_url: "",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (30.0, 60.0, 0.0, 0.0),
        context_window: 8_192,
        max_tokens: 8_192,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "cerebras",
        id: "gemma-4-31b",
        name: "Gemma 4 31B IT",
        api: "openai-completions",
        base_url: "https://api.cerebras.ai/v1",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.99, 1.49, 0.0, 0.0),
        context_window: 131_072,
        max_tokens: 40_960,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "cloudflare-ai-gateway",
        id: "claude-fable-5",
        name: "Claude Fable 5",
        api: "anthropic-messages",
        base_url: "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/anthropic",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (10.0, 50.0, 1.0, 12.5),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "cloudflare-ai-gateway",
        id: "gpt-4.1",
        name: "GPT-4.1",
        api: "openai-responses",
        base_url: "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/openai",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (2.0, 8.0, 0.5, 0.0),
        context_window: 1_047_576,
        max_tokens: 32_768,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "cloudflare-workers-ai",
        id: "@cf/deepseek-ai/deepseek-v4-flash-0731",
        name: "DeepSeek V4 Flash 0731",
        api: "openai-completions",
        base_url: "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Low, None),
                (crate::types::ModelThinkingLevel::Medium, None),
                (crate::types::ModelThinkingLevel::High, Some("high")),
                (crate::types::ModelThinkingLevel::Xhigh, Some("max")),
                (crate::types::ModelThinkingLevel::Max, None),
        ],
        input: &[crate::types::ModelInput::Text],
        cost: (0.44, 1.32, 0.014, 0.0),
        context_window: 1_310_720,
        max_tokens: 1_048_576,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: None,
            supports_developer_role: None,
            supports_reasoning_effort: None,
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: Some(true),
            thinking_format: Some(crate::types::ThinkingFormat::Deepseek),
            zai_tool_stream: None,
            send_session_affinity_headers: Some(true),
            supports_strict_mode: None,
        }),
    },
    RawModel {
        provider: "deepseek",
        id: "deepseek-v4-flash",
        name: "DeepSeek V4 Flash",
        api: "openai-completions",
        base_url: "https://api.deepseek.com",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Low, None),
                (crate::types::ModelThinkingLevel::Medium, None),
                (crate::types::ModelThinkingLevel::High, Some("high")),
                (crate::types::ModelThinkingLevel::Xhigh, Some("max")),
                (crate::types::ModelThinkingLevel::Max, None),
        ],
        input: &[crate::types::ModelInput::Text],
        cost: (0.14, 0.28, 0.002_8, 0.0),
        context_window: 1_000_000,
        max_tokens: 384_000,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: None,
            supports_developer_role: None,
            supports_reasoning_effort: None,
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: Some(true),
            thinking_format: Some(crate::types::ThinkingFormat::Deepseek),
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: None,
        }),
    },
    RawModel {
        provider: "fireworks",
        id: "accounts/fireworks/models/deepseek-v4-flash-0731",
        name: "DeepSeek V4 Flash 0731",
        api: "anthropic-messages",
        base_url: "https://api.fireworks.ai/inference",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.22, 0.66, 0.007, 0.0),
        context_window: 1_000_000,
        max_tokens: 384_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "github-copilot",
        id: "claude-fable-5",
        name: "Claude Fable 5",
        api: "anthropic-messages",
        base_url: "https://api.individual.githubcopilot.com",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (10.0, 50.0, 1.0, 12.5),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[
                ("User-Agent", "GitHubCopilotChat/0.48.1"),
                ("Editor-Version", "vscode/1.136.1"),
                ("Editor-Plugin-Version", "copilot-chat/0.48.1"),
                ("Copilot-Integration-Id", "vscode-chat"),
        ],
        compat: None,
    },
    RawModel {
        provider: "github-copilot",
        id: "gemini-3.5-flash",
        name: "Gemini 3.5 Flash",
        api: "openai-completions",
        base_url: "https://api.individual.githubcopilot.com",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.5, 9.0, 0.15, 0.0),
        context_window: 200_000,
        max_tokens: 64_000,
        featured: None,
        headers: &[
                ("User-Agent", "GitHubCopilotChat/0.48.1"),
                ("Editor-Version", "vscode/1.136.1"),
                ("Editor-Plugin-Version", "copilot-chat/0.48.1"),
                ("Copilot-Integration-Id", "vscode-chat"),
        ],
compat: Some(RawCompat {
            supports_store: Some(false),
            supports_developer_role: Some(false),
            supports_reasoning_effort: Some(false),
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: None,
            thinking_format: None,
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: None,
        }),
    },
    RawModel {
        provider: "github-copilot",
        id: "gpt-5-mini",
        name: "GPT-5 Mini",
        api: "openai-responses",
        base_url: "https://api.individual.githubcopilot.com",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.25, 2.0, 0.025, 0.0),
        context_window: 264_000,
        max_tokens: 64_000,
        featured: None,
        headers: &[
                ("User-Agent", "GitHubCopilotChat/0.48.1"),
                ("Editor-Version", "vscode/1.136.1"),
                ("Editor-Plugin-Version", "copilot-chat/0.48.1"),
                ("Copilot-Integration-Id", "vscode-chat"),
        ],
        compat: None,
    },
    RawModel {
        provider: "google",
        id: "gemini-2.5-flash",
        name: "Gemini 2.5 Flash",
        api: "google-generative-ai",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.3, 2.5, 0.03, 0.0),
        context_window: 1_048_576,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "google-vertex",
        id: "gemini-1.5-flash",
        name: "Gemini 1.5 Flash (Vertex)",
        api: "google-vertex",
        base_url: "https://{location}-aiplatform.googleapis.com",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.075, 0.3, 0.018_75, 0.0),
        context_window: 1_000_000,
        max_tokens: 8_192,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "groq",
        id: "llama-3.1-8b-instant",
        name: "Llama 3.1 8B",
        api: "openai-completions",
        base_url: "https://api.groq.com/openai/v1",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.05, 0.08, 0.0, 0.0),
        context_window: 131_072,
        max_tokens: 131_072,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "huggingface",
        id: "deepseek-ai/DeepSeek-R1",
        name: "DeepSeek-R1",
        api: "openai-completions",
        base_url: "https://router.huggingface.co/v1",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.7, 2.5, 0.0, 0.0),
        context_window: 64_000,
        max_tokens: 32_768,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: None,
            supports_developer_role: Some(false),
            supports_reasoning_effort: None,
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: None,
            thinking_format: None,
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: None,
        }),
    },
    RawModel {
        provider: "kimi-coding",
        id: "k3",
        name: "Kimi K3",
        api: "anthropic-messages",
        base_url: "https://api.kimi.com/coding",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Low, None),
                (crate::types::ModelThinkingLevel::Medium, None),
                (crate::types::ModelThinkingLevel::High, None),
                (crate::types::ModelThinkingLevel::Xhigh, None),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.0, 0.0, 0.0, 0.0),
        context_window: 1_048_576,
        max_tokens: 131_072,
        featured: None,
        headers: &[
                ("User-Agent", "KimiCLI/1.5"),
        ],
        compat: None,
    },
    RawModel {
        provider: "minimax",
        id: "MiniMax-M2.7",
        name: "MiniMax-M2.7",
        api: "anthropic-messages",
        base_url: "https://api.minimax.io/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.3, 1.2, 0.06, 0.375),
        context_window: 204_800,
        max_tokens: 131_072,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "minimax-cn",
        id: "MiniMax-M2.7",
        name: "MiniMax-M2.7",
        api: "anthropic-messages",
        base_url: "https://api.minimaxi.com/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.3, 1.2, 0.06, 0.375),
        context_window: 204_800,
        max_tokens: 131_072,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "mistral",
        id: "codestral-latest",
        name: "Codestral (latest)",
        api: "mistral-conversations",
        base_url: "https://api.mistral.ai",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.3, 0.9, 0.0, 0.0),
        context_window: 256_000,
        max_tokens: 4_096,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "moonshotai",
        id: "kimi-k2-0711-preview",
        name: "Kimi K2 0711",
        api: "openai-completions",
        base_url: "https://api.moonshot.ai/v1",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.6, 2.5, 0.15, 0.0),
        context_window: 131_072,
        max_tokens: 16_384,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: Some(false),
            supports_developer_role: Some(false),
            supports_reasoning_effort: Some(false),
            max_tokens_field: Some(crate::types::MaxTokensField::MaxTokens),
            requires_reasoning_content_on_assistant_messages: None,
            thinking_format: None,
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: Some(false),
        }),
    },
    RawModel {
        provider: "moonshotai-cn",
        id: "kimi-k2-0711-preview",
        name: "Kimi K2 0711",
        api: "openai-completions",
        base_url: "https://api.moonshot.cn/v1",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.6, 2.5, 0.15, 0.0),
        context_window: 131_072,
        max_tokens: 16_384,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: Some(false),
            supports_developer_role: Some(false),
            supports_reasoning_effort: Some(false),
            max_tokens_field: Some(crate::types::MaxTokensField::MaxTokens),
            requires_reasoning_content_on_assistant_messages: None,
            thinking_format: None,
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: Some(false),
        }),
    },
    RawModel {
        provider: "openai",
        id: "gpt-4",
        name: "GPT-4",
        api: "openai-responses",
        base_url: "https://api.openai.com/v1",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (30.0, 60.0, 0.0, 0.0),
        context_window: 8_192,
        max_tokens: 8_192,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "openai",
        id: "gpt-6.1-terra",
        name: "GPT-6.1 Terra",
        api: "openai-responses",
        base_url: "https://api.openai.com/v1",
        reasoning: true,
        thinking_levels: &[
            (ModelThinkingLevel::Off, None),
            (ModelThinkingLevel::Minimal, None),
            (ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[ModelInput::Text, ModelInput::Image],
        cost: (1.0, 5.0, 0.05, 1.25),
        context_window: 272_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "openai",
        id: "gpt-6.2-astra",
        name: "GPT-6.2 Astra",
        api: "openai-responses",
        base_url: "https://api.openai.com/v1",
        reasoning: true,
        thinking_levels: &[
            (ModelThinkingLevel::Off, None),
            (ModelThinkingLevel::Minimal, None),
            (ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[ModelInput::Text, ModelInput::Image],
        cost: (8.0, 40.0, 0.4, 10.0),
        context_window: 272_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "openai",
        id: "gpt-6.2-sol",
        name: "GPT-6.2 Sol",
        api: "openai-responses",
        base_url: "https://api.openai.com/v1",
        reasoning: true,
        thinking_levels: &[
            (ModelThinkingLevel::Off, None),
            (ModelThinkingLevel::Minimal, None),
            (ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[ModelInput::Text, ModelInput::Image],
        cost: (2.0, 10.0, 0.1, 2.5),
        context_window: 272_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "openai-codex",
        id: "gpt-5.1",
        name: "GPT-5.1",
        api: "openai-codex-responses",
        base_url: "https://chatgpt.com/backend-api",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.25, 10.0, 0.125, 0.0),
        context_window: 272_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode",
        id: "big-pickle",
        name: "Big Pickle",
        api: "openai-completions",
        base_url: "https://opencode.ai/zen/v1",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.0, 0.0, 0.0, 0.0),
        context_window: 200_000,
        max_tokens: 32_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode",
        id: "claude-fable-5",
        name: "Claude Fable 5",
        api: "anthropic-messages",
        base_url: "https://opencode.ai/zen",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (10.0, 50.0, 1.0, 12.5),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode",
        id: "gemini-3-flash",
        name: "Gemini 3 Flash",
        api: "google-generative-ai",
        base_url: "https://opencode.ai/zen/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.5, 3.0, 0.05, 0.0),
        context_window: 1_048_576,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode",
        id: "gpt-5",
        name: "GPT-5",
        api: "openai-responses",
        base_url: "https://opencode.ai/zen/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.07, 8.5, 0.107, 0.0),
        context_window: 400_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode-go",
        id: "deepseek-v4-flash",
        name: "DeepSeek V4 Flash",
        api: "openai-completions",
        base_url: "https://opencode.ai/zen/go/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Low, None),
                (crate::types::ModelThinkingLevel::Medium, None),
                (crate::types::ModelThinkingLevel::High, Some("high")),
                (crate::types::ModelThinkingLevel::Xhigh, Some("max")),
                (crate::types::ModelThinkingLevel::Max, None),
        ],
        input: &[crate::types::ModelInput::Text],
        cost: (0.22, 0.66, 0.007, 0.0),
        context_window: 1_000_000,
        max_tokens: 384_000,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: None,
            supports_developer_role: None,
            supports_reasoning_effort: None,
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: Some(true),
            thinking_format: Some(crate::types::ThinkingFormat::Deepseek),
            zai_tool_stream: None,
            send_session_affinity_headers: None,
            supports_strict_mode: None,
        }),
    },
    RawModel {
        provider: "opencode-go",
        id: "gpt-5.6-luna",
        name: "GPT-5.6 Luna",
        api: "openai-responses",
        base_url: "https://opencode.ai/zen/go/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.2, 1.2, 0.02, 0.25),
        context_window: 1_050_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "opencode-go",
        id: "minimax-m3",
        name: "MiniMax-M3",
        api: "anthropic-messages",
        base_url: "https://opencode.ai/zen/go",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (0.3, 1.2, 0.06, 0.0),
        context_window: 1_000_000,
        max_tokens: 131_072,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "openrouter",
        id: "~anthropic/claude-fable-latest",
        name: "Anthropic: Claude Fable Latest",
        api: "openai-completions",
        base_url: "https://openrouter.ai/api/v1",
        reasoning: true,
        thinking_levels: &[
                (crate::types::ModelThinkingLevel::Off, None),
                (crate::types::ModelThinkingLevel::Minimal, None),
                (crate::types::ModelThinkingLevel::Low, Some("low")),
                (crate::types::ModelThinkingLevel::Medium, Some("medium")),
                (crate::types::ModelThinkingLevel::High, Some("high")),
                (crate::types::ModelThinkingLevel::Xhigh, Some("xhigh")),
                (crate::types::ModelThinkingLevel::Max, Some("max")),
        ],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (10.0, 50.0, 0.25, 12.5),
        context_window: 1_000_000,
        max_tokens: 128_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "vercel-ai-gateway",
        id: "alibaba/qwen-3-14b",
        name: "Qwen3-14B",
        api: "anthropic-messages",
        base_url: "https://ai-gateway.vercel.sh",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.12, 0.24, 0.0, 0.0),
        context_window: 40_960,
        max_tokens: 16_384,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "xai",
        id: "grok-4.20-0309-non-reasoning",
        name: "Grok 4.20 (Non-Reasoning)",
        api: "openai-completions",
        base_url: "https://api.x.ai/v1",
        reasoning: false,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text, crate::types::ModelInput::Image],
        cost: (1.25, 2.5, 0.2, 0.0),
        context_window: 1_000_000,
        max_tokens: 30_000,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "xiaomi",
        id: "mimo-v2-flash",
        name: "MiMo-V2-Flash",
        api: "anthropic-messages",
        base_url: "https://api.xiaomimimo.com/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.14, 0.28, 0.002_8, 0.0),
        context_window: 262_144,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "xiaomi-token-plan-ams",
        id: "mimo-v2-flash",
        name: "MiMo-V2-Flash",
        api: "anthropic-messages",
        base_url: "https://token-plan-ams.xiaomimimo.com/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.14, 0.28, 0.002_8, 0.0),
        context_window: 262_144,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "xiaomi-token-plan-cn",
        id: "mimo-v2-flash",
        name: "MiMo-V2-Flash",
        api: "anthropic-messages",
        base_url: "https://token-plan-cn.xiaomimimo.com/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.14, 0.28, 0.002_8, 0.0),
        context_window: 262_144,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "xiaomi-token-plan-sgp",
        id: "mimo-v2-flash",
        name: "MiMo-V2-Flash",
        api: "anthropic-messages",
        base_url: "https://token-plan-sgp.xiaomimimo.com/anthropic",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.14, 0.28, 0.002_8, 0.0),
        context_window: 262_144,
        max_tokens: 65_536,
        featured: None,
        headers: &[],
        compat: None,
    },
    RawModel {
        provider: "zai",
        id: "glm-4.7",
        name: "GLM-4.7",
        api: "openai-completions",
        base_url: "https://api.z.ai/api/coding/paas/v4",
        reasoning: true,
        thinking_levels: &[],
        input: &[crate::types::ModelInput::Text],
        cost: (0.0, 0.0, 0.0, 0.0),
        context_window: 204_800,
        max_tokens: 131_072,
        featured: None,
        headers: &[],
compat: Some(RawCompat {
            supports_store: None,
            supports_developer_role: Some(false),
            supports_reasoning_effort: None,
            max_tokens_field: None,
            requires_reasoning_content_on_assistant_messages: None,
            thinking_format: Some(crate::types::ThinkingFormat::Zai),
            zai_tool_stream: Some(true),
            send_session_affinity_headers: None,
            supports_strict_mode: None,
        }),
    },];

/// Expand a raw compat into the wire compat object (the fallback
/// table only uses the OpenAI-completions shape).
fn compat_from_raw(raw: &RawCompat) -> ModelCompat {
    ModelCompat::from_kind(CompatKind::OpenAiCompletions(Box::new(
        OpenAiCompletionsCompat {
            supports_store: raw.supports_store,
            supports_developer_role: raw.supports_developer_role,
            supports_reasoning_effort: raw.supports_reasoning_effort,
            max_tokens_field: raw.max_tokens_field,
            requires_reasoning_content_on_assistant_messages: raw
                .requires_reasoning_content_on_assistant_messages,
            thinking_format: raw.thinking_format,
            zai_tool_stream: raw.zai_tool_stream,
            send_session_affinity_headers: raw.send_session_affinity_headers,
            supports_strict_mode: raw.supports_strict_mode,
            ..OpenAiCompletionsCompat::default()
        },
    )))
}

/// Expand one raw table entry into a full [`Model`].
fn model_from_raw(raw: &RawModel) -> Model {
    let thinking_level_map = (!raw.thinking_levels.is_empty()).then(|| {
        raw.thinking_levels
            .iter()
            .copied()
            .map(|(level, value)| (level, value.map(String::from)))
            .collect()
    });
    let headers = (!raw.headers.is_empty()).then(|| {
        raw.headers
            .iter()
            .copied()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    });
    Model {
        id: raw.id.to_string(),
        name: raw.name.to_string(),
        api: raw.api.to_string(),
        provider: raw.provider.to_string(),
        base_url: raw.base_url.to_string(),
        reasoning: raw.reasoning,
        thinking_level_map,
        input: raw.input.to_vec(),
        cost: ModelCost {
            input: JsNumber::from(raw.cost.0),
            output: JsNumber::from(raw.cost.1),
            cache_read: JsNumber::from(raw.cost.2),
            cache_write: JsNumber::from(raw.cost.3),
        },
        context_window: raw.context_window,
        max_tokens: raw.max_tokens,
        max_tokens_explicit: false,
        featured: raw.featured,
        headers,
        compat: raw.compat.as_ref().map(compat_from_raw),
    }
}

fn catalog() -> &'static ProviderCatalog {
    static CATALOG: LazyLock<ProviderCatalog> = LazyLock::new(|| {
        let mut catalog: ProviderCatalog = HashMap::with_capacity(RAW_MODELS.len());
        for raw in RAW_MODELS {
            let model = model_from_raw(raw);
            catalog
                .entry(model.provider.clone())
                .or_default()
                .insert(model.id.clone(), model);
        }
        catalog
    });
    &CATALOG
}

/// Look up a model by provider and model id (TS `getModel`).
#[must_use]
pub fn get_model(provider: &str, model_id: &str) -> Option<&'static Model> {
    catalog().get(provider)?.get(model_id)
}

/// All providers in the catalog (TS `getProviders`).
pub fn get_providers() -> Vec<&'static str> {
    let mut providers: Vec<&'static str> = catalog().keys().map(String::as_str).collect();
    providers.sort_unstable();
    providers
}

/// All models for one provider, in catalog order (TS `getModels`).
#[must_use]
pub fn get_models(provider: &str) -> Vec<&'static Model> {
    let Some(models) = catalog().get(provider) else {
        return Vec::new();
    };
    // The table is provider/id-sorted, but keep the explicit id sort so the
    // ordering never depends on the HashMap iteration order.
    let mut models: Vec<&'static Model> = models.values().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads_at_the_ts_fallback_scale() {
        let providers = get_providers();
        assert_eq!(providers.len(), 31, "the compiled fallback provider list");
        let total: usize = providers
            .iter()
            .map(|provider| get_models(provider).len())
            .sum();
        assert_eq!(total, 45, "the compiled fallback model list");
        // The compiled transport table: one-plus template per tuple.
        // The models are collected first: the tuples borrow from them, so
        // the values must outlive the loop.
        let models: Vec<Model> = RAW_MODELS.iter().map(model_from_raw).collect();
        let mut tuples: Vec<(&str, &str, &str)> = Vec::new();
        for model in &models {
            let tuple = (
                model.provider.as_str(),
                model.api.as_str(),
                model.base_url.as_str(),
            );
            if !tuples.contains(&tuple) {
                tuples.push(tuple);
            }
        }
        assert_eq!(tuples.len(), 42, "the public transport tuples");
    }

    #[test]
    fn looks_up_known_models() {
        let model = get_model("anthropic", "claude-fable-5")
            .or_else(|| get_model("amazon-bedrock", "eu.anthropic.claude-fable-5"))
            .expect("fable template present");
        assert!(model.reasoning);
        assert!(model.context_window > 0);
        assert!(model.cost.input.as_f64() >= 0.0);
    }

    #[test]
    fn unknown_lookups_are_none() {
        assert!(get_model("nope", "nope").is_none());
        assert!(get_models("nope").is_empty());
    }
}
