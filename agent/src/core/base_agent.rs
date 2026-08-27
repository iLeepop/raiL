use std::error::Error;

use llm::{Message, RaiLLM, RaiLLMArgs, Role, Think};

use crate::{config::Config, traits::Agent};

pub struct BaseAgent {
    pub name: String,
    pub llm: RaiLLM,
    pub system_prompt: String,
    pub config: Config,
    pub history: Vec<Message>,
}

impl BaseAgent {
    pub fn new(name: impl Into<String>, system_prompt: impl Into<String>, config: Config) -> Self {
        let c = config.clone();

        let rllm = RaiLLMArgs::default()
            .with_provider(c.default_provider)
            .with_model_id(c.default_model.unwrap())
            .with_api_key(c.default_api_key)
            .with_base_url(c.default_base_url)
            .build()
            .expect("RaiLLM 初始化失败");

        BaseAgent {
            name: name.into(),
            llm: rllm,
            system_prompt: system_prompt.into(),
            config,
            history: Vec::<Message>::new(),
        }
    }
}

impl Agent for BaseAgent {
    fn run(
        self,
        input_text: impl Into<String>,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> {
        let user_m = Message::new(Role::User, input_text.into());
        let system_m = Message::new(Role::System, self.system_prompt);

        let messages = vec![
            user_m.to_chat_message().unwrap(),
            system_m.to_chat_message().unwrap(),
        ];
        self.llm.think(
            messages,
            self.config.max_token.unwrap_or(RaiLLM::MAX_TOKENS_SHORT),
        )
    }

    fn add_message(&mut self, message: Message) {
        self.history.push(message);
    }

    fn truncate(&mut self, keep: usize) {
        self.history.truncate(keep);
    }

    fn clear_message(&mut self) {
        self.history.clear();
    }
}
