use std::error::Error;

use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestMessageContentPartImage, ChatCompletionRequestMessageContentPartText,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestToolMessage,
    ChatCompletionRequestUserMessageArgs, ChatCompletionRequestUserMessageContentPart, ImageUrl,
};

use crate::enums::Role;

pub struct Message {
    pub role: Role,
    pub text: String,
    pub image_url: Option<String>,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        return Message {
            role,
            text: text.into(),
            image_url: None,
        };
    }

    pub fn with_image_url(mut self, image_url: impl Into<String>) -> Self {
        self.image_url = Some(image_url.into());
        return self;
    }

    pub fn to_chat_message(self) -> Result<ChatCompletionRequestMessage, Box<dyn Error>> {
        match self.role {
            Role::User => {
                let mut m_content: Vec<ChatCompletionRequestUserMessageContentPart> =
                    vec![ChatCompletionRequestMessageContentPartText::from(self.text).into()];
                if let Some(image_url) = self.image_url {
                    m_content.push(
                        ChatCompletionRequestMessageContentPartImage::from(ImageUrl {
                            url: image_url.to_string(),
                            ..Default::default()
                        })
                        .into(),
                    );
                }
                let m = ChatCompletionRequestUserMessageArgs::default()
                    .content(m_content)
                    .build()?
                    .into();

                return Ok(m);
            }
            Role::System => return Ok(ChatCompletionRequestSystemMessage::from(self.text).into()),
            Role::Assistant => {
                return Ok(ChatCompletionRequestAssistantMessage::from(self.text).into());
            }
            Role::Tool => {
                let cm = ChatCompletionRequestToolMessage::default().into();
                return Ok(cm);
            }
        }
    }
}
