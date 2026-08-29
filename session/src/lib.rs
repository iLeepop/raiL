//! # session — 会话记录与检索库
//!
//! `SessionSpace` 是一个作用域化的运行时上下文:空间内发生的消息(`push`)
//! 与操作(工具调用、检查点、自定义事件)全部记录进 `Session`。
//! `Session` 通过 `SessionStore` 持久化,并按 `id` 精确、`title` 模糊检索。

pub mod error;

pub use error::SessionError;
