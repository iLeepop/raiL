use llm::{Message, RaiLLM, Role};

#[tokio::main]
async fn main() {
    let rllm = RaiLLM::new(
        "vllm",
        "http://10.110.110.9:8000/v1",
        "ailab-sk-ZG6OxdKdJJR_HBHtCpsm1BOTYPDgqFGl0dPpQ7ulmcY",
        "Qwen2.5-VL-7B-Instruct",
    );

    let mesaages = vec![Message::new(Role::User, "我是谁？").to_chat_message()];

    if let Ok(r) = rllm.think(mesaages, 512u32).await {
        println!("Ok");
        println!("{}", r);
    }
}
