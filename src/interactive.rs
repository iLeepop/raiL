//! 终端交互会话:FunctionCall 范式,支持多轮对话与工具调用。

use std::{error::Error, io::Write};

use agent::{
    agent::FunctionCallAgent,
    config::Config,
    core::{Agent, ToolParameters, ToolRegister},
};
use llm::{RaiLLM, RaiLLMArgs};

/// 内置演示工具:`now` — 返回当前 Unix 时间戳(秒)。
pub fn demo_tools() -> ToolRegister {
    let mut tools = ToolRegister::new();
    tools.register(
        "now",
        "获取当前的 Unix 时间戳(秒)",
        ToolParameters::new(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        })),
        |_: serde_json::Value| async move {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Ok(serde_json::json!({ "timestamp": secs }))
        },
    );
    tools
}

/// 在终端运行交互式 FunctionCall 会话,直到输入 `exit` / `quit` / `q` /
/// `退出` / `再见` 或 EOF(Ctrl-D)。
///
/// 会话历史由 agent 内部维护(`run_stream` 自动记录 user/assistant 消息),
/// 实例只建一次、跨轮复用,并按 `config.max_history_len` 裁剪,防止上下文无限膨胀。
///
/// 初始化失败(`RaiLLM` 配置错误,如未设置模型)时返回 `Err`;运行中的模型错误
/// 打印到 stderr 后继续下一轮。
pub async fn run_terminal(config: Config, tools: ToolRegister) -> Result<(), Box<dyn Error>> {
    let model = config
        .default_model
        .clone()
        .ok_or_else(|| "Config::default_model 未设置".to_string())?;

    let llm = RaiLLMArgs::default()
        .with_provider(config.default_provider)
        .with_model_id(model)
        .with_api_key(config.default_api_key.clone())
        .with_base_url(config.default_base_url.clone())
        .build()?;

    let system_prompt = FunctionCallAgent::<RaiLLM>::DEFAULT_SYSTEM_PROMPT;
    // 会话历史由 agent 内部维护(run_stream 自动记录 user/assistant 消息),
    // 实例只建一次,跨轮复用,无需重建
    let mut agent = FunctionCallAgent::from_parts(
        "interactive",
        system_prompt,
        config.clone(),
        llm.clone(),
        tools.clone(),
    );

    println!("=== 交互会话开始,输入 exit / quit / 退出 结束 ===");
    loop {
        print!("你: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(0) => {
                // Ctrl-D / EOF
                println!("\n会话结束。");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("读取输入失败: {e}");
                break;
            }
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if matches!(input, "exit" | "quit" | "q" | "退出" | "再见") {
            println!("会话结束。");
            break;
        }

        print!("Agent: ");
        std::io::stdout().flush()?;
        match agent
            .run_stream(input.to_string(), |delta| {
                print!("{delta}");
                let _ = std::io::stdout().flush();
            })
            .await
        {
            Ok(_) => {
                println!("\n");
                // 本轮 user/assistant 已由 run_stream 记入历史,按配置裁剪,防止上下文无限膨胀
                if let Some(limit) = config.max_history_len {
                    agent.truncate(limit as usize);
                }
            }
            Err(e) => {
                eprintln!("Agent 出错: {e}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn demo_tools_registers_working_now_tool() {
        let tools = demo_tools();
        let out = tools
            .call("now", serde_json::json!({}))
            .await
            .expect("now 工具应已注册")
            .expect("now 工具调用不应失败");
        assert!(out["timestamp"].is_u64() || out["timestamp"].is_i64());
    }
}
