use agent::config::Config;
use llm::Provider;
use rai_l::interactive;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let config = Config::default()
        .with_default_provider(Provider::DEEPSEEK)
        .with_default_model("deepseek-v4-flash")
        .with_max_history_len(40);

    interactive::run_terminal(config, interactive::demo_tools()).await
}

#[cfg(test)]
mod tests {

    use agent::{
        agent::BaseAgent,
        config::Config,
        core::{Agent, ToolRegister},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use llm::{Message, Provider, RaiLLM, Role, Think};

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
}
