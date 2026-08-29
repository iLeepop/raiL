//! # session — 会话记录与检索库
//!
//! `SessionSpace` 是一个作用域化的运行时上下文:空间内发生的消息(`push`)
//! 与操作(工具调用、检查点、自定义事件)全部记录进 `Session`。
//! `Session` 通过 `SessionStore` 持久化,并按 `id` 精确、`title` 模糊检索。

pub mod error;
pub mod event;
pub mod session;
pub mod store;

pub use error::SessionError;
pub use event::SessionEvent;
pub use session::{Session, SessionStatus, SessionSummary};
pub use store::memory::InMemoryStore;
pub use store::{SessionQuery, SessionStore};
