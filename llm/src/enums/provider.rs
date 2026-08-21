#[derive(Debug, Clone, Copy)]
pub enum Provider {
    AUTO,
    LOCAL,
    VLLM,
    OLLAMA,
    DEEPSEEK,
    KIMI,
}
