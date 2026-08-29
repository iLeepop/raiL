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
