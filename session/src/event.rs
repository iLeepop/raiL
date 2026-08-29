use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会话内一次操作留痕。消息走 `Session::messages`,`SessionEvent` 记录非消息操作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionEvent {
    /// 工具被调用
    ToolCalled {
        tool: String,
        arguments: Value,
        occurred_at: DateTime<Utc>,
    },
    /// 工具执行结果(或错误)
    ToolResult {
        tool: String,
        result: Value,
        error: Option<String>,
        occurred_at: DateTime<Utc>,
    },
    /// 阶段标记/快照点
    Checkpoint {
        label: String,
        data: Value,
        occurred_at: DateTime<Utc>,
    },
    /// 任意自定义事件
    Custom {
        kind: String,
        data: Value,
        occurred_at: DateTime<Utc>,
    },
}

impl SessionEvent {
    pub fn tool(name: impl Into<String>, arguments: Value) -> Self {
        Self::ToolCalled { tool: name.into(), arguments, occurred_at: Utc::now() }
    }

    pub fn tool_result(name: impl Into<String>, result: Value) -> Self {
        Self::ToolResult { tool: name.into(), result, error: None, occurred_at: Utc::now() }
    }

    pub fn tool_error(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self::ToolResult { tool: name.into(), result: Value::Null, error: Some(error.into()), occurred_at: Utc::now() }
    }

    pub fn checkpoint(label: impl Into<String>, data: Value) -> Self {
        Self::Checkpoint { label: label.into(), data, occurred_at: Utc::now() }
    }

    pub fn custom(kind: impl Into<String>, data: Value) -> Self {
        Self::Custom { kind: kind.into(), data, occurred_at: Utc::now() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_json_roundtrip() {
        let ev = SessionEvent::tool("search", serde_json::json!({"q": "订单"}));
        let json = serde_json::to_string(&ev).unwrap();
        let back: SessionEvent = serde_json::from_str(&json).unwrap();
        match back {
            SessionEvent::ToolCalled { tool, arguments, occurred_at } => {
                assert_eq!(tool, "search");
                assert_eq!(arguments, serde_json::json!({"q": "订单"}));
                assert!(occurred_at.timestamp() > 0);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn tool_error_sets_error_field() {
        let ev = SessionEvent::tool_error("calc", "division by zero");
        match ev {
            SessionEvent::ToolResult { error, result, .. } => {
                assert_eq!(error.as_deref(), Some("division by zero"));
                assert_eq!(result, serde_json::Value::Null);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn custom_kind_preserved() {
        let ev = SessionEvent::custom("checkpoint", serde_json::json!({"n": 3}));
        match ev {
            SessionEvent::Custom { kind, data, .. } => {
                assert_eq!(kind, "checkpoint");
                assert_eq!(data, serde_json::json!({"n": 3}));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
