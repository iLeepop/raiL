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
