use std::error::Error;

use async_openai::types::chat::ChatCompletionRequestMessage;

pub trait Think {
    async fn think(
        self,
        messages: Vec<ChatCompletionRequestMessage>,
        max_tokens: u32,
    ) -> Result<String, Box<dyn Error>>;
}
