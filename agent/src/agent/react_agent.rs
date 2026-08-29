use std::error::Error;

use llm::{Message, RaiLLM, Role, Think};

use crate::{
    config::Config,
    core::{Agent, ToolError, ToolRegister},
};

use super::base_agent::BaseAgent;
use super::runtime;

/// ReAct(Reasoning + Acting)范式:Thought → Action → Observation 文本协议循环,
/// 适合不支持原生 function calling 的模型。
pub struct ReActAgent<L = RaiLLM> {
    pub core: BaseAgent<L>,
}

impl ReActAgent<RaiLLM> {
    pub const DEFAULT_SYSTEM_PROMPT: &'static str = "你是一个采用 ReAct(Reasoning + Acting)范式的智能体,通过\"思考 → 行动 → 观察\"的循环解决问题。\n\n输出格式(严格遵循):\n- Thought:用自然语言描述你的推理过程\n- Action:要调用的工具名(仅限提供的工具)\n- Action Input:该工具的 JSON 参数(与工具的 JSON Schema 一致)\n- 工具执行结果会以 \"Observation: <结果>\" 的形式提供,基于它继续思考\n- 不再需要工具时,输出 \"Final Answer: <最终答案>\"\n\n规则:\n1. 每轮先输出 Thought,再输出 Action 和 Action Input\n2. Action Input 必须是合法 JSON,只能包含 schema 定义的字段\n3. 只能使用提供的工具,不得编造工具名或虚构工具结果\n4. 工具报错时,根据错误信息修正参数或换工具后重试\n5. 信息足够后立刻输出 Final Answer,不要继续调用工具";

    pub fn new(name: impl Into<String>, config: Config, tools: ToolRegister) -> Self {
        Self {
            core: BaseAgent::new(name, Self::DEFAULT_SYSTEM_PROMPT, config, tools),
        }
    }
}

impl<L: Think> ReActAgent<L> {
    pub fn from_parts(
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        config: Config,
        llm: L,
        tools: ToolRegister,
    ) -> Self {
        Self {
            core: BaseAgent::from_parts(name, system_prompt, config, llm, tools),
        }
    }
}

impl<L: Think + Clone + Send> Agent for ReActAgent<L> {
    fn run<'a>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a {
        async move {
            let message = message.into();
            let mut messages = runtime::chat_history(
                self.core.system_prompt.clone(),
                self.core.history.clone(),
                message.clone(),
            );
            self.core.history.push(Message::new(Role::User, message));
            let max_tokens = self
                .core
                .config
                .max_token
                .unwrap_or(RaiLLM::MAX_TOKENS_NORMAL);

            for _ in 0..self.core.config.max_tool_rounds {
                let chat = runtime::to_chat(&messages)?;
                let out = self.core.llm.clone().think(chat, &[], max_tokens).await?;
                let text = out.content.unwrap_or_default();

                // 模型明确输出 Final Answer:直接结束
                if let Some(answer) = extract_final_answer(&text) {
                    self.core
                        .history
                        .push(Message::new(Role::Assistant, answer.clone()));
                    return Ok(answer);
                }

                match parse_react_action(&text) {
                    // 模型请求执行工具:保留 Thought,执行并把结果作为 Observation 回填
                    Some((name, args)) => {
                        messages.push(Message::new(Role::Assistant, text));
                        let result_text = match serde_json::from_str(&args) {
                            Ok(v) => match self.core.tools.call(&name, v).await {
                                Ok(Ok(v)) => v.to_string(),
                                Ok(Err(e)) => e.to_string(),
                                Err(ToolError::NotFound(_)) => format!("unknown tool: {name}"),
                            },
                            Err(e) => format!("arguments 不是合法 JSON: {e}"),
                        };
                        messages.push(Message::new(
                            Role::User,
                            format!("Observation: {result_text}"),
                        ));
                    }
                    // 没有 Action → 视为最终回答
                    None => {
                        self.core
                            .history
                            .push(Message::new(Role::Assistant, text.clone()));
                        return Ok(text);
                    }
                }
            }

            Err(format!(
                "ReAct 循环超过最大轮数 ({})",
                self.core.config.max_tool_rounds
            )
            .into())
        }
    }

    fn add_message(&mut self, message: Message) {
        self.core.add_message(message);
    }

    fn truncate(&mut self, keep: usize) {
        self.core.truncate(keep);
    }

    fn clear_message(&mut self) {
        self.core.clear_message();
    }
}

/// 提取 "Final Answer: xxx" 之后的内容
fn extract_final_answer(text: &str) -> Option<String> {
    const MARK: &str = "Final Answer:";
    let idx = text.find(MARK)?;
    let rest = text[idx + MARK.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// 解析最后一组 "Action: <name>" + 下一行 "Action Input: <json>"
fn parse_react_action(text: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut action: Option<(String, String)> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(name) = trimmed.strip_prefix("Action:") else {
            continue;
        };
        let name = name.trim().to_string();
        let args = lines
            .get(i + 1)
            .and_then(|l| l.trim().strip_prefix("Action Input:"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        action = Some((name, args));
    }
    action
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use crate::core::ToolParameters;
    use llm::ThinkOutput;

    use super::*;
    use crate::agent::test_fake::FakeLLM;

    #[test]
    fn parse_react_basic() {
        let text =
            "Thought: 需要查询天气。\nAction: get_weather\nAction Input: {\"city\": \"北京\"}";
        let (name, args) = parse_react_action(text).unwrap();
        assert_eq!(name, "get_weather");
        assert_eq!(args, r#"{"city": "北京"}"#);
    }

    #[test]
    fn parse_react_takes_last_action() {
        let text = "Action: a\nAction Input: {}\nAction: b\nAction Input: {\"x\": 1}";
        let (name, args) = parse_react_action(text).unwrap();
        assert_eq!(name, "b");
        assert_eq!(args, r#"{"x": 1}"#);
    }

    #[test]
    fn parse_react_none_without_action() {
        assert!(parse_react_action("Thought: 直接回答即可。").is_none());
    }

    #[test]
    fn final_answer_extracted() {
        let text = "Thought: 完成。\nFinal Answer: 北京今天 25°C";
        assert_eq!(extract_final_answer(text).unwrap(), "北京今天 25°C");
    }

    #[tokio::test]
    async fn react_loop_executes_tool_then_answers() {
        let called = Arc::new(AtomicBool::new(false));

        let mut reg = ToolRegister::new();
        let flag = called.clone();
        reg.register(
            "ping",
            "返回 pong",
            ToolParameters::new(serde_json::json!({})),
            move |_: serde_json::Value| {
                let flag = flag.clone();
                async move {
                    flag.store(true, Ordering::SeqCst);
                    Ok(serde_json::json!({ "pong": true }))
                }
            },
        );

        let fake = FakeLLM::new(vec![
            ThinkOutput {
                content: Some("Thought: 先 ping。\nAction: ping\nAction Input: {}".into()),
                tool_calls: vec![],
            },
            ThinkOutput {
                content: Some("Final Answer: pong 完成".into()),
                tool_calls: vec![],
            },
        ]);

        let mut agent = ReActAgent::from_parts("r", "sys", Config::default(), fake.clone(), reg);
        let result = agent.run("测试").await.unwrap();
        assert_eq!(result, "pong 完成");
        assert!(called.load(Ordering::SeqCst), "工具应当被执行");
        // 行动轮 + 回答轮
        assert_eq!(fake.received().len(), 2);
    }

    #[tokio::test]
    async fn react_loop_stops_without_action() {
        let fake = FakeLLM::new(vec![ThinkOutput {
            content: Some("直接回答:42".into()),
            tool_calls: vec![],
        }]);
        let mut agent =
            ReActAgent::from_parts("r", "sys", Config::default(), fake, ToolRegister::new());
        let result = agent.run("问题").await.unwrap();
        assert_eq!(result, "直接回答:42");
    }
}
