#[tokio::main]
async fn main() {
    println!("Hello RaiLLM")
}

#[cfg(test)]
mod tests {

    use agent::{
        agent::{BaseAgent, FunctionCallAgent},
        config::Config,
        core::{Agent, ToolParameters, ToolRegister},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use llm::{Message, Provider, RaiLLM, RaiLLMArgs, Role, Think};

    #[tokio::main]
    #[test]
    async fn image_vision_chat() {
        dotenvy::dotenv().ok();

        let rllm = RaiLLM::default()
            .init(
                Some(llm::Provider::VLLM),
                None::<String>,
                None::<String>,
                "Qwen2.5-VL-7B-Instruct",
            )
            .expect("RaiLLM 初始化失败");
        let img_path = "/Users/crf/proj/rs/raiL/files/front.png";
        let img_bytes = std::fs::read(img_path).expect("failed to read image file");
        let img_url = format!("data:image/png;base64,{}", STANDARD.encode(img_bytes));

        let propmt = r#"请获取身份证证件信息中的关键信息，并按照模版进行格式化输出。

        确保不要输出其他多余内容，直接给出JSON格式的结果。

        模版：
        {
            "name": "姓名",
            "id_number": "公民身份号码"
        }

        请开始吧!
        "#;

        let user_m = Message::new(Role::User, propmt).with_image_url(img_url);

        let mesaages = vec![user_m.to_chat_message().unwrap()];

        match rllm.think(mesaages, &[], RaiLLM::MAX_TOKENS_NORMAL).await {
            Ok(r) => {
                println!("{}", r.content.unwrap_or_default());
            }
            Err(e) => {
                eprintln!("{:#?}", e);
            }
        }
    }

    #[tokio::main]
    #[test]
    async fn normal_chat() {
        dotenvy::dotenv().ok();

        let config = Config::default()
            .with_default_provider(Provider::DEEPSEEK)
            .with_default_model("deepseek-v4-flash");

        let mut b_a = BaseAgent::new(
            "normal",
            "快速思考，简短回答，答案保持在200字以内",
            config,
            ToolRegister::new(),
        );

        match b_a.run("猜猜我是谁？我是皇后区的超级英雄！").await {
            Ok(r) => {
                println!("{}", r);
            }
            Err(e) => {
                eprintln!("{:#?}", e);
            }
        }
    }

    /// 终端交互会话:FunctionCall 范式,支持多轮对话与工具调用。
    /// 运行方式:`cargo test interactive_session -- --nocapture`(需要真实模型配置)。
    #[tokio::main]
    #[test]
    async fn interactive_session() {
        use std::io::Write;

        dotenvy::dotenv().ok();

        // 演示工具:返回当前 Unix 时间戳
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

        let config = Config::default()
            .with_default_provider(Provider::DEEPSEEK)
            .with_default_model("deepseek-v4-flash")
            .with_max_history_len(40);

        let llm = RaiLLMArgs::default()
            .with_provider(config.default_provider)
            .with_model_id(config.default_model.clone().unwrap())
            .with_api_key(config.default_api_key.clone())
            .with_base_url(config.default_base_url.clone())
            .build()
            .expect("RaiLLM 初始化失败");

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
            std::io::stdout().flush().unwrap();

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
            std::io::stdout().flush().unwrap();
            match agent
                .run_stream(input.to_string(), |delta| {
                    print!("{delta}");
                    std::io::stdout().flush().unwrap();
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
    }
}
