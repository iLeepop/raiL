use std::env;
use std::error::Error;

use crate::enums::Provider;
use crate::traits::Think;
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    traits::RequestOptionsBuilder,
    types::chat::{ChatCompletionRequestMessage, CreateChatCompletionRequestArgs, FinishReason},
};

#[derive(Debug, Clone)]
pub struct RaiLLMArgs {
    pub provider: Option<Provider>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_id: String,
}

impl RaiLLMArgs {
    pub fn with_provider(mut self, provider: Option<Provider>) -> Self {
        self.provider = provider;
        self
    }

    pub fn with_base_url(mut self, base_url: Option<String>) -> Self {
        self.base_url = base_url;
        self
    }

    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key;
        self
    }

    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    pub fn build(self) -> Result<RaiLLM, Box<dyn Error>> {
        RaiLLM::default().init(self.provider, self.base_url, self.api_key, self.model_id)
    }
}

impl Default for RaiLLMArgs {
    fn default() -> Self {
        return RaiLLMArgs {
            provider: None,
            base_url: None,
            api_key: None,
            model_id: String::new(),
        };
    }
}

#[derive(Debug, Clone)]
pub struct RaiLLM {
    pub provider: Provider,
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
}

impl RaiLLM {
    // 创建新的LLM
    pub fn new(
        provider: Provider,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        return RaiLLM {
            provider: provider,
            base_url: base_url.into(),
            api_key: api_key.into(),
            model_id: model_id.into(),
        };
    }

    // 初始化LLM, 提供自动检测适配能力
    // 适配vllm\Ollama
    // DeepSeek\Kimi
    pub fn init(
        mut self,
        provider: Option<Provider>,
        base_url: Option<impl Into<String>>,
        api_key: Option<impl Into<String>>,
        model_id: impl Into<String>,
    ) -> Result<Self, Box<dyn Error>> {
        // 先一次性转成 Option<String>，之后两处都可以借用或转移，不再互相冲突
        let api_key: Option<String> = api_key.map(Into::into);
        let base_url: Option<String> = base_url.map(Into::into);

        if let Some(p) = provider {
            self.provider = p;
        } else {
            self.provider = Self::auto_detect_provider(api_key.as_deref(), base_url.as_deref());
        }

        let (key, url) = Self::resolve_credentials(&self.provider, api_key, base_url);
        if let Some(key) = key {
            self.api_key = key;
        }
        if let Some(url) = url {
            self.base_url = url;
        }

        self.model_id = model_id.into();

        // 核心参数校验：显式参数、环境变量、默认值全部走完后仍缺失 → 初始化失败，
        // 避免带着空配置跑到请求阶段才报出难懂的构建错误
        let mut missing: Vec<&str> = Vec::new();
        if self.base_url.is_empty() {
            missing.push("base_url（显式参数或对应环境变量）");
        }
        if self.api_key.is_empty()
            && matches!(
                &self.provider,
                Provider::DEEPSEEK
                    | Provider::KIMI
                    | Provider::AUTO
                    | Provider::LOCAL
                    | Provider::VLLM
                    | Provider::OLLAMA
            )
        {
            missing.push("api_key（显式参数或对应环境变量）");
        }
        if self.model_id.is_empty() {
            missing.push("model_id");
        }
        if !missing.is_empty() {
            return Err(format!("RaiLLM::init 失败，缺少核心参数: {}", missing.join("、")).into());
        }

        Ok(self)
    }

    // 自动检测供应商
    // 优先级：显式传入的 BASE_URL > 环境变量
    fn auto_detect_provider(_api_key: Option<&str>, base_url: Option<&str>) -> Provider {
        // 通过 BASE_URL 判断
        if let Some(url) = base_url {
            let lowercase_url = url.to_lowercase();
            if lowercase_url.contains("deepseek.com") {
                return Provider::DEEPSEEK;
            } else if lowercase_url.contains("moonshot.cn") {
                return Provider::KIMI;
            } else if lowercase_url.contains("localhost") {
                if lowercase_url.contains(":8000") {
                    return Provider::VLLM;
                } else if lowercase_url.contains(":11434") {
                    return Provider::OLLAMA;
                }
                return Provider::LOCAL;
            }
        }

        // 环境变量兜底
        if env::var_os("DEEPSEEK_API_KEY").is_some() {
            return Provider::DEEPSEEK;
        }
        if env::var_os("KIMI_API_KEY").is_some() {
            return Provider::KIMI;
        }

        Provider::AUTO
    }

    // 推断参数配置：显式参数 > 供应商专属环境变量 > 内置默认值；AUTO/本地供应商回退通用 LLM_* 变量
    fn resolve_credentials(
        provider: &Provider,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> (Option<String>, Option<String>) {
        let (key_env, url_env, default_url) = match provider {
            Provider::DEEPSEEK => (
                "DEEPSEEK_API_KEY",
                "DEEPSEEK_BASE_URL",
                "https://api.deepseek.com",
            ),
            Provider::KIMI => (
                "KIMI_API_KEY",
                "KIMI_BASE_URL",
                "https://api.moonshot.cn/v1",
            ),
            _ => {
                let key = api_key.or_else(|| env::var("LLM_API_KEY").ok());
                let url = base_url.or_else(|| env::var("LLM_BASE_URL").ok());
                return (key, url);
            }
        };

        let key = api_key
            .or_else(|| env::var(key_env).ok())
            .unwrap_or_default();
        let url = base_url
            .or_else(|| env::var(url_env).ok())
            .unwrap_or_else(|| default_url.to_string());

        (Some(key), Some(url))
    }
}

impl RaiLLM {
    const TOKENS_BASE: u32 = 512;
    const TOKENS_BASE_UNIT: u32 = Self::TOKENS_BASE * 2;
    pub const MAX_TOKENS_SHORT: u32 = 2 * Self::TOKENS_BASE_UNIT;
    pub const MAX_TOKENS_NORMAL: u32 = 5 * Self::TOKENS_BASE_UNIT;
    pub const MAX_TOKENS_LONG: u32 = 8 * Self::TOKENS_BASE_UNIT;
    pub const MAX_TOKENS_REASONING: u32 = 12 * Self::TOKENS_BASE_UNIT;

    /// 按供应商返回合适的默认 max_tokens；think 传 0 时使用
    pub fn default_max_tokens(&self) -> u32 {
        match self.provider {
            Provider::DEEPSEEK | Provider::KIMI => Self::MAX_TOKENS_REASONING,
            _ => Self::MAX_TOKENS_NORMAL,
        }
    }
}

impl Default for RaiLLM {
    fn default() -> Self {
        return RaiLLM {
            provider: Provider::AUTO,
            base_url: String::new(),
            api_key: String::new(),
            model_id: String::new(),
        };
    }
}

impl Think for RaiLLM {
    // 实际调用模型能力
    async fn think(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        max_tokens: u32,
    ) -> Result<String, Box<dyn Error>> {
        // max_tokens 传 0 表示使用当前供应商的默认值
        let max_tokens = if max_tokens == 0 {
            self.default_max_tokens()
        } else {
            max_tokens
        };
        // 本地推理服务（vLLM/Ollama）可能不需要 key，空 key 不写入请求头
        let mut config = OpenAIConfig::default().with_api_base(self.base_url);
        if !self.api_key.is_empty() {
            config = config.with_api_key(self.api_key);
        }

        let client = Client::with_config(config);

        let request = CreateChatCompletionRequestArgs::default()
            .max_tokens(max_tokens)
            .model(self.model_id)
            .messages(messages)
            .build()?;

        let response = match client
            .chat()
            .query(&vec![("limit", 10)])?
            .create(request)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // async-openai 的 ApiError.code 是 Option<String>，而 vLLM 返回整数 code，
                // 反序列化失败导致 JSONDeserialize 掩盖真实错误信息——手动提取 message
                let msg = match &e {
                    OpenAIError::JSONDeserialize(_, raw) => {
                        let parsed = serde_json::from_str::<serde_json::Value>(raw).ok();
                        parsed
                            .as_ref()
                            .and_then(|v| v.pointer("/error/message"))
                            .and_then(|m| m.as_str())
                            .map(String::from)
                            .unwrap_or_else(|| e.to_string())
                    }
                    _ => e.to_string(),
                };
                return Err(msg.into());
            }
        };

        println!("\nResponse:\n");

        for choice in response.choices {
            println!(
                "{}: Role: {}  Content: {:?}  FinishReason: {:?}",
                choice.index, choice.message.role, choice.message.content, choice.finish_reason
            );

            // 推理模型（如 DeepSeek 推理类模型）会把 max_tokens 预算耗尽在思考上，
            // 导致最终内容为空且 finish_reason=length —— 直接报错，不静默返回空
            let content = choice.message.content.as_deref().unwrap_or_default();
            if content.is_empty() && choice.finish_reason == Some(FinishReason::Length) {
                return Err(format!(
                    "模型返回空内容（finish_reason=length）：max_tokens={} 被推理过程耗尽，请调大 max_tokens",
                    max_tokens
                )
                .into());
            }
            if !content.is_empty() {
                return Ok(content.to_string());
            }
        }

        return Err("模型未返回任何内容".into());
    }
}
