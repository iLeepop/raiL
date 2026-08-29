pub mod memory;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::SessionError;
use crate::session::{Session, SessionStatus, SessionSummary};

/// 检索条件。所有字段可选;`title` 为大小写不敏感的包含匹配。
/// `created_after`/`created_before` 为开区间:边界时刻本身被排除(严格大于/小于)。
/// `limit` 超过 500 会被截断到 500。结果按 `updated_at` 倒序(相同时按 `id` 升序),
/// 再应用 `offset`/`limit` 分页。
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
            title: None, status: None, created_after: None, created_before: None,
            limit: 50, offset: 0,
        }
    }
}

/// 会话存储抽象:多后端可插拔(InMemory / File / 预留 SQLite)。
// 决策(规格 D7):保持原生 async fn in trait,不引入 async_trait。
// 已实证:原生 async fn 与 RPITIT 在 stable 均不可 dyn 兼容(E0038),
// 因此本库不消费 dyn SessionStore,消费方(如 SessionSpace)使用泛型 Arc<S>。
#[allow(async_fn_in_trait)]
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
    out.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let limit = query.limit.min(500);
    out.into_iter().skip(query.offset).take(limit).collect()
}

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

        let q = SessionQuery { title: Some("订单".into()), limit: 1, offset: 0, ..Default::default() };
        let page1 = filter_and_page(refs.clone(), &q);
        assert_eq!(page1.len(), 1);
        assert_eq!(page1[0].title, "订单-B"); // updated_at 倒序

        let q2 = SessionQuery { title: Some("订单".into()), limit: 1, offset: 1, ..Default::default() };
        let page2 = filter_and_page(refs.clone(), &q2);
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].title, "订单-A");
    }

    #[test]
    fn filter_matches_case_insensitive_title() {
        let t = Utc::now();
        let sessions = vec![session_with_updated("Order Assistant", t)];
        let refs: Vec<&Session> = sessions.iter().collect();
        let q = SessionQuery { title: Some("order".into()), ..Default::default() };
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
        let huge = SessionQuery { limit: 9999, ..Default::default() };
        let out2 = filter_and_page(sessions.iter().collect::<Vec<&Session>>(), &huge);
        assert_eq!(out2.len(), 500); // 超过上限被截到 500
    }

    #[test]
    fn matches_excludes_open_interval_boundaries() {
        let t = Utc::now();
        let mut s = session_with_updated("边界", t);
        s.created_at = t; // created_at 精确等于边界时刻

        // created_after == created_at → 开区间排除
        let q_after = SessionQuery { created_after: Some(t), ..Default::default() };
        assert!(!matches(&s, &q_after));
        // created_before == created_at → 开区间排除
        let q_before = SessionQuery { created_before: Some(t), ..Default::default() };
        assert!(!matches(&s, &q_before));
        // 严格大于 after 才命中
        let q_after_ok = SessionQuery { created_after: Some(t - chrono::Duration::seconds(1)), ..Default::default() };
        assert!(matches(&s, &q_after_ok));
        // 严格小于 before 才命中
        let q_before_ok = SessionQuery { created_before: Some(t + chrono::Duration::seconds(1)), ..Default::default() };
        assert!(matches(&s, &q_before_ok));
    }
}
