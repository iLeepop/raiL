//! # rai_l — RaiLLM 统一库入口
//!
//! 聚合 workspace 内的三个库,作为单一依赖提供给其它包:
//!
//! - [`llm`]:LLM 客户端封装(消息、多 Provider、思考)
//! - [`agent`]:智能体(ReAct / FunctionCall / 反思 / 计划执行)
//! - [`session`]:会话记录与检索
//!
//! 并附带开箱即用的终端交互会话 [`interactive`]。

pub mod interactive;

pub use agent;
pub use llm;
pub use session;
