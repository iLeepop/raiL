use std::sync::Arc;

use llm::Message;
use serde_json::Value;
use uuid::Uuid;

use crate::error::SessionError;
use crate::event::SessionEvent;
use crate::session::{Session, SessionStatus};
use crate::store::SessionStore;

/// 会话空间:持有会话与存储,记录空间内一切消息与操作。
/// 写操作在会话 `Closed` 后返回 `SessionError::Closed`(只读)。
pub struct SessionSpace<S: SessionStore> {
    session: Session,
    store: Arc<S>,
}

impl<S: SessionStore> SessionSpace<S> {
    /// 新建会话(不落盘;首次 `persist` 时创建)
    pub fn new(store: Arc<S>, title: impl Into<String>) -> Self {
        Self {
            session: Session::new(title),
            store,
        }
    }

    /// 恢复既有会话(可从 store 读出后继续)
    pub fn resume(store: Arc<S>, session: Session) -> Self {
        Self { session, store }
    }

    /// 追加检索元数据(仅对未持久化的新会话有意义)
    pub fn with_meta(mut self, key: impl Into<String>, value: Value) -> Self {
        self.session.metadata.insert(key.into(), value);
        self
    }

    pub fn id(&self) -> Uuid {
        self.session.id
    }

    pub fn title(&self) -> &str {
        &self.session.title
    }

    pub fn messages(&self) -> &[Message] {
        &self.session.messages
    }

    pub fn events(&self) -> &[SessionEvent] {
        &self.session.events
    }

    fn ensure_writable(&self) -> Result<(), SessionError> {
        if self.session.status == SessionStatus::Closed {
            return Err(SessionError::Closed(self.session.id));
        }
        Ok(())
    }

    /// 记录一条对话消息
    pub fn push(&mut self, msg: Message) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.messages.push(msg);
        Ok(())
    }

    /// 记录工具被调用
    pub fn record_tool(&mut self, name: impl Into<String>, arguments: Value) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.events.push(SessionEvent::tool(name, arguments));
        Ok(())
    }

    /// 记录工具成功结果
    pub fn record_tool_result(&mut self, name: impl Into<String>, result: Value) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.events.push(SessionEvent::tool_result(name, result));
        Ok(())
    }

    /// 记录工具失败
    pub fn record_tool_error(&mut self, name: impl Into<String>, error: impl Into<String>) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.events.push(SessionEvent::tool_error(name, error));
        Ok(())
    }

    /// 记录阶段检查点
    pub fn checkpoint(&mut self, label: impl Into<String>, data: Value) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.events.push(SessionEvent::checkpoint(label, data));
        Ok(())
    }

    /// 记录任意自定义事件(逃生口)
    pub fn record(&mut self, event: SessionEvent) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.session.events.push(event);
        Ok(())
    }

    /// 落盘实现(不做只读检查):会话在 store 中已存在则覆写,否则创建。刷新 `updated_at`。
    async fn persist_inner(&mut self) -> Result<(), SessionError> {
        self.session.touch();
        match self.store.get(self.session.id).await? {
            Some(_) => self.store.save(&self.session).await?,
            None => self.store.create(&self.session).await?,
        }
        Ok(())
    }

    /// 显式落盘:会话在 store 中已存在则覆写,否则创建。刷新 `updated_at`。
    /// 非原子(get→判→写);两空间并发首写可能一方报 `AlreadyExists`,重试即可落入 save 分支。
    /// 已关闭会话返回 `SessionError::Closed`(只读)。
    pub async fn persist(&mut self) -> Result<(), SessionError> {
        self.ensure_writable()?;
        self.persist_inner().await
    }

    /// 关闭会话:置 `Closed` 并落盘,返回最终 `Session`。之后空间不可再写。
    /// 落盘失败时返回错误,会话数据随 `self` 一并丢弃(可从 store 中最近一次持久化内容恢复)。
    pub async fn close(mut self) -> Result<Session, SessionError> {
        self.session.status = SessionStatus::Closed;
        self.persist_inner().await?;
        Ok(self.session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::memory::InMemoryStore;
    use llm::Role;

    #[tokio::test]
    async fn persist_creates_then_saves() -> Result<(), SessionError> {
        let store = Arc::new(InMemoryStore::new());
        let mut space = SessionSpace::new(store.clone(), "订单助手")
            .with_meta("model", serde_json::json!("m1"));
        space.push(Message::new(Role::User, "hi"))?;
        space.record_tool("calc", serde_json::json!({"x": 1}))?;
        space.persist().await.unwrap();
        let id = space.id();

        let loaded = store.get(id).await.unwrap().unwrap();
        assert_eq!(loaded.title, "订单助手");
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.metadata.get("model"), Some(&serde_json::json!("m1")));

        // 再次 persist 走 save 分支,不报 AlreadyExists
        space.persist().await.unwrap();
        // updated_at 被刷新
        let reloaded = store.get(id).await.unwrap().unwrap();
        assert!(reloaded.updated_at >= loaded.updated_at);
        Ok(())
    }

    #[tokio::test]
    async fn closed_space_rejects_writes() {
        let store = Arc::new(InMemoryStore::new());
        let space = SessionSpace::new(store.clone(), "t");
        let closed = space.close().await.unwrap();
        assert_eq!(closed.status, SessionStatus::Closed);
        assert!(store.get(closed.id).await.unwrap().is_some());

        let mut resumed = SessionSpace::resume(store.clone(), closed);
        let err = resumed.push(Message::new(Role::User, "x")).unwrap_err();
        assert!(matches!(err, SessionError::Closed(_)));
        let err2 = resumed.record(SessionEvent::custom("x", serde_json::json!(1))).unwrap_err();
        assert!(matches!(err2, SessionError::Closed(_)));
        // 只读访问仍可用
        assert_eq!(resumed.title(), "t");
    }

    #[tokio::test]
    async fn resume_keeps_session_state() -> Result<(), SessionError> {
        let store = Arc::new(InMemoryStore::new());
        let mut space = SessionSpace::new(store.clone(), "原题");
        space.push(Message::new(Role::User, "一"))?;
        space.persist().await.unwrap();
        let id = space.id();

        let loaded = store.get(id).await.unwrap().unwrap();
        let mut resumed = SessionSpace::resume(store.clone(), loaded);
        resumed.push(Message::new(Role::Assistant, "二"))?;
        resumed.persist().await.unwrap();
        let again = store.get(id).await.unwrap().unwrap();
        assert_eq!(again.messages.len(), 2);
        assert_eq!(again.messages[1].text, "二");
        Ok(())
    }

    #[tokio::test]
    async fn closed_session_persist_rejected() -> Result<(), SessionError> {
        let store = Arc::new(InMemoryStore::new());
        let space = SessionSpace::new(store.clone(), "t");
        let closed = space.close().await.unwrap();
        let mut resumed = SessionSpace::resume(store.clone(), closed);
        let err = resumed.persist().await.unwrap_err();
        assert!(matches!(err, SessionError::Closed(_)));
        Ok(())
    }
}
