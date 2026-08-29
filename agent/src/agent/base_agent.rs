use std::error::Error;

use llm::{Message, RaiLLM, RaiLLMArgs, Role, Think};

use crate::{
    config::Config,
    core::{Agent, ToolRegister},
};

use super::runtime;

pub struct BaseAgent<L = RaiLLM> {
    pub name: String,
    pub llm: L,
    pub system_prompt: String,
    pub config: Config,
    pub history: Vec<Message>,
    pub tools: ToolRegister,
}

impl BaseAgent<RaiLLM> {
    pub fn new(
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        config: Config,
        tools: ToolRegister,
    ) -> Self {
        let c = config.clone();

        let rllm = RaiLLMArgs::default()
            .with_provider(c.default_provider)
            .with_model_id(c.default_model.unwrap())
            .with_api_key(c.default_api_key)
            .with_base_url(c.default_base_url)
            .build()
            .expect("RaiLLM 初始化失败");

        Self::from_parts(name, system_prompt, config, rllm, tools)
    }
}

impl<L: Think> BaseAgent<L> {
    /// 依赖注入构造:测试时可用实现了 Think 的 fake 替换真实 LLM
    pub fn from_parts(
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        config: Config,
        llm: L,
        tools: ToolRegister,
    ) -> Self {
        BaseAgent {
            name: name.into(),
            llm,
            system_prompt: system_prompt.into(),
            config,
            history: Vec::new(),
            tools,
        }
    }
}

impl<L: Think + Clone + Send> Agent for BaseAgent<L> {
    fn run<'a>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a {
        // 非流式 = 流式 + 丢弃增量
        self.run_stream(message, |_| {})
    }

    fn run_stream<'a, F>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
        on_delta: F,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a
    where
        F: FnMut(&str) + Send + 'a,
    {
        async move {
            let mut on_delta = on_delta;
            let message = message.into();

            let mut messages = runtime::chat_history(
                self.system_prompt.clone(),
                self.history.clone(),
                message.clone(),
            );
            // 记录用户消息,使 agent 跨轮复用时可保留完整会话
            self.history.push(Message::new(Role::User, message));

            let tools_schemas = self.tools.schemas();
            let max_tokens = self.config.max_token.unwrap_or(RaiLLM::MAX_TOKENS_NORMAL);

            for _ in 0..self.config.max_tool_rounds {
                let chat = runtime::to_chat(&messages)?;
                let out = self
                    .llm
                    .clone()
                    .think_stream(chat, &tools_schemas, max_tokens, &mut on_delta)
                    .await?;

                // 模型没有请求工具:本轮结束
                if out.tool_calls.is_empty() {
                    let reply = out.content.unwrap_or_default();
                    self.history
                        .push(Message::new(Role::Assistant, reply.clone()));
                    return Ok(reply);
                }

                // 执行工具并把结果回填,进入下一轮
                runtime::execute_tool_calls(&mut messages, out, &self.tools).await;
            }

            Err(format!(
                "tool loop exceeded max rounds ({})",
                self.config.max_tool_rounds
            )
            .into())
        }
    }

    fn add_message(&mut self, message: Message) {
        self.history.push(message);
    }

    /// 只保留最近 `keep` 条消息(会话裁剪)
    fn truncate(&mut self, keep: usize) {
        if self.history.len() > keep {
            self.history.drain(..self.history.len() - keep);
        }
    }

    fn clear_message(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use llm::{ChatCompletionRequestMessage, ThinkOutput, ToolCall};

    use super::*;
    use crate::agent::test_fake::FakeLLM;
    use crate::core::ToolParameters;

    #[tokio::test]
    async fn run_executes_tool_and_feeds_result_back() {
        let mut reg = ToolRegister::new();
        reg.register(
            "echo",
            "原样返回文本",
            ToolParameters::new(serde_json::json!({ "type": "object" })),
            |args: serde_json::Value| async move { Ok(args) },
        );

        let fake = FakeLLM::new(vec![
            ThinkOutput {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "echo".into(),
                    arguments: r#"{"hello":"world"}"#.into(),
                }],
            },
            ThinkOutput {
                content: Some("done".into()),
                tool_calls: vec![],
            },
        ]);

        let mut agent = BaseAgent::from_parts("t", "sys", Config::default(), fake.clone(), reg);
        let result = agent.run("hi").await.unwrap();
        assert_eq!(result, "done");

        // 第二轮请求里必须包含 tool 结果消息,且 tool_call_id 与请求一致
        let received = fake.received();
        assert_eq!(received.len(), 2);
        let second = &received[1];
        let tool_msg = second.iter().find_map(|m| match m {
            ChatCompletionRequestMessage::Tool(tm) => Some(tm),
            _ => None,
        });
        assert!(tool_msg.is_some(), "第二轮请求缺少 Role::Tool 消息");
        let tm = tool_msg.unwrap();
        assert_eq!(tm.tool_call_id, "call_1");
        match &tm.content {
            llm::ChatCompletionRequestToolMessageContent::Text(text) => {
                assert!(text.contains("hello"));
            }
            llm::ChatCompletionRequestToolMessageContent::Array(_) => {
                panic!("tool 结果消息应为文本内容");
            }
        }
    }

    #[tokio::test]
    async fn run_stream_emits_deltas() {
        // FakeLLM 的 think_stream 走默认实现:一次性回调全文
        let fake = FakeLLM::new(vec![ThinkOutput {
            content: Some("流式输出测试".into()),
            tool_calls: vec![],
        }]);

        let mut agent = BaseAgent::from_parts(
            "t",
            "sys",
            Config::default(),
            fake.clone(),
            ToolRegister::new(),
        );
        let mut deltas: Vec<String> = Vec::new();
        let result = agent
            .run_stream("hi", |delta| deltas.push(delta.to_string()))
            .await
            .unwrap();
        assert_eq!(result, "流式输出测试");
        assert_eq!(deltas, vec!["流式输出测试"]);
    }

    #[tokio::test]
    async fn run_reuses_history_across_rounds() {
        let fake = FakeLLM::new(vec![
            ThinkOutput {
                content: Some("第一轮回复".into()),
                tool_calls: vec![],
            },
            ThinkOutput {
                content: Some("第二轮回复".into()),
                tool_calls: vec![],
            },
        ]);

        let mut agent = BaseAgent::from_parts(
            "t",
            "sys",
            Config::default(),
            fake.clone(),
            ToolRegister::new(),
        );

        let r1 = agent.run("你好").await.unwrap();
        assert_eq!(r1, "第一轮回复");

        let r2 = agent.run("再问").await.unwrap();
        assert_eq!(r2, "第二轮回复");

        // 同一实例两轮调用,不重建;历史在 agent 内部累积
        let texts: Vec<&str> = agent.history.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["你好", "第一轮回复", "再问", "第二轮回复"]);

        // 第二轮 LLM 请求 = system + 第一轮 user/assistant + 本轮 user
        let received = fake.received();
        assert_eq!(received.len(), 2);
        let second = &received[1];
        assert_eq!(second.len(), 4);
        assert!(
            second.iter().any(
                |m| matches!(m, ChatCompletionRequestMessage::Assistant(am) if am.content.is_some())
            ),
            "第二轮请求应包含第一轮的 assistant 回复"
        );
    }
}
