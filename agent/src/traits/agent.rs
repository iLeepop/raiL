use std::error::Error;

use llm::Message;

pub trait Agent {
    async fn run(self, message: impl Into<String>) -> Result<String, Box<dyn Error>>;

    fn add_message(&mut self, message: Message);

    fn truncate(&mut self, keep: usize);

    fn clear_message(&mut self);
}
