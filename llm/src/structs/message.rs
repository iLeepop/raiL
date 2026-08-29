use std::{
    collections::HashMap,
    error::Error,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
    ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
    ChatCompletionRequestUserMessageContentPart, FunctionCall, ImageUrl,
};

use crate::enums::Role;
use crate::traits::ToolCall;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub image_url: Option<String>,
    pub timestamp: Duration,
    pub meta_data: Option<HashMap<String, String>>,
    /// Role::Tool 时必填:对应的 assistant tool_call id
    pub tool_call_id: Option<String>,
    /// Role::Assistant 时携带:模型发起的工具调用(回显给 API 用)
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        return Message {
            role,
            text: text.into(),
            image_url: None,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
            meta_data: None,
            tool_call_id: None,
            tool_calls: None,
        };
    }

    pub fn with_image_url(mut self, image_url: impl Into<String>) -> Self {
        self.image_url = Some(image_url.into());
        self
    }

    pub fn with_meta_data(mut self, meta_datas: HashMap<String, String>) -> Self {
        self.meta_data = Some(meta_datas);
        self
    }

    pub fn with_tool_call_id(mut self, tool_call_id: impl Into<String>) -> Self {
        self.tool_call_id = Some(tool_call_id.into());
        self
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = Some(tool_calls);
        self
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
                if let Some(tool_calls) = self.tool_calls {
                    let tool_calls: Vec<_> = tool_calls
                        .into_iter()
                        .map(|tc| {
                            ChatCompletionMessageToolCalls::Function(
                                ChatCompletionMessageToolCall {
                                    id: tc.id,
                                    function: FunctionCall {
                                        name: tc.name,
                                        arguments: tc.arguments,
                                    },
                                },
                            )
                        })
                        .collect();
                    let mut args = ChatCompletionRequestAssistantMessageArgs::default();
                    args.tool_calls(tool_calls);
                    if !self.text.is_empty() {
                        args.content(self.text);
                    }
                    return Ok(args.build()?.into());
                }
                return Ok(ChatCompletionRequestAssistantMessage::from(self.text).into());
            }
            Role::Tool => {
                let tool_call_id = self
                    .tool_call_id
                    .ok_or_else(|| "Role::Tool 消息缺少 tool_call_id".to_string())?;
                let cm = ChatCompletionRequestToolMessageArgs::default()
                    .tool_call_id(tool_call_id)
                    .content(self.text)
                    .build()?;
                return Ok(cm.into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ToolCall;

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message::new(Role::User, "你好").with_image_url("data:image/png;base64,xx");
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "你好");
        assert!(matches!(back.role, Role::User));
        assert_eq!(back.image_url.as_deref(), Some("data:image/png;base64,xx"));
    }

    #[test]
    fn message_with_tool_calls_roundtrip() {
        let msg = Message::new(Role::Assistant, "").with_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            name: "calc".into(),
            arguments: "{\"x\":1}".into(),
        }]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        let tcs = back.tool_calls.expect("tool_calls 应存在");
        assert_eq!(tcs[0].name, "calc");
    }
}
