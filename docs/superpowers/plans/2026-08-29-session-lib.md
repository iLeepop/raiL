# Session Lib 实现计划

> **面向 AI 代理的工作者:** 必需子技能:使用 superpowers:subagent-driven-development(推荐)或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框(`- [ ]`)语法来跟踪进度。

**目标:** 在 raiL workspace 新增 `session` crate:会话空间(SessionSpace)记录空间内一切消息与操作,会话(Session)通过 id 精确检索、title 模糊检索,存储层可插拔(InMemory/File/SQLite 预留)。

**架构:** 混合记录模型 — `Session { messages: Vec<Message>, events: Vec<SessionEvent> }`,消息复用 `llm::Message`,操作(工具调用/检查点/自定义)进 events。`SessionStore` trait 抽象存储,`SessionSpace` 持有 session + `Arc<dyn SessionStore>`,显式 `persist()` 落盘,`close()` 后只读。

**技术栈:** Rust edition 2024(原生 async fn in trait)、tokio(fs)、serde/serde_json、uuid v7、chrono(serde)。无 async_trait。

**规格:** `docs/superpowers/specs/2026-08-29-session-lib-design.md`

---

## 文件结构

- 创建:`session/Cargo.toml` — 包清单
- 创建:`session/src/lib.rs` — 重导出 + 使用示例 rustdoc
- 创建:`session/src/error.rs` — `SessionError`
- 创建:`session/src/event.rs` — `SessionEvent` + 构造器
- 创建:`session/src/session.rs` — `Session` / `SessionStatus` / `SessionSummary`
- 创建:`session/src/space.rs` — `SessionSpace`
- 创建:`session/src/store/mod.rs` — `SessionStore` trait + `SessionQuery` + 共享过滤/排序/分页逻辑
- 创建:`session/src/store/memory.rs` — `InMemoryStore`
- 创建:`session/src/store/file.rs` — `FileStore`
- 创建:`session/tests/store_roundtrip.rs` — 集成测试
- 修改:`Cargo.toml` — workspace members 追加 `"session"`
- 修改:`llm/Cargo.toml` — 追加 `serde = { version = "1", features = ["derive"] }`
- 修改:`llm/src/structs/message.rs` — `Message` 补 serde derive
- 修改:`llm/src/enums/role.rs` — `Role` 补 serde derive
- 修改:`llm/src/traits/think.rs` — `ToolCall` 补 serde derive

依赖顺序:Task 1(llm serde)→ Task 2(scaffold + error)→ Task 3(event + session)→ Task 4(store trait + query)→ Task 5(InMemory)→ Task 6(File)→ Task 7(Space)→ Task 8(集成测试)→ Task 9(收尾)。

**重要:** 本仓库 `src/main.rs` 的测试需要真实模型 API(会发网络请求),**禁止**运行根 crate 的 `cargo test`。所有验证命令都带 `-p` 限定 crate。

---

### 任务 1:llm crate 补 serde derive

**文件:**
- 修改:`llm/Cargo.toml`
- 修改:`llm/src/structs/message.rs`
- 修改:`llm/src/enums/role.rs`
- 修改:`llm/src/traits/think.rs`
- 测试:`llm/src/structs/message.rs`(内嵌 test 模块)

- [ ] **步骤 1:编写失败的测试**

在 `llm/src/structs/message.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ToolCall;

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message::new(Role::User, "你好").with_image_url("data:image/png;base64,xx");
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "你好");
        assert!(matches!(back.role, Role::User));
        assert_eq!(back.image_url.as_deref(), Some("data:image/png;base64,xx"));
    }

    #[test]
    fn message_with_tool_calls_roundtrip() {
        let msg = Message::new(Role::Assistant, "").with_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            name: "calc".into(),
            arguments: "{\"x\":1}".into(),
        }]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        let tcs = back.tool_calls.expect("tool_calls 应存在");
        assert_eq!(tcs[0].name, "calc");
    }
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p llm`
预期:编译失败,报错 `the trait bound 'Message: Serialize' is not satisfied`(或 `Serialize is not implemented for Message`)。

- [ ] **步骤 3:实现 — 加依赖与 derive**

`llm/Cargo.toml` 的 `[dependencies]` 追加:

```toml
serde = { version = "1", features = ["derive"] }
```

`llm/src/enums/role.rs`,改为:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    User,
    System,
    Assistant,
    Tool,
}
```

并在文件顶部加 `use serde::{Deserialize, Serialize};`。

`llm/src/traits/think.rs`,`ToolCall` 的 derive 改为:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
```

并在文件顶部加 `use serde::{Deserialize, Serialize};`。`ThinkOutput` 不动。

`llm/src/structs/message.rs`,`Message` 的 derive 改为:

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
```

并在文件顶部(现有 use 块内)加 `use serde::{Deserialize, Serialize};`。

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p llm`
预期:全部 PASS,含 `message_serde_roundtrip` 与 `message_with_tool_calls_roundtrip`。

- [ ] **步骤 5:Commit**

```bash
git add llm/Cargo.toml llm/src/structs/message.rs llm/src/enums/role.rs llm/src/traits/think.rs
git commit -m "feat(llm): Message/Role/ToolCall 补 serde derive"
```

---

### 任务 2:session crate scaffold + SessionError

**文件:**
- 创建:`session/Cargo.toml`
- 创建:`session/src/lib.rs`
- 创建:`session/src/error.rs`
- 修改:`Cargo.toml`(workspace members)

- [ ] **步骤 1:创建 crate 骨架**

`session/Cargo.toml`:

```toml
[package]
name = "session"
version = "0.1.0"
edition = "2024"

[dependencies]
llm = { path = "../llm" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v7"] }
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["fs", "rt", "macros"] }

[dev-dependencies]
tempfile = "3"
```

根 `Cargo.toml` 的 `[workspace] members` 改为:

```toml
members = [
    "llm",
    "agent",
    "session",
]
```

`session/src/lib.rs`(先只声明 error 模块,其余模块后续任务补齐):

```rust
//! # session — 会话记录与检索库
//!
//! `SessionSpace` 是一个作用域化的运行时上下文:空间内发生的消息(`push`)
//! 与操作(工具调用、检查点、自定义事件)全部记录进 `Session`。
//! `Session` 通过 `SessionStore` 持久化,并按 `id` 精确、`title` 模糊检索。

pub mod error;

pub use error::SessionError;
```

- [ ] **步骤 2:运行 check 验证骨架可用**

运行:`cargo check -p session`
预期:成功,无警告。

- [ ] **步骤 3:实现 SessionError**

`session/src/error.rs`:

```rust
use std::error::Error;
use std::fmt;

use uuid::Uuid;

/// session 库的统一错误类型
#[derive(Debug)]
pub enum SessionError {
    /// 按 id 找不到会话(create 之外的写入/读取路径)
    NotFound(Uuid),
    /// 创建时 id 已存在
    AlreadyExists(Uuid),
    /// 对已关闭的会话执行写操作
    Closed(Uuid),
    /// 底层 IO 错误(FileStore)
    Io(std::io::Error),
    /// JSON 序列化/反序列化错误
    Serialize(serde_json::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotFound(id) => write!(f, "session {id} 不存在"),
            SessionError::AlreadyExists(id) => write!(f, "session {id} 已存在"),
            SessionError::Closed(id) => write!(f, "session {id} 已关闭,只读"),
            SessionError::Io(e) => write!(f, "IO 错误: {e}"),
            SessionError::Serialize(e) => write!(f, "序列化错误: {e}"),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SessionError::Io(e) => Some(e),
            SessionError::Serialize(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SessionError {
    fn from(e: std::io::Error) -> Self {
        SessionError::Io(e)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(e: serde_json::Error) -> Self {
        SessionError::Serialize(e)
    }
}
```

- [ ] **步骤 4:check 验证通过**

运行:`cargo check -p session`
预期:成功。

- [ ] **步骤 5:Commit**

```bash
git add Cargo.toml session/Cargo.toml session/src/lib.rs session/src/error.rs
git commit -m "feat(session): crate 骨架与 SessionError"
```

---

### 任务 3:Session 模型 — Session / SessionStatus / SessionSummary / SessionEvent

**文件:**
- 创建:`session/src/event.rs`
- 创建:`session/src/session.rs`
- 修改:`session/src/lib.rs`

- [ ] **步骤 1:编写失败的测试**

`session/src/event.rs` 末尾追加:

```rust
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
```

`session/src/session.rs` 末尾追加:

```rust
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
        s.events.push(SessionEvent::checkpoint("c", serde_json::json!(null)));
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
        s.events.push(SessionEvent::tool("calc", serde_json::json!({"x": 1})));
        s.metadata.insert("model".into(), serde_json::json!("m1"));
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "roundtrip");
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.metadata.get("model"), Some(&serde_json::json!("m1")));
    }
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session`
预期:编译失败,报 `unresolved import` / `cannot find module event` 或类似(模块尚未创建)。

- [ ] **步骤 3:实现 — 事件与会话模型**

`session/src/event.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会话内一次操作留痕。消息走 `Session::messages`,`SessionEvent` 记录非消息操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        Self::ToolCalled {
            tool: name.into(),
            arguments,
            occurred_at: Utc::now(),
        }
    }

    pub fn tool_result(name: impl Into<String>, result: Value) -> Self {
        Self::ToolResult {
            tool: name.into(),
            result,
            error: None,
            occurred_at: Utc::now(),
        }
    }

    pub fn tool_error(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self::ToolResult {
            tool: name.into(),
            result: Value::Null,
            error: Some(error.into()),
            occurred_at: Utc::now(),
        }
    }

    pub fn checkpoint(label: impl Into<String>, data: Value) -> Self {
        Self::Checkpoint {
            label: label.into(),
            data,
            occurred_at: Utc::now(),
        }
    }

    pub fn custom(kind: impl Into<String>, data: Value) -> Self {
        Self::Custom {
            kind: kind.into(),
            data,
            occurred_at: Utc::now(),
        }
    }
}
```

`session/src/session.rs`:

```rust
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
```

`session/src/lib.rs` 改为(此阶段不含使用示例,示例随任务 7 的 `SessionSpace` 一同加入,避免 doctest 引用未实现类型):

```rust
//! # session — 会话记录与检索库
//!
//! `SessionSpace` 是一个作用域化的运行时上下文:空间内发生的消息(`push`)
//! 与操作(工具调用、检查点、自定义事件)全部记录进 `Session`。
//! `Session` 通过 `SessionStore` 持久化,并按 `id` 精确、`title` 模糊检索。

pub mod error;
pub mod event;
pub mod session;

pub use error::SessionError;
pub use event::SessionEvent;
pub use session::{Session, SessionStatus, SessionSummary};
```

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:event 与 session 两个模块的 6 个测试全部 PASS。

- [ ] **步骤 5:Commit**

```bash
git add session/src/event.rs session/src/session.rs session/src/lib.rs
git commit -m "feat(session): Session 模型与 SessionEvent"
```

---

### 任务 4:SessionStore trait + SessionQuery + 共享过滤逻辑

**文件:**
- 创建:`session/src/store/mod.rs`
- 修改:`session/src/lib.rs`

- [ ] **步骤 1:编写失败的测试**

`session/src/store/mod.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    fn session_with_updated(title: &str, updated_at: DateTime<Utc>) -> Session {
        let mut s = Session::new(title);
        s.updated_at = updated_at;
        s
    }

    #[test]
    fn filter_and_page_sorts_desc_and_paginates() {
        let t = Utc::now();
        let sessions = vec![
            session_with_updated("订单-A", t + chrono::Duration::seconds(1)),
            session_with_updated("订单-B", t + chrono::Duration::seconds(2)),
            session_with_updated("发票", t + chrono::Duration::seconds(3)),
        ];
        let refs: Vec<&Session> = sessions.iter().collect();

        let q = SessionQuery {
            title: Some("订单".into()),
            limit: 1,
            offset: 0,
            ..Default::default()
        };
        let page1 = filter_and_page(refs.clone(), &q);
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0].title, "订单-B"); // updated_at 倒序

        let q2 = SessionQuery {
            title: Some("订单".into()),
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        let page2 = filter_and_page(refs.clone(), &q2);
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].title, "订单-A");
    }

    #[test]
    fn filter_matches_case_insensitive_title() {
        let t = Utc::now();
        let sessions = vec![session_with_updated("Order Assistant", t)];
        let refs: Vec<&Session> = sessions.iter().collect();
        let q = SessionQuery {
            title: Some("order".into()),
            ..Default::default()
        };
        assert_eq!(filter_and_page(refs, &q).len(), 1);
    }

    #[test]
    fn default_query_has_limit_50_and_clamps_500() {
        assert_eq!(SessionQuery::default().limit, 50);
        let t = Utc::now();
        let sessions: Vec<Session> = (0..600i64)
            .map(|i| session_with_updated(&format!("s-{i}"), t + chrono::Duration::seconds(i)))
            .collect();
        let refs: Vec<&Session> = sessions.iter().collect();
        let out = filter_and_page(refs, &SessionQuery::default());
        assert_eq!(out.len(), 50); // 未超上限取默认 50
        let huge = SessionQuery {
            limit: 9999,
            ..Default::default()
        };
        let out2 = filter_and_page(
            sessions.iter().collect::<Vec<&Session>>(),
            &huge,
        );
        assert_eq!(out2.len(), 500); // 超过上限被截到 500
    }
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session`
预期:编译失败,`unresolved import crate::store` / `cannot find module store`。

- [ ] **步骤 3:实现 — trait、查询、共享逻辑**

`session/src/store/mod.rs`:

```rust
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::SessionError;
use crate::session::{Session, SessionStatus, SessionSummary};

/// 检索条件。所有字段可选;`title` 为大小写不敏感的包含匹配。
/// 结果按 `updated_at` 倒序,再应用 `offset`/`limit` 分页。
#[derive(Debug, Clone)]
pub struct SessionQuery {
    pub title: Option<String>,
    pub status: Option<SessionStatus>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for SessionQuery {
    fn default() -> Self {
        Self {
            title: None,
            status: None,
            created_after: None,
            created_before: None,
            limit: 50,
            offset: 0,
        }
    }
}

/// 会话存储抽象:多后端可插拔(InMemory / File / 预留 SQLite)。
pub trait SessionStore: Send + Sync {
    /// 新建会话;id 已存在返回 `AlreadyExists`
    async fn create(&self, session: &Session) -> Result<(), SessionError>;
    /// 按 id 取会话;不存在返回 `None`
    async fn get(&self, id: Uuid) -> Result<Option<Session>, SessionError>;
    /// 覆写已存在会话;不存在返回 `NotFound`
    async fn save(&self, session: &Session) -> Result<(), SessionError>;
    /// 删除会话;不存在也是 Ok(幂等)
    async fn delete(&self, id: Uuid) -> Result<(), SessionError>;
    /// 按条件检索,返回轻量摘要列表
    async fn list(&self, query: &SessionQuery) -> Result<Vec<SessionSummary>, SessionError>;
}

/// 单个会话是否命中查询条件
pub(crate) fn matches(session: &Session, query: &SessionQuery) -> bool {
    if let Some(title) = &query.title {
        if !session.title.to_lowercase().contains(&title.to_lowercase()) {
            return false;
        }
    }
    if let Some(status) = query.status {
        if session.status != status {
            return false;
        }
    }
    if let Some(after) = query.created_after {
        if session.created_at <= after {
            return false;
        }
    }
    if let Some(before) = query.created_before {
        if session.created_at >= before {
            return false;
        }
    }
    true
}

/// 过滤 → 按 updated_at 倒序 → 分页。InMemory 与 File 后端复用。
pub(crate) fn filter_and_page(sessions: Vec<&Session>, query: &SessionQuery) -> Vec<SessionSummary> {
    let mut out: Vec<SessionSummary> = sessions
        .into_iter()
        .filter(|s| matches(s, query))
        .map(SessionSummary::from)
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let limit = query.limit.min(500);
    out.into_iter().skip(query.offset).take(limit).collect()
}
```

`session/src/lib.rs` 的模块声明追加:

```rust
pub mod store;
```

以及重导出追加:

```rust
pub use store::{SessionQuery, SessionStore};
```

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:store 模块 3 个测试 PASS(排序/分页、大小写不敏感、默认 limit 与 500 上限截断)。

- [ ] **步骤 5:Commit**

```bash
git add session/src/store/mod.rs session/src/lib.rs
git commit -m "feat(session): SessionStore trait 与 SessionQuery 检索"
```

---

### 任务 5:InMemoryStore

**文件:**
- 创建:`session/src/store/memory.rs`
- 修改:`session/src/store/mod.rs`
- 修改:`session/src/lib.rs`

- [ ] **步骤 1:编写失败的测试**

`session/src/store/memory.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[tokio::test]
    async fn create_save_get_delete_semantics() {
        let store = InMemoryStore::new();
        let s = Session::new("t");
        let id = s.id;

        store.create(&s).await.unwrap();
        // 重复 create → AlreadyExists
        let dup = store.create(&s).await.unwrap_err();
        assert!(matches!(dup, SessionError::AlreadyExists(_)));
        // get 命中
        assert!(store.get(id).await.unwrap().is_some());
        // 未存在 id → None
        assert!(store.get(Uuid::now_v7()).await.unwrap().is_none());
        // save 不存在 → NotFound
        let ghost = Session::new("ghost");
        let err = store.save(&ghost).await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
        // save 已存在 → 覆写
        let mut s2 = s.clone();
        s2.title = "改名".into();
        store.save(&s2).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().unwrap().title, "改名");
        // delete 幂等
        store.delete(id).await.unwrap();
        store.delete(id).await.unwrap();
        assert!(store.get(id).await.unwrap().is_none());
    }
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session`
预期:编译失败,`unresolved import crate::store::memory`。

- [ ] **步骤 3:实现 — 内存后端**

`session/src/store/memory.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use super::{SessionQuery, filter_and_page};
use crate::error::SessionError;
use crate::session::{Session, SessionSummary};
use crate::store::SessionStore;

/// 进程内存储。测试/开发用;多空间并发写同一 session 为 last-write-wins。
#[derive(Default)]
pub struct InMemoryStore {
    inner: Mutex<HashMap<Uuid, Session>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for InMemoryStore {
    async fn create(&self, session: &Session) -> Result<(), SessionError> {
        let mut inner = self.inner.lock().expect("InMemoryStore mutex poisoned");
        if inner.contains_key(&session.id) {
            return Err(SessionError::AlreadyExists(session.id));
        }
        inner.insert(session.id, session.clone());
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Session>, SessionError> {
        let inner = self.inner.lock().expect("InMemoryStore mutex poisoned");
        Ok(inner.get(&id).cloned())
    }

    async fn save(&self, session: &Session) -> Result<(), SessionError> {
        let mut inner = self.inner.lock().expect("InMemoryStore mutex poisoned");
        if !inner.contains_key(&session.id) {
            return Err(SessionError::NotFound(session.id));
        }
        inner.insert(session.id, session.clone());
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<(), SessionError> {
        let mut inner = self.inner.lock().expect("InMemoryStore mutex poisoned");
        inner.remove(&id);
        Ok(())
    }

    async fn list(&self, query: &SessionQuery) -> Result<Vec<SessionSummary>, SessionError> {
        let inner = self.inner.lock().expect("InMemoryStore mutex poisoned");
        let sessions: Vec<&Session> = inner.values().collect();
        Ok(filter_and_page(sessions, query))
    }
}
```

`session/src/store/mod.rs` 顶部加:

```rust
pub mod file;
pub mod memory;
```

`session/src/lib.rs` 重导出追加:

```rust
pub use store::memory::InMemoryStore;
```

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:memory 模块 1 个测试 PASS(create/save/get/delete 语义)。

- [ ] **步骤 5:Commit**

```bash
git add session/src/store/mod.rs session/src/store/memory.rs session/src/lib.rs
git commit -m "feat(session): InMemoryStore"
```

---

### 任务 6:FileStore

**文件:**
- 创建:`session/src/store/file.rs`
- 修改:`session/src/store/mod.rs`
- 修改:`session/src/lib.rs`

- [ ] **步骤 1:编写失败的测试**

`session/src/store/file.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use tempfile::TempDir;

    #[tokio::test]
    async fn persists_and_reloads_across_instances() {
        let dir = TempDir::new().unwrap();
        let id = {
            let store = FileStore::new(dir.path());
            let mut s = Session::new("跨实例");
            s.messages.push(llm::Message::new(llm::Role::User, "hi"));
            store.create(&s).await.unwrap();
            s.id
        };
        // 新实例(模拟进程重启)仍可读
        let store = FileStore::new(dir.path());
        let loaded = store.get(id).await.unwrap().expect("重启后应可恢复");
        assert_eq!(loaded.title, "跨实例");
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].text, "hi");
        // create 重复 → AlreadyExists(文件已存在)
        let dup = store.create(&loaded).await.unwrap_err();
        assert!(matches!(dup, SessionError::AlreadyExists(_)));
        // save 不存在 → NotFound(文件不存在)
        let ghost = Session::new("ghost");
        let err = store.save(&ghost).await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
        // delete 幂等
        store.delete(id).await.unwrap();
        store.delete(id).await.unwrap();
        assert!(store.get(id).await.unwrap().is_none());
        // 目录不存在时 list 返回空
        let empty_dir = TempDir::new().unwrap();
        let empty_store = FileStore::new(empty_dir.path().join("nope"));
        let list = empty_store.list(&SessionQuery::default()).await.unwrap();
        assert!(list.is_empty());
    }
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session`
预期:编译失败,`unresolved import crate::store::file`。

- [ ] **步骤 3:实现 — 文件后端**

`session/src/store/file.rs`:

```rust
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{SessionQuery, filter_and_page};
use crate::error::SessionError;
use crate::session::{Session, SessionSummary};
use crate::store::SessionStore;

/// 目录存储:每个会话一个 `{id}.json`,tmp+rename 原子写。
/// 查询为目录扫描,适合中小规模;大规模检索请迁移 SqliteStore(预留 feature)。
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

async fn atomic_write(path: &Path, session: &Session) -> Result<(), SessionError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_vec_pretty(session)?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, json).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

impl SessionStore for FileStore {
    async fn create(&self, session: &Session) -> Result<(), SessionError> {
        let path = self.path_for(session.id);
        if path.exists() {
            return Err(SessionError::AlreadyExists(session.id));
        }
        atomic_write(&path, session).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<Session>, SessionError> {
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path).await?;
        let session = serde_json::from_slice(&bytes)?;
        Ok(Some(session))
    }

    async fn save(&self, session: &Session) -> Result<(), SessionError> {
        let path = self.path_for(session.id);
        if !path.exists() {
            return Err(SessionError::NotFound(session.id));
        }
        atomic_write(&path, session).await
    }

    async fn delete(&self, id: Uuid) -> Result<(), SessionError> {
        let path = self.path_for(id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn list(&self, query: &SessionQuery) -> Result<Vec<SessionSummary>, SessionError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut sessions = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = tokio::fs::read(&path).await?;
            let session: Session = serde_json::from_slice(&bytes)?;
            sessions.push(session);
        }
        let refs: Vec<&Session> = sessions.iter().collect();
        Ok(filter_and_page(refs, query))
    }
}
```

`session/src/lib.rs` 重导出追加:

```rust
pub use store::file::FileStore;
```

(`store/mod.rs` 已在任务 5 声明 `pub mod file;`,无需再改。)

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:file 模块 1 个测试 PASS(跨实例恢复、AlreadyExists/NotFound/delete 幂等、空目录 list)。

- [ ] **步骤 5:Commit**

```bash
git add session/src/store/file.rs session/src/lib.rs
git commit -m "feat(session): FileStore 原子写持久化"
```

---

### 任务 7:SessionSpace

**文件:**
- 创建:`session/src/space.rs`
- 修改:`session/src/lib.rs`

- [ ] **步骤 1:编写失败的测试**

`session/src/space.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::memory::InMemoryStore;

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
}
```


- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session`
预期:编译失败,`unresolved import crate::space` 或 push 返回类型不符。

- [ ] **步骤 3:实现 — 会话空间**

`session/src/space.rs`:

```rust
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
pub struct SessionSpace {
    session: Session,
    store: Arc<dyn SessionStore>,
}

impl SessionSpace {
    /// 新建会话(不落盘;首次 `persist` 时创建)
    pub fn new(store: Arc<dyn SessionStore>, title: impl Into<String>) -> Self {
        Self {
            session: Session::new(title),
            store,
        }
    }

    /// 恢复既有会话(可从 store 读出后继续)
    pub fn resume(store: Arc<dyn SessionStore>, session: Session) -> Self {
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

    /// 显式落盘:会话在 store 中已存在则覆写,否则创建。刷新 `updated_at`。
    pub async fn persist(&mut self) -> Result<(), SessionError> {
        self.session.touch();
        match self.store.get(self.session.id).await? {
            Some(_) => self.store.save(&self.session).await?,
            None => self.store.create(&self.session).await?,
        }
        Ok(())
    }

    /// 关闭会话:置 `Closed` 并落盘,返回最终 `Session`。之后空间不可再写。
    pub async fn close(mut self) -> Result<Session, SessionError> {
        self.session.status = SessionStatus::Closed;
        self.persist().await?;
        Ok(self.session)
    }
}
```

`session/src/lib.rs` 改为(含使用示例 doctest):

```rust
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
//! let store: Arc<dyn SessionStore> = Arc::new(FileStore::new("sessions/"));
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
```

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:space 模块 3 个测试 PASS(首次 persist 创建/再次 persist 覆写并刷新 updated_at、Closed 拒绝写入、resume 保留状态)。

- [ ] **步骤 5:Commit**

```bash
git add session/src/space.rs session/src/lib.rs
git commit -m "feat(session): SessionSpace 会话空间"
```

---

### 任务 8:集成测试 — 跨后端 round-trip 与检索

**文件:**
- 创建:`session/tests/store_roundtrip.rs`

- [ ] **步骤 1:编写失败的测试**

`session/tests/store_roundtrip.rs`:

```rust
//! 跨后端一致性:同一套行为在 InMemory 与 File 上结果相同。
use std::sync::Arc;

use chrono::Duration;
use llm::{Message, Role};
use session::store::file::FileStore;
use session::store::memory::InMemoryStore;
use session::{Session, SessionQuery, SessionSpace, SessionStatus, SessionStore};
use tempfile::TempDir;
use uuid::Uuid;

/// 对任意后端执行同一套行为断言
async fn exercise_store<S: SessionStore>(store: Arc<S>) {
    // round-trip:写 → 持久化 → 读
    let mut space = SessionSpace::new(store.clone(), "订单助手")
        .with_meta("model", serde_json::json!("Qwen2.5-72B"));
    space.push(Message::new(Role::User, "帮我查订单 #1024")).unwrap();
    space.record_tool("search_order", serde_json::json!({"id": "1024"})).unwrap();
    space.record_tool_result("search_order", serde_json::json!({"status": "shipped"})).unwrap();
    space.checkpoint("fetched", serde_json::json!({"took_ms": 12})).unwrap();
    space.persist().await.unwrap();
    let id = space.id();

    let loaded = store.get(id).await.unwrap().expect("会话应已持久化");
    assert_eq!(loaded.title, "订单助手");
    assert!(matches!(loaded.messages[0].role, Role::User));
    assert_eq!(loaded.messages[0].text, "帮我查订单 #1024");
    assert_eq!(loaded.events.len(), 3);
    assert_eq!(loaded.metadata.get("model"), Some(&serde_json::json!("Qwen2.5-72B")));

    // title contains 检索 + 摘要计数
    let found = store
        .list(&SessionQuery { title: Some("订单".into()), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id);
    assert_eq!(found[0].message_count, 1);
    assert_eq!(found[0].event_count, 3);

    // 不存在的 id → None
    assert!(store.get(Uuid::now_v7()).await.unwrap().is_none());

    // delete 后不可再读
    store.delete(id).await.unwrap();
    assert!(store.get(id).await.unwrap().is_none());
}

#[tokio::test]
async fn memory_store_behaves() {
    exercise_store(Arc::new(InMemoryStore::new())).await;
}

#[tokio::test]
async fn file_store_behaves() {
    let dir = TempDir::new().unwrap();
    exercise_store(Arc::new(FileStore::new(dir.path()))).await;
}

#[tokio::test]
async fn file_store_survives_restart() {
    let dir = TempDir::new().unwrap();
    let id = {
        let store = Arc::new(FileStore::new(dir.path()));
        let mut space = SessionSpace::new(store.clone(), "重启恢复");
        space.push(Message::new(Role::User, "hi")).unwrap();
        space.persist().await.unwrap();
        space.id()
    };
    // 新实例模拟进程重启
    let store = Arc::new(FileStore::new(dir.path()));
    let list = store
        .list(&SessionQuery { title: Some("重启".into()), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
}

#[tokio::test]
async fn list_filters_status_and_paginates() {
    let store = Arc::new(InMemoryStore::new());
    let base = chrono::Utc::now();
    let mut sessions: Vec<Session> = Vec::new();
    for i in 0..4i64 {
        let mut s = Session::new(format!("批量-{i}"));
        s.updated_at = base + Duration::seconds(i);
        sessions.push(s);
    }
    sessions[3].status = SessionStatus::Closed;
    for s in &sessions {
        store.create(s).await.unwrap();
    }

    // 全部,updated_at 倒序 → 批量-3 最新
    let all = store.list(&SessionQuery::default()).await.unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].title, "批量-3");

    // status 过滤
    let active = store
        .list(&SessionQuery { status: Some(SessionStatus::Active), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(active.len(), 3);

    // title 过滤 + 分页
    let page = store
        .list(&SessionQuery { title: Some("批量".into()), limit: 2, offset: 1, ..Default::default() })
        .await
        .unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].title, "批量-2");
    assert_eq!(page[1].title, "批量-1");
}
```

- [ ] **步骤 2:运行测试验证失败**

运行:`cargo test -p session --test store_roundtrip`
预期:编译失败(`unresolved import` 或失败断言),因为此前只验证了单元测试。

- [ ] **步骤 3:实现 — 无需新代码**

集成测试直接使用已实现 API;若此处编译失败或断言失败,说明前面任务的实现与规格不符,修复对应模块。

- [ ] **步骤 4:运行测试验证通过**

运行:`cargo test -p session`
预期:全部单元测试 + 4 个集成测试 PASS(两种后端行为一致、File 重启恢复、status 过滤与分页)。

- [ ] **步骤 5:Commit**

```bash
git add session/tests/store_roundtrip.rs
git commit -m "test(session): 跨后端集成测试"
```

---

### 任务 9:收尾 — rustdoc 示例、格式化、全量验证

**文件:**
- 修改:`session/src/lib.rs`(若有 rustdoc 示例编译问题)

- [ ] **步骤 1:运行 rustdoc 示例(doctest)**

运行:`cargo test -p session --doc`
预期:lib.rs 使用示例编译并运行通过。若 `SessionQuery` 的 `Default` 导入或 `store.list(...).await?` 的 `?` 在 doctest 中报错,修正 lib.rs 示例(示例中 `main` 外的 `async fn demo` 需要 `Result` 返回,已按此书写)。

- [ ] **步骤 2:格式化**

运行:`cargo fmt -p session -p llm`
预期:无改动或仅格式微调;`git diff` 确认没有意外改动其他 crate。

- [ ] **步骤 3:全量编译验证**

运行:`cargo check --workspace`
预期:成功,无警告(workspace 全部 crate 编译;`--workspace` 只做 check,不触发 main.rs 的真实模型测试)。

- [ ] **步骤 4:全量测试(仅 session)**

运行:`cargo test -p session`
预期:全部 PASS。统计:event(3)+ session(3)+ store/mod(3)+ memory(1)+ file(1)+ space(3)+ 集成(4)= 18 个测试。

- [ ] **步骤 5:Commit(若有改动)**

```bash
git add session/src/lib.rs
git commit -m "docs(session): rustdoc 示例与收尾"
```

若无改动则跳过。

---

## 自检结果

- **规格覆盖度:** §2 范围(模型/空间/存储/错误/llm derive)→ 任务 1-7;§4.2 检索语义 → 任务 4 单元测试 + 任务 8 集成测试;§4.3 三后端 → 任务 5/6 + 二期 SqliteStore 不在本期;§5 不变量(Closed 只读、显式 persist)→ 任务 7;§9 测试策略 → 任务 8。规格 §5"closed 后 push/record* 返回 Closed"由 resume + Closed 会话的单元测试覆盖(close(self) 消费空间,写入路径只能经 resume 到达)。
- **占位符扫描:** 无 TODO/待定;每个代码步骤都有完整代码块。
- **类型一致性:** `SessionEvent::tool/tool_result/tool_error/checkpoint/custom` 构造器、`SessionStore` 五个方法签名、`SessionQuery` 六个字段、`SessionSpace` 的 `push`/`record*` 均返回 `Result<(), SessionError>`,跨任务引用一致。`SessionQuery::default().limit == 50` 与 `filter_and_page` 的 `min(500)` 由测试锁定。
