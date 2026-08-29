use std::error::Error;

use llm::{Message, RaiLLM, Think};

use crate::{
    config::Config,
    core::{Agent, ToolRegister},
};

use super::base_agent::BaseAgent;

/// Function Calling 范式:依赖模型原生 tool_calls 完成工具调用。
/// 执行内核即 BaseAgent,这里提供专门的函数调用系统提示词与构造入口。
pub struct FunctionCallAgent<L = RaiLLM> {
    pub core: BaseAgent<L>,
}

impl FunctionCallAgent<RaiLLM> {
    pub const DEFAULT_SYSTEM_PROMPT: &'static str = "你是一个通过函数调用(Function Calling)解决任务的智能体。\n\n系统会动态提供可用工具及其 JSON Schema。需要获取信息或执行操作时,以 tool_calls 请求调用工具;信息齐备后直接给出最终回答。\n\n规则:\n1. 严格按 JSON Schema 生成参数:不添加 schema 之外的字段,不省略必填字段\n2. 多个互不依赖的工具调用可以一次并行发起\n3. 依据工具返回结果推理;工具报错时修正参数后重试\n4. 不需要工具时不要调用,直接回答";

    pub fn new(name: impl Into<String>, config: Config, tools: ToolRegister) -> Self {
        Self {
            core: BaseAgent::new(name, Self::DEFAULT_SYSTEM_PROMPT, config, tools),
        }
    }
}

impl<L: Think> FunctionCallAgent<L> {
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

impl<L: Think + Clone + Send> Agent for FunctionCallAgent<L> {
    fn run<'a>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a {
        async move { self.core.run(message).await }
    }

    fn run_stream<'a, F>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
        on_delta: F,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a
    where
        F: FnMut(&str) + Send + 'a,
    {
        async move { self.core.run_stream(message, on_delta).await }
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

#[cfg(test)]
mod tests {
    use llm::{Provider, ThinkOutput};

    use super::*;
    use crate::agent::test_fake::FakeLLM;

    #[tokio::test]
    async fn function_call_delegates_to_core() {
        let fake = FakeLLM::new(vec![ThinkOutput {
            content: Some("你好".into()),
            tool_calls: vec![],
        }]);
        let mut agent = FunctionCallAgent::from_parts(
            "f",
            "sys",
            Config::default(),
            fake.clone(),
            ToolRegister::new(),
        );
        let result = agent.run("hi").await.unwrap();
        assert_eq!(result, "你好");
        assert_eq!(fake.received().len(), 1);
    }

    #[test]
    fn default_prompt_is_function_calling() {
        let config = Config::default()
            .with_default_provider(Provider::DEEPSEEK)
            .with_default_model("deepseek-v4-flash")
            .with_default_api_key("sk-test");
        let agent = FunctionCallAgent::new("f", config, ToolRegister::new());
        assert_eq!(
            agent.core.system_prompt,
            FunctionCallAgent::<RaiLLM>::DEFAULT_SYSTEM_PROMPT
        );
    }
}
