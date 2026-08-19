use std::error::Error;

use async_openai::{
    Client,
    config::OpenAIConfig,
    traits::RequestOptionsBuilder,
    types::chat::{
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestToolMessage,
        ChatCompletionRequestUserMessage, CreateChatCompletionRequestArgs,
    },
};

pub enum Role {
    User,
    System,
    Assistant,
    Tool,
}

pub struct Message {
    pub role: Role,
    pub context: &'static str,
}

impl Message {
    pub fn new(role: Role, context: &'static str) -> Self {
        return Message { role, context };
    }

    pub fn to_chat_message(self) -> ChatCompletionRequestMessage {
        match self.role {
            Role::User => return ChatCompletionRequestUserMessage::from(self.context).into(),
            Role::System => return ChatCompletionRequestSystemMessage::from(self.context).into(),
            Role::Assistant => {
                return ChatCompletionRequestAssistantMessage::from(self.context).into();
            }
            Role::Tool => {
                let cm = ChatCompletionRequestToolMessage::default().into();
                // cm.content = self.context.into();
                return cm;
            }
        }
    }
}

pub struct RaiLLM {
    pub provider: &'static str,
    pub base_url: &'static str,
    pub api_key: &'static str,
    pub model_name: &'static str,
}

impl RaiLLM {
    pub fn new(
        provider: &'static str,
        base_url: &'static str,
        api_key: &'static str,
        model_name: &'static str,
    ) -> Self {
        return RaiLLM {
            provider,
            base_url,
            api_key,
            model_name,
        };
    }

    pub async fn think(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        max_tokens: u32,
    ) -> Result<String, Box<dyn Error>> {
        let config = OpenAIConfig::default()
            .with_api_base(self.base_url)
            .with_api_key(self.api_key);

        let client = Client::with_config(config);

        let request = CreateChatCompletionRequestArgs::default()
            .max_tokens(max_tokens)
            .model(self.model_name)
            .messages(messages)
            .build()?;

        let response = client
            .chat()
            .query(&vec![("limit", 10)])?
            .create(request)
            .await?;

        println!("\nResponse:\n");

        for choice in response.choices {
            println!(
                "{}: Role: {}  Content: {:?}",
                choice.index, choice.message.role, choice.message.content
            );
        }

        return Ok(String::from("Thinking"));
    }
}
