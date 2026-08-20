#[tokio::main]
async fn main() {
    println!("Hello RaiLLM")
}

#[cfg(test)]
mod tests {

    use base64::{Engine, engine::general_purpose::STANDARD};
    use llm::{Message, RaiLLM, Role, Think};

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

        match rllm.think(mesaages, 512u32).await {
            Ok(r) => {
                println!("{}", r);
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

        let rllm = RaiLLM::default()
            .init(
                Some(llm::Provider::DEEPSEEK),
                None::<String>,
                None::<String>,
                "deepseek-v4-flash",
            )
            .expect("RaiLLM 初始化失败");

        let propmt = r#"你是谁?"#;

        let user_m = Message::new(Role::User, propmt);

        let mesaages = vec![user_m.to_chat_message().unwrap()];

        match rllm.think(mesaages, 512u32).await {
            Ok(r) => {
                println!("{}", r);
            }
            Err(e) => {
                eprintln!("{:#?}", e);
            }
        }
    }
}
