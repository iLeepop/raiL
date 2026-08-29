use std::error::Error;

use llm::{Message, RaiLLM, Role, Think};

use crate::{
    config::Config,
    core::{Agent, ToolRegister},
};

use super::base_agent::BaseAgent;
use super::runtime;

/// Plan-and-Resolve 范式:先输出有序步骤清单,再逐步执行(可调用工具),最后整合验证。
pub struct PlanAndResolveAgent<L = RaiLLM> {
    pub core: BaseAgent<L>,
}

impl PlanAndResolveAgent<RaiLLM> {
    pub const DEFAULT_SYSTEM_PROMPT: &'static str = "你是一个采用 Plan-and-Resolve 范式的智能体,处理复杂任务时按三步走:\n\n1. 规划:把任务拆解为有序步骤,每行一步,格式 \"Step N: 描述\";步骤要具体、可独立执行\n2. 执行:逐步骤执行;需要外部信息或操作时调用工具,依据工具返回结果继续\n3. 收尾:检查所有步骤是否完成、结果是否自洽;发现遗漏或失败时补充执行,最后整合为完整答案\n\n规则:\n- 计划宁细勿粗,不跳过必要步骤\n- 每步只处理本步内容,结果必须基于实际工具返回,不虚构\n- 最终答案要整合所有步骤的结果,而不是复述计划";

    pub fn new(name: impl Into<String>, config: Config, tools: ToolRegister) -> Self {
        Self {
            core: BaseAgent::new(name, Self::DEFAULT_SYSTEM_PROMPT, config, tools),
        }
    }
}

impl<L: Think> PlanAndResolveAgent<L> {
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

impl<L: Think + Clone + Send> Agent for PlanAndResolveAgent<L> {
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
            let tools_schemas = self.core.tools.schemas();

            // Phase 1: 规划
            let plan_text = runtime::ask(self.core.llm.clone(), &messages,
            "请为上面的任务制定执行计划。输出格式:每行一步,以 \"Step N: 描述\" 开头,不超过 8 步。只列计划,不要执行。",
            &tools_schemas,
            max_tokens,)
            .await?;
            messages.push(Message::new(Role::Assistant, plan_text.clone()));
            let steps = parse_steps(&plan_text);

            // Phase 2: 逐步执行(原生工具调用,直到该步不再请求工具)
            for (i, step) in steps.iter().enumerate() {
                messages.push(Message::new(
                    Role::User,
                    format!("请执行第 {} 步: {}", i + 1, step),
                ));
                for _ in 0..self.core.config.max_tool_rounds {
                    let chat = runtime::to_chat(&messages)?;
                    let out = self
                        .core
                        .llm
                        .clone()
                        .think(chat, &tools_schemas, max_tokens)
                        .await?;
                    if out.tool_calls.is_empty() {
                        messages.push(Message::new(
                            Role::Assistant,
                            out.content.unwrap_or_default(),
                        ));
                        break;
                    }
                    runtime::execute_tool_calls(&mut messages, out, &self.core.tools).await;
                }
            }

            // Phase 3: 收尾整合,发现遗漏由模型补充
            let final_text = runtime::ask(self.core.llm.clone(), &messages,
            "请检查以上所有步骤的结果:是否全部完成、结果是否自洽。若完成,输出整合后的最终答案;若发现遗漏或失败,说明并补充。",
            &tools_schemas,
            max_tokens,)
            .await?;
            self.core
                .history
                .push(Message::new(Role::Assistant, final_text.clone()));
            Ok(final_text)
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

/// 解析计划文本中的步骤清单;没有可识别步骤时整体视为一步
fn parse_steps(plan: &str) -> Vec<String> {
    let steps: Vec<String> = plan
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.is_empty() {
                return None;
            }
            let desc = t
                .strip_prefix("Step ")
                .and_then(|s| s.split_once(':').or_else(|| s.split_once('：')))
                .map(|(_, d)| d.trim().to_string())
                .or_else(|| {
                    t.split_once('.').and_then(|(n, d)| {
                        n.trim().parse::<u32>().ok().map(|_| d.trim().to_string())
                    })
                });
            desc.filter(|d| !d.is_empty())
        })
        .collect();
    if steps.is_empty() {
        vec![plan.trim().to_string()]
    } else {
        steps
    }
}

#[cfg(test)]
mod tests {
    use llm::ThinkOutput;

    use super::*;
    use crate::agent::test_fake::FakeLLM;

    #[test]
    fn steps_parsed_from_plan() {
        let plan = "Step 1: 查询天气\nStep 2: 计算温差";
        assert_eq!(parse_steps(plan), vec!["查询天气", "计算温差"]);
    }

    #[test]
    fn steps_fallback_to_single() {
        assert_eq!(parse_steps("直接回答"), vec!["直接回答"]);
    }

    #[tokio::test]
    async fn plan_execute_resolve_flow() {
        let fake = FakeLLM::new(vec![
            // 计划
            ThinkOutput {
                content: Some("Step 1: 取数\nStep 2: 汇总".into()),
                tool_calls: vec![],
            },
            // 执行第 1 步
            ThinkOutput {
                content: Some("步骤 1 完成".into()),
                tool_calls: vec![],
            },
            // 执行第 2 步
            ThinkOutput {
                content: Some("步骤 2 完成".into()),
                tool_calls: vec![],
            },
            // 收尾整合
            ThinkOutput {
                content: Some("最终结果: 42".into()),
                tool_calls: vec![],
            },
        ]);

        let mut agent = PlanAndResolveAgent::from_parts(
            "p",
            "sys",
            Config::default(),
            fake.clone(),
            ToolRegister::new(),
        );
        let result = agent.run("任务").await.unwrap();
        assert_eq!(result, "最终结果: 42");
        assert_eq!(fake.received().len(), 4);
    }
}
