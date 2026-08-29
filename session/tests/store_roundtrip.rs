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
    space
        .push(Message::new(Role::User, "帮我查订单 #1024"))
        .unwrap();
    space
        .record_tool("search_order", serde_json::json!({"id": "1024"}))
        .unwrap();
    space
        .record_tool_result("search_order", serde_json::json!({"status": "shipped"}))
        .unwrap();
    space
        .checkpoint("fetched", serde_json::json!({"took_ms": 12}))
        .unwrap();
    space.persist().await.unwrap();
    let id = space.id();

    let loaded = store.get(id).await.unwrap().expect("会话应已持久化");
    assert_eq!(loaded.title, "订单助手");
    assert!(matches!(loaded.messages[0].role, Role::User));
    assert_eq!(loaded.messages[0].text, "帮我查订单 #1024");
    assert_eq!(loaded.events.len(), 3);
    assert_eq!(
        loaded.metadata.get("model"),
        Some(&serde_json::json!("Qwen2.5-72B"))
    );

    // title contains 检索 + 摘要计数
    let found = store
        .list(&SessionQuery {
            title: Some("订单".into()),
            ..Default::default()
        })
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
        .list(&SessionQuery {
            title: Some("重启".into()),
            ..Default::default()
        })
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
        .list(&SessionQuery {
            status: Some(SessionStatus::Active),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(active.len(), 3);

    // title 过滤 + 分页
    let page = store
        .list(&SessionQuery {
            title: Some("批量".into()),
            limit: 2,
            offset: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].title, "批量-2");
    assert_eq!(page[1].title, "批量-1");
}
