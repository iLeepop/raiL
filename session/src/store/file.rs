use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

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
    let tmp = path.with_extension(format!("json.tmp.{}", Uuid::now_v7()));
    {
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(&json).await?;
        f.sync_all().await?;
    }
    tokio::fs::rename(&tmp, path).await?;
    // fsync 父目录使 rename 持久化(断电后不丢文件);部分平台不支持目录 fsync,失败忽略
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

impl SessionStore for FileStore {
    async fn create(&self, session: &Session) -> Result<(), SessionError> {
        let path = self.path_for(session.id);
        if tokio::fs::try_exists(&path).await? {
            return Err(SessionError::AlreadyExists(session.id));
        }
        atomic_write(&path, session).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<Session>, SessionError> {
        let path = self.path_for(id);
        if !tokio::fs::try_exists(&path).await? {
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path).await?;
        let session = serde_json::from_slice(&bytes)?;
        Ok(Some(session))
    }

    async fn save(&self, session: &Session) -> Result<(), SessionError> {
        let path = self.path_for(session.id);
        if !tokio::fs::try_exists(&path).await? {
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
            let session: Session = match serde_json::from_slice(&bytes) {
                Ok(s) => s,
                Err(_) => continue, // 损坏/人工编辑的会话文件跳过,不阻断整个列表;get 仍会响亮报错
            };
            sessions.push(session);
        }
        let refs: Vec<&Session> = sessions.iter().collect();
        Ok(filter_and_page(refs, query))
    }
}

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

    #[tokio::test]
    async fn list_ignores_non_json_and_tmp_residue() {
        let dir = TempDir::new().unwrap();
        let store = FileStore::new(dir.path());
        let s = Session::new("仅存此一个");
        store.create(&s).await.unwrap();
        // 目录里塞入无关文件与一次原子写失败的 .tmp 残留
        let other = Uuid::now_v7();
        tokio::fs::write(dir.path().join("readme.txt"), "hello")
            .await
            .unwrap();
        tokio::fs::write(
            dir.path().join(format!("{other}.json.tmp")),
            "{\"partial\":true}",
        )
        .await
        .unwrap();
        let list = store.list(&SessionQuery::default()).await.unwrap();
        assert_eq!(list.len(), 1, "非 .json 文件与 .tmp 残留不应出现在列表");
        assert_eq!(list[0].id, s.id);
    }

    #[tokio::test]
    async fn corrupt_file_fails_get_but_not_list() {
        let dir = TempDir::new().unwrap();
        let store = FileStore::new(dir.path());
        let bad = Uuid::now_v7();
        tokio::fs::write(dir.path().join(format!("{bad}.json")), "not json")
            .await
            .unwrap();
        // get 响亮报错
        let err = store.get(bad).await.unwrap_err();
        assert!(matches!(err, SessionError::Serialize(_)));
        // list 跳过损坏文件,不整体失败
        let list = store.list(&SessionQuery::default()).await.unwrap();
        assert!(list.is_empty());
    }
}
