use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use llm::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::event::SessionEvent;

/// 会话生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Closed,
}

/// 会话聚合:消息 + 操作留痕 + 检索元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub title: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    /// 最近一次持久化的时间;`SessionSpace::persist` 负责刷新
    pub updated_at: DateTime<Utc>,
    /// 检索外挂信息:agent 名、模型、标签等
    pub metadata: BTreeMap<String, Value>,
    /// 对话消息(直接可喂 LLM)
    pub messages: Vec<Message>,
    /// 操作留痕
    pub events: Vec<SessionEvent>,
}

impl Session {
    pub fn new(title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
            metadata: BTreeMap::new(),
            messages: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// 列表页轻量视图,不携带全量消息/事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub title: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub event_count: usize,
}

impl From<&Session> for SessionSummary {
    fn from(s: &Session) -> Self {
        Self {
            id: s.id,
            title: s.title.clone(),
            status: s.status,
            created_at: s.created_at,
            updated_at: s.updated_at,
            message_count: s.messages.len(),
            event_count: s.events.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::{Message, Role};

    #[test]
    fn new_session_defaults() {
        let s = Session::new("订单助手");
        assert_eq!(s.title, "订单助手");
        assert_eq!(s.status, SessionStatus::Active);
        assert!(s.messages.is_empty());
        assert!(s.events.is_empty());
        assert!(s.metadata.is_empty());
        assert!(s.created_at.timestamp() > 0);
        assert_eq!(s.updated_at, s.created_at);
    }

    #[test]
    fn summary_from_session_counts() {
        let mut s = Session::new("t");
        s.messages.push(Message::new(Role::User, "hi"));
        s.messages.push(Message::new(Role::Assistant, "yo"));
        s.events
            .push(SessionEvent::checkpoint("c", serde_json::json!(null)));
        let sum = SessionSummary::from(&s);
        assert_eq!(sum.id, s.id);
        assert_eq!(sum.title, "t");
        assert_eq!(sum.status, SessionStatus::Active);
        assert_eq!(sum.message_count, 2);
        assert_eq!(sum.event_count, 1);
    }

    #[test]
    fn session_json_roundtrip() {
        let mut s = Session::new("roundtrip");
        s.messages.push(Message::new(Role::User, "hi"));
        s.events
            .push(SessionEvent::tool("calc", serde_json::json!({"x": 1})));
        s.metadata.insert("model".into(), serde_json::json!("m1"));
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "roundtrip");
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.metadata.get("model"), Some(&serde_json::json!("m1")));
        assert_eq!(back.id, s.id);
        assert_eq!(back.status, s.status);
        assert_eq!(back.created_at, s.created_at);
        assert_eq!(back.updated_at, s.updated_at);
    }
}
