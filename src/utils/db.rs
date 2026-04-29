use crate::errors::{handler_disel_error, CommonResponseError};
use diesel::prelude::*;
use uuid::Uuid;

/// sub 값으로 users 테이블에서 user_id(UUID)를 조회하는 공통 헬퍼.
/// 모든 핸들러에서 공유해 사용한다.
pub fn get_user_id_by_sub(conn: &mut PgConnection, user_sub: &str) -> Result<Uuid, CommonResponseError> {
    use crate::schema::users::dsl as users_dsl;
    users_dsl::users
        .select(users_dsl::id)
        .filter(users_dsl::sub.eq(user_sub))
        .first::<Uuid>(conn)
        .map_err(handler_disel_error)
}
