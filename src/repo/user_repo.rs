//! DEPRECATED: 旧栈（Postgres/R2/旧 WS），仅 `legacy` 路径使用，勿新增依赖。
#![allow(dead_code)]

use async_trait::async_trait;

use crate::domain::ids::UserId;
use crate::domain::user::User;
use crate::error::AppResult;

pub type GroupId = String;

#[async_trait]
pub trait UserRepo: Send + Sync {
    async fn find_by_id(&self, user_id: &UserId) -> AppResult<Option<User>>;
    async fn find_by_qq_id(&self, qq_id: &str) -> AppResult<Option<User>>;
    async fn upsert(&self, user: &User) -> AppResult<User>;
}
