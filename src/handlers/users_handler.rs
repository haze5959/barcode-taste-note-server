use crate::Pool;
use crate::constants::DEFAULT_NICK;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::models::{CommonResponse, NewUser, User, USER_COLUMNS};
use crate::schema::users::dsl::*;
use crate::schema::favorites;
use crate::schema::notes;
use crate::utils::auth::get_sub;
use crate::errors::handler_disel_error;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{count, delete, exists, insert_into, select};
use diesel::expression_methods::*;
use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use std::vec::Vec;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct AddUserParams {
    pub nick_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, diesel::AsChangeset)]
#[diesel(table_name = crate::schema::users)]
#[diesel(treat_none_as_null = false)]
pub struct SetUserParams {
    pub nick_name: Option<String>,
    pub intro: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserDetailResponse {
    pub user: User,
    pub note_count: i64,
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /users
pub async fn get_users(db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_arr = web::block(move || get_all_user_infos(db)).await??;
    let response = CommonResponse {
        result: true,
        data: user_arr,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// Path: /users/me
pub async fn get_my_info(req: HttpRequest, db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;

    let user_info = web::block(move || get_user_info_by_sub(db, user_sub)).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// Path: /users/favorites
pub async fn get_my_favorites(req: HttpRequest, db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;

    let user_info = web::block(move || get_user_favorites_by_sub(db, user_sub)).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /users/{id}
pub async fn get_user_by_id(
    db: web::Data<Pool>,
    user_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let user_info = web::block(move || db_get_user_info_by_id(db, user_id.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /users
pub async fn add_user(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<AddUserParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let user_info = web::block(move || add_single_user(db, item, user_sub.as_str())).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for PUT
// ============================================

/// Path: /users/me
pub async fn update_user_nick(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<SetUserParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let user_info =
        web::block(move || update_single_user_nick(db, item, user_sub.as_str())).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for DELETE
// ============================================

/// Path: /users/me
pub async fn delete_user(req: HttpRequest, db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let delete_result = web::block(move || delete_single_user(db, user_sub.as_str())).await??;
    let response = CommonResponse {
        result: true,
        data: delete_result,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Internal Methods
// ============================================
fn get_all_user_infos(pool: web::Data<Pool>) -> Result<Vec<UserDetailResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let items = users
        .select(USER_COLUMNS)
        .load::<User>(conn)
        .map_err(handler_disel_error)?;

    let mut result = Vec::new();

    for user in items {
        let note_count: i64 = notes::table
            .filter(notes::user_id.eq(user.id))
            .select(count(notes::id))
            .first(conn)
            .map_err(handler_disel_error)?;

        result.push(UserDetailResponse {
            user,
            note_count,
        });
    }

    Ok(result)
}

fn get_user_info_by_sub(pool: web::Data<Pool>, user_sub: String) -> Result<UserDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let user = match users
        .select(USER_COLUMNS)
        .filter(sub.eq(&user_sub))
        .first::<User>(conn)
    {
        Ok(user) => user,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?,
        Err(e) => return Err(handler_disel_error(e)),
    };

    let note_count: i64 = notes::table
        .filter(notes::user_id.eq(user.id))
        .select(count(notes::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    Ok(UserDetailResponse {
        user,
        note_count,
    })
}

fn get_user_favorites_by_sub(pool: web::Data<Pool>, user_sub: String) -> Result<Vec<Uuid>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let user = match users
        .select(USER_COLUMNS)
        .filter(sub.eq(&user_sub))
        .first::<User>(conn)
    {
        Ok(user) => user,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?,
        Err(e) => return Err(handler_disel_error(e)),
    };

    let favorite_product_ids: Vec<Uuid> = favorites::table
        .filter(favorites::user_id.eq(user.id))
        .select(favorites::product_id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;
    Ok(favorite_product_ids)
}

fn db_get_user_info_by_id(pool: web::Data<Pool>, user_id: Uuid) -> Result<UserDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let user = users
        .select(USER_COLUMNS)
        .find(user_id)
        .get_result::<User>(conn)
        .map_err(handler_disel_error)?;

    let note_count: i64 = notes::table
        .filter(notes::user_id.eq(user.id))
        .select(count(notes::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    Ok(UserDetailResponse {
        user,
        note_count,
    })
}

fn add_single_user(
    pool: web::Data<Pool>,
    item: web::Json<AddUserParams>,
    user_sub: &str,
) -> Result<User, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    register_user(conn, item.nick_name.clone(), user_sub)
}

pub(crate) fn register_user(
    conn: &mut PgConnection,
    provided_nick_name: Option<String>,
    user_sub: &str,
) -> Result<User, CommonResponseError> {
    let nick: String = if let Some(n) = provided_nick_name.as_deref() {
        n.to_string()
    } else {
        let user_count = users
            .select(id)
            .count()
            .get_result::<i64>(conn)
            .map_err(handler_disel_error)?;
        format!("{DEFAULT_NICK}{user_count}")
    };

    // nick, sub 중복 체크
    let is_duplicate = select(exists(
        users
            .select(id)
            .filter(nick_name.eq(nick.clone()))
            .or_filter(sub.eq(user_sub)),
    ))
    .get_result(conn)
    .map_err(handler_disel_error)?;

    if is_duplicate {
        return Err(CommonResponseError::DuplicatedError);
    }

    // 유저 추가
    let new_uuid = Uuid::new_v4();
    let new_user = NewUser {
        id: new_uuid,
        nick_name: &nick,
        sub: user_sub,
    };
    let res = insert_into(users)
        .values(&new_user)
        .returning(USER_COLUMNS)
        .get_result(conn)
        .map_err(handler_disel_error)?;
    Ok(res)
}

fn update_single_user_nick(
    db: web::Data<Pool>,
    item: web::Json<SetUserParams>,
    user_sub: &str,
) -> Result<User, CommonResponseError> {
    let conn = &mut db.get().unwrap();

    if item.nick_name.is_none() && item.intro.is_none() {
        let user = users
            .select(USER_COLUMNS)
            .filter(sub.eq(user_sub))
            .first::<User>(conn)
            .map_err(handler_disel_error)?;
        return Ok(user);
    }

    let params = item.into_inner();
    let res = diesel::update(users.filter(sub.eq(user_sub)))
        .set(&params)
        .returning(USER_COLUMNS)
        .get_result::<User>(conn)
        .map_err(handler_disel_error)?;
    Ok(res)
}

fn delete_single_user(db: web::Data<Pool>, user_sub: &str) -> Result<bool, CommonResponseError> {
    let conn = &mut db.get().unwrap();
    let count = delete(users.filter(sub.eq(user_sub)))
        .execute(conn)
        .map_err(handler_disel_error)?;
    Ok(count == 1)
}
