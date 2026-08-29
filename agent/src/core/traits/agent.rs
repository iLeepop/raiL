use std::error::Error;

use llm::Message;

pub trait Agent {
    /// 运行一轮:用户消息记入历史,返回模型最终回答(同样记入历史)。
    /// 接收 `&mut self`,同一实例可跨轮复用,无需重建。
    fn run<'a>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a;

    /// 流式运行:内容增量逐段回调;默认实现 = run + 一次性回调全文
    fn run_stream<'a, F>(
        &'a mut self,
        message: impl Into<String> + Send + 'a,
        on_delta: F,
    ) -> impl Future<Output = Result<String, Box<dyn Error>>> + Send + 'a
    where
        F: FnMut(&str) + Send + 'a,
        Self: Sized + Send,
    {
        async move {
            let out = self.run(message).await?;
            let mut on_delta = on_delta;
            on_delta(&out);
            Ok(out)
        }
    }

    fn add_message(&mut self, message: Message);

    /// 裁剪历史,只保留最近 `keep` 条消息
    fn truncate(&mut self, keep: usize);

    fn clear_message(&mut self);
}
