# Session Lib 设计规格

- 日期:2026-08-29
- 状态:待审查
- 范围:raiL workspace 新增 `session` crate

## 1. 背景与目标

当前 `agent::BaseAgent` 把对话历史放在内存 `history: Vec<Message>` 里:无会话概念、无持久化、无检索、无操作留痕。

目标:新增 `session` crate,提供:

1. **会话空间(SessionSpace)** — 一个作用域化的运行时上下文;空间内发生的操作(工具调用、检查点、自定义事件)与消息(对话)全部被记录。
2. **会话(Session)** — 可持久化的记录单元,通过 `id`(精确)与 `title`(模糊)查询检索。
3. **存储抽象(SessionStore)** — 多后端可插拔,默认文件持久化,内存后端供测试,SQLite 留 feature 扩展。

设计遵循主流做法:LangGraph checkpoint(消息+状态分存)、OpenAI Agents SDK 的多后端存储抽象、UUIDv7 时间有序 ID。

## 2. 范围

### 本期包含

- `session` crate 完整实现:数据模型、`SessionSpace`、`SessionStore` trait、`InMemoryStore`、`FileStore`、错误类型。
- `llm` crate 增量改动:`Message` / `Role` / `ToolCall` 补 `Serialize` / `Deserialize` derive(纯增量,不改行为)。
- 单元测试 + 三后端 round-trip 集成测试。

### 本期不包含(后续扩展)

- `SqliteStore`(feature `sqlite`,接口已预留)。
- `agent` crate 的自动集成适配(把 agent run 的消息自动灌入空间);本期通过使用示例演示手动接入。
- 全文检索 / 向量检索、多用户权限模型、session 合并/导出。

## 3. 核心概念与数据模型

```rust
pub type SessionId = Uuid; // UUIDv7,时间有序

pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub status: SessionStatus,             // Active | Closed
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,         // 每次 save 自动刷新
    pub metadata: BTreeMap<String, Value>, // 检索外挂信息:agent 名、模型、标签等
    pub messages: Vec<Message>,            // 复用 llm::Message,可直接喂 LLM
    pub events: Vec<SessionEvent>,         // 操作留痕
}

pub enum SessionStatus {
    Active,
    Closed,
}

pub enum SessionEvent {
    ToolCalled   { tool: String, arguments: Value, occurred_at: DateTime<Utc> },
    ToolResult   { tool: String, result: Value, error: Option<String>, occurred_at: DateTime<Utc> },
    Checkpoint   { label: String, data: Value, occurred_at: DateTime<Utc> },
    Custom       { kind: String, data: Value, occurred_at: DateTime<Utc> },
}
```

### 记录模型决策:混合模型(消息 + 事件分存)

对比过三条路线:

- **纯事件溯源**:session = 只追加事件日志,消息是事件的投影。审计/回放最强,但取 LLM 上下文需过滤投影,复杂度高。
- **混合模型(选定)**:消息单独存,直接对接 `agent::runtime::chat_history` 的 `Vec<Message>` 消费方式;工具调用等非消息操作进 `events`。对标 LangGraph checkpoint。
- **纯快照**:每次操作整体覆写,并发丢事件、无审计,弃。

## 4. 存储抽象与后端

### 4.1 SessionStore trait

edition 2024 原生 `async fn` in trait,不引入 `async_trait`。

```rust
pub trait SessionStore: Send + Sync {
    async fn create(&self, session: &Session) -> Result<(), SessionError>;
    async fn get(&self, id: SessionId) -> Result<Option<Session>, SessionError>;
    async fn save(&self, session: &Session) -> Result<(), SessionError>; // upsert
    async fn delete(&self, id: SessionId) -> Result<(), SessionError>;
    async fn list(&self, query: &SessionQuery) -> Result<Vec<SessionSummary>, SessionError>;
}
```

`create` 对已存在的 id 返回 `SessionError::AlreadyExists`;`save` 对不存在的 id 返回 `NotFound`(语义区分,便于调用方诊断)。

### 4.2 查询

```rust
pub struct SessionQuery {
    pub title: Option<String>,            // contains 模糊匹配,大小写不敏感
    pub status: Option<SessionStatus>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub limit: usize,                     // 默认 50,上限 500
    pub offset: usize,                    // 默认 0
}

pub struct SessionSummary {
    pub id: SessionId,
    pub title: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub event_count: usize,
}
```

`list` 返回轻量 `SessionSummary`,不拖全量消息;结果按 `updated_at` 倒序。`find_by_title` 不设独立方法,统一走 `list(SessionQuery { title: Some(..), .. })`。

### 4.3 后端

| 后端 | 实现 | 用途 |
|---|---|---|
| `InMemoryStore` | `Mutex<HashMap<SessionId, Session>>` | 测试、无持久化场景 |
| `FileStore` | 每 session 一个 `{id}.json`,tmp+rename 原子写 | 默认持久化,重启可恢复 |
| `SqliteStore`(feature `sqlite`,二期) | `sessions` / `session_messages` / `session_events` 三表,`title LIKE` | 检索规模升级 |

`FileStore` 的 `list` 采用目录扫描解析 summary;规模上限以文档注明,不提前做索引(满足 YAGNI)。`InMemoryStore` 与 `FileStore` 的查询语义一致(同一实现层复用 contains/过滤/分页逻辑)。

## 5. 会话空间 API

```rust
pub struct SessionSpace<S: SessionStore> { session: Session, store: Arc<S> }

impl<S: SessionStore> SessionSpace<S> {
    pub fn new(store: Arc<S>, title: impl Into<String>) -> Self;
    pub fn resume(store: Arc<S>, session: Session) -> Self;
    pub fn with_meta(mut self, key: impl Into<String>, value: Value) -> Self;

    // 只读访问
    pub fn id(&self) -> SessionId;
    pub fn title(&self) -> &str;
    pub fn messages(&self) -> &[Message];
    pub fn events(&self) -> &[SessionEvent];

    // 记录 —— 空间内一切操作都留痕
    pub fn push(&mut self, msg: Message);
    pub fn record_tool(&mut self, name: impl Into<String>, arguments: Value);
    pub fn record_tool_result(&mut self, name: impl Into<String>, result: Value);
    pub fn record_tool_error(&mut self, name: impl Into<String>, error: impl Into<String>);
    pub fn checkpoint(&mut self, label: impl Into<String>, data: Value);
    pub fn record(&mut self, event: SessionEvent);

    // 生命周期
    pub async fn persist(&mut self) -> Result<(), SessionError>;
    pub async fn close(self) -> Result<Session, SessionError>;
}
```

### 语义与不变量

- **显式持久化**:`persist()` 才写盘(对标 LangGraph checkpoint 语义),不逐条自动刷盘。
- **Closed 只读**:`close()` 置 `Closed` 并 persist;此后 `push` / `record*` / `checkpoint` 返回 `SessionError::Closed`。
- **持久化即所有权转移**:`close(self)` 消费空间,返回最终 `Session`;调用方拿到后可存入 `SessionSummary` 外的场景或做后续处理。
- **消息构造不重复造轮子**:`push` 接收现成 `llm::Message`(如 `Message::new(Role::User, text)`),由调用方组装。

### 使用示例(与 agent 手动集成)

```rust
let store: Arc<FileStore> = Arc::new(FileStore::new("sessions/")?);
let mut space = SessionSpace::new(store.clone(), "订单助手")
    .with_meta("model", json!("Qwen2.5-72B"));

space.push(Message::new(Role::User, "帮我查订单 #1024"));
let reply = agent.run("帮我查订单 #1024").await?;
space.push(Message::new(Role::Assistant, &reply));
space.persist().await?;

let found = store.list(&SessionQuery { title: Some("订单".into()), ..Default::default() }).await?;
```

## 6. 错误处理与并发

```rust
pub enum SessionError {
    NotFound(SessionId),
    AlreadyExists(SessionId),
    Closed(SessionId),
    Io(std::io::Error),
    Serialize(serde_json::Error),
    // 二期:Sqlite(rusqlite::Error) feature-gated
}
```

实现 `Display` + `std::error::Error` + `From<std::io::Error>` / `From<serde_json::Error>`。

并发模型:`store` 经 `Arc<S>` 共享(泛型;原生 async fn in trait 不可 dyn 兼容 — 已实证 E0038,故设计不用 `Arc<dyn SessionStore>`);`InMemoryStore` 内部 `Mutex`。多空间同时写同一 session 为 **last-write-wins**,文档注明;单空间持有会话是主用法,不提供乐观锁(YAGNI)。

## 7. 对 llm crate 的增量改动

`llm::Message`、`llm::Role`、`llm::ToolCall` 目前仅 `Clone`,补 `Serialize` / `Deserialize` derive。纯增量、不改行为,换取 session 直接复用领域类型,不复制 `SessionMessage`。注意:`Message` 的 `timestamp: Duration` 与 `meta_data: Option<HashMap<String, String>>` 均满足 serde 约束。

## 8. 文件布局与依赖

```
session/
  Cargo.toml          # deps: llm, serde(derive), serde_json, uuid(v7), chrono(serde)
  src/
    lib.rs            # 重导出
    session.rs        # Session / SessionStatus / SessionSummary
    event.rs          # SessionEvent
    space.rs          # SessionSpace
    error.rs          # SessionError
    store/
      mod.rs          # SessionStore trait + SessionQuery
      memory.rs       # InMemoryStore
      file.rs         # FileStore
  tests/
    store_roundtrip.rs
```

依赖选择:uuid v7(时间有序 ID)、chrono + serde(时间戳序列化;async_openai 已传递依赖 chrono,不新增第三方家族)。workspace members 追加 `session`。

## 9. 测试策略

- **三后端 round-trip**:create → push 消息 + 记录各类事件 → persist → `store.get` 重载 → 断言字段全等;FileStore 额外验证进程重建后可恢复。
- **检索语义**:title contains(大小写不敏感)、status 过滤、时间范围、分页(limit/offset)、`updated_at` 倒序。
- **不变量**:`close()` 后写入返回 `Closed`;`get` 不存在返回 `None`;`create` 重复 id 返回 `AlreadyExists`;`save` 不存在 id 返回 `NotFound`。
- **序列化**:`SessionEvent` 各变体 JSON round-trip。

## 10. 决策记录

| # | 决策 | 理由 |
|---|---|---|
| D1 | 混合记录模型:消息 + 事件分存 | 消息直接对接 agent 的 `Vec<Message>`;操作单独留痕,对标 LangGraph |
| D2 | `SessionStore` trait + 三后端(InMemory/File/SQLite feature) | 主流多后端抽象;默认 File 零依赖持久化 |
| D3 | UUIDv7 | 时间有序,分页/排序友好,LLM 应用主流 |
| D4 | title 检索 = contains;过滤/分页统一走 `SessionQuery`,不做全文索引 | 满足检索需求,保持 API 单一入口(YAGNI) |
| D5 | 显式 persist,不自动刷盘 | checkpoint 语义,避免每条记录一次 IO |
| D6 | 不依赖 agent crate | 避免循环依赖,SessionSpace 为通用层 |
| D7 | 原生 async fn in trait,不用 async_trait;消费方用泛型 `Arc<S>` 而非 `Arc<dyn SessionStore>`(已实证 async fn 与 RPITIT 均不可 dyn 兼容,E0038) | edition 2024 已稳定;不加依赖;泛型匹配代码库既有风格 |
