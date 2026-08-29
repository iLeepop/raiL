use serde::{Deserialize, Serialize};

use std::error::Error;

use async_openai::types::chat::{ChatCompletionRequestMessage, ChatCompletionTools};

/// 模型发起的一次工具调用请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// 模型生成的 JSON 参数——注意模型不一定输出合法 JSON,调用方需校验
    pub arguments: String,
}

/// 一次 think 的完整输出:正文 + 工具调用
#[derive(Debug, Clone)]
pub struct ThinkOutput {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

pub trait Think {
    /// 反糖为 `-> impl Future` 以保留 dyn 兼容性(by-value `self` 仍要求具体类型,见 BaseAgent 泛型设计)
    fn think(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: &[ChatCompletionTools],
        max_tokens: u32,
    ) -> impl Future<Output = Result<ThinkOutput, Box<dyn Error>>> + Send;

    /// 流式版本:内容增量逐段回调 `on_delta`,流结束后返回聚合的 `ThinkOutput`
    /// (工具调用也在其中)。默认实现退化为一次性回调完整内容。
    fn think_stream<F>(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: &[ChatCompletionTools],
        max_tokens: u32,
        on_delta: F,
    ) -> impl Future<Output = Result<ThinkOutput, Box<dyn Error>>> + Send
    where
        F: FnMut(&str) + Send,
        Self: Sized + Send,
    {
        async move {
            let out = self.think(messages, tools, max_tokens).await?;
            let mut on_delta = on_delta;
            if let Some(content) = &out.content {
                on_delta(content);
            }
            Ok(out)
        }
    }
}
