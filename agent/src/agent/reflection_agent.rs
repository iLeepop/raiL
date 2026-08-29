use std::error::Error;

use llm::{Message, RaiLLM, Role, Think};

use crate::{
    config::Config,
    core::{Agent, ToolRegister},
};

use super::base_agent::BaseAgent;
use super::runtime;

/// Reflection 范式:"生成 → 反思 → 修正" 循环,不依赖工具,靠自我批判提升回答质量。
pub struct ReflectionAgent<L = RaiLLM> {
    pub core: BaseAgent<L>,
}

impl ReflectionAgent<RaiLLM> {
    pub const DEFAULT_SYSTEM_PROMPT: &'static str = "你是一个采用 Reflection 范式的智能体,通过\"生成 → 反思 → 修正\"的循环给出高质量回答。\n\n工作方式:\n1. 生成:针对用户问题给出初步回答\n2. 反思:批判性审查当前回答,指出事实错误、逻辑漏洞、遗漏信息与不确定之处\n3. 修正:根据反思意见重写回答,输出完整独立的版本\n\n原则:\n- 反思意见要具体可执行:指出哪里错、为什么错、怎么改\n- 修正后的回答必须完整,不依赖之前的草稿\n- 不确定的事实要明确标注,绝不编造\n- 当回答准确、完整、无重大缺陷时,只回复 APPROVED 结束循环";

    pub fn new(name: impl Into<String>, config: Config) -> Self {
        Self {
            core: BaseAgent::new(
                name,
                Self::DEFAULT_SYSTEM_PROMPT,
                config,
                ToolRegister::new(),
            ),
        }
    }
}

impl<L: Think> ReflectionAgent<L> {
    pub fn from_parts(
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        config: Config,
        llm: L,
    ) -> Self {
        Self {
            core: BaseAgent::from_parts(name, system_prompt, config, llm, ToolRegister::new()),
        }
    }
}

impl<L: Think + Clone + Send> Agent for ReflectionAgent<L> {
    fn run<'a>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a {
        async move {
            let message = message.into();
            let messages = runtime::chat_history(
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

            // 1. 生成初步回答
            let mut answer = runtime::ask(
                self.core.llm.clone(),
                &messages,
                "请针对上面的问题给出你的初步回答。",
                &[],
                max_tokens,
            )
            .await?;

            // 2. 反思循环:批判 → 通过则结束,否则修正后继续
            for _ in 0..self.core.config.max_reflection_rounds {
                let critique = runtime::ask(self.core.llm.clone(), &messages,
                format!(
                    "这是当前回答:\n\n{answer}\n\n请批判性审查:列出事实错误、逻辑漏洞、遗漏信息与不确定之处。如果回答已经准确完整,只回复 APPROVED。"
                ),
                &[],
                max_tokens,)
                .await?;

                if critique.trim().eq_ignore_ascii_case("APPROVED") {
                    self.core
                        .history
                        .push(Message::new(Role::Assistant, answer.clone()));
                    return Ok(answer);
                }

                answer = runtime::ask(self.core.llm.clone(), &messages,
                format!(
                    "根据以下审查意见,重写回答。直接输出修正后的完整答案,不要解释过程。\n\n审查意见:\n{critique}\n\n当前回答:\n{answer}"
                ),
                &[],
                max_tokens,)
                .await?;
            }

            self.core
                .history
                .push(Message::new(Role::Assistant, answer.clone()));
            Ok(answer)
        }
    }

    fn add_message(&mut self, message: llm::Message) {
        self.core.add_message(message);
    }

    fn truncate(&mut self, keep: usize) {
        self.core.truncate(keep);
    }

    fn clear_message(&mut self) {
        self.core.clear_message();
    }
}

#[cfg(test)]
mod tests {
    use llm::ThinkOutput;

    use super::*;
    use crate::agent::test_fake::FakeLLM;

    #[tokio::test]
    async fn reflection_stops_on_approval() {
        let fake = FakeLLM::new(vec![
            ThinkOutput {
                content: Some("初步回答 v1".into()),
                tool_calls: vec![],
            },
            ThinkOutput {
                content: Some("APPROVED".into()),
                tool_calls: vec![],
            },
        ]);
        let mut agent = ReflectionAgent::from_parts("r", "sys", Config::default(), fake.clone());
        let result = agent.run("问题").await.unwrap();
        assert_eq!(result, "初步回答 v1");
        assert_eq!(fake.received().len(), 2);
    }

    #[tokio::test]
    async fn reflection_revises_until_approved() {
        let fake = FakeLLM::new(vec![
            ThinkOutput {
                content: Some("v1".into()),
                tool_calls: vec![],
            },
            // 批判意见:不通过
            ThinkOutput {
                content: Some("缺少事实来源。".into()),
                tool_calls: vec![],
            },
            // 修正版
            ThinkOutput {
                content: Some("v2 修正版".into()),
                tool_calls: vec![],
            },
            // 通过
            ThinkOutput {
                content: Some("APPROVED".into()),
                tool_calls: vec![],
            },
        ]);
        let mut agent = ReflectionAgent::from_parts("r", "sys", Config::default(), fake.clone());
        let result = agent.run("问题").await.unwrap();
        assert_eq!(result, "v2 修正版");
        assert_eq!(fake.received().len(), 4);
    }
}
