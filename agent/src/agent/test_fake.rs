use std::collections::VecDeque;
use std::error::Error;
use std::sync::{Arc, Mutex};

use llm::{ChatCompletionRequestMessage, ChatCompletionTools, Think, ThinkOutput};

/// 脚本化 fake LLM:按调用顺序依次吐出预设输出,并记录收到的消息
#[derive(Clone)]
pub struct FakeLLM {
    script: Arc<Mutex<VecDeque<ThinkOutput>>>,
    received: Arc<Mutex<Vec<Vec<ChatCompletionRequestMessage>>>>,
}

impl FakeLLM {
    pub fn new(script: Vec<ThinkOutput>) -> Self {
        Self {
            script: Arc::new(Mutex::new(script.into())),
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 已收到的请求消息(按调用顺序)
    pub fn received(&self) -> Vec<Vec<ChatCompletionRequestMessage>> {
        self.received.lock().unwrap().clone()
    }
}

impl Think for FakeLLM {
    fn think(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        _tools: &[ChatCompletionTools],
        _max_tokens: u32,
    ) -> impl Future<Output = Result<ThinkOutput, Box<dyn Error>>> + Send {
        async move {
            self.received.lock().unwrap().push(messages);
            let next = self.script.lock().unwrap().pop_front().unwrap();
            Ok(next)
        }
    }
}
