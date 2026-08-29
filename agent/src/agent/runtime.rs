use std::error::Error;

use llm::{ChatCompletionRequestMessage, ChatCompletionTools, Message, Role, Think, ThinkOutput};

use crate::core::{ToolError, ToolRegister};

/// 组装消息:system 提示 + 既有历史 + 用户输入(与 BaseAgent 一致)
pub(crate) fn chat_history(
    system_prompt: String,
    history: Vec<Message>,
    user_input: impl Into<String>,
) -> Vec<Message> {
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(Message::new(Role::System, system_prompt));
    messages.extend(history);
    messages.push(Message::new(Role::User, user_input));
    messages
}

/// Message 列表 → ChatCompletionRequestMessage 列表
pub(crate) fn to_chat(
    messages: &[Message],
) -> Result<Vec<ChatCompletionRequestMessage>, Box<dyn Error>> {
    messages
        .iter()
        .cloned()
        .map(Message::to_chat_message)
        .collect()
}

/// 追加一条 User 提示并询问模型,返回文本内容(工具调用由调用方处理)
pub(crate) async fn ask<L: Think + Clone + Send>(
    llm: L,
    messages: &[Message],
    prompt: impl Into<String>,
    tools: &[ChatCompletionTools],
    max_tokens: u32,
) -> Result<String, Box<dyn Error>> {
    let mut msgs = messages.to_vec();
    msgs.push(Message::new(Role::User, prompt));
    let chat = to_chat(&msgs)?;
    let out = llm.think(chat, tools, max_tokens).await?;
    Ok(out.content.unwrap_or_default())
}

/// 原生 tool_calls 执行:回显 assistant 的 tool_calls → 逐个执行 → 回填 Tool 结果;
/// 工具错误/非法 JSON 以文本回填,让 LLM 自行纠正
pub(crate) async fn execute_tool_calls(
    history: &mut Vec<Message>,
    output: ThinkOutput,
    tools: &ToolRegister,
) {
    // 回显 assistant 的 tool_calls(OpenAI 协议要求 tool 消息前有对应 tool_call)
    history.push(
        Message::new(Role::Assistant, output.content.clone().unwrap_or_default())
            .with_tool_calls(output.tool_calls.clone()),
    );

    for tc in output.tool_calls {
        let args: serde_json::Value = match serde_json::from_str(&tc.arguments) {
            Ok(v) => v,
            Err(e) => {
                history.push(
                    Message::new(Role::Tool, format!("arguments 不是合法 JSON: {e}"))
                        .with_tool_call_id(tc.id),
                );
                continue;
            }
        };
        let result_text = match tools.call(&tc.name, args).await {
            Ok(Ok(v)) => v.to_string(),
            Ok(Err(e)) => e.to_string(),
            Err(ToolError::NotFound(_)) => format!("unknown tool: {}", tc.name),
        };
        history.push(Message::new(Role::Tool, result_text).with_tool_call_id(tc.id));
    }
}
