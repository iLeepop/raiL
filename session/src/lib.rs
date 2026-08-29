//! # session — 会话记录与检索库
//!
//! `SessionSpace` 是一个作用域化的运行时上下文:空间内发生的消息(`push`)
//! 与操作(工具调用、检查点、自定义事件)全部记录进 `Session`。
//! `Session` 通过 `SessionStore` 持久化,并按 `id` 精确、`title` 模糊检索。
//!
//! # 使用示例
//!
//! ```no_run
//! use std::sync::Arc;
//! use llm::{Message, Role};
//! use session::store::file::FileStore;
//! use session::{SessionQuery, SessionSpace, SessionStore};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let store: Arc<FileStore> = Arc::new(FileStore::new("sessions/"));
//! let mut space = SessionSpace::new(store.clone(), "订单助手")
//!     .with_meta("model", serde_json::json!("Qwen2.5-72B"));
//!
//! space.push(Message::new(Role::User, "帮我查订单 #1024"))?;
//! space.record_tool("search_order", serde_json::json!({"id": "1024"}))?;
//! space.persist().await?;
//!
//! let found = store
//!     .list(&SessionQuery { title: Some("订单".into()), ..Default::default() })
//!     .await?;
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod event;
pub mod session;
pub mod space;
pub mod store;

pub use error::SessionError;
pub use event::SessionEvent;
pub use session::{Session, SessionStatus, SessionSummary};
pub use space::SessionSpace;
pub use store::file::FileStore;
pub use store::memory::InMemoryStore;
pub use store::{SessionQuery, SessionStore};
