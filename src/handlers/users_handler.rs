use crate::Pool;
use crate::constants::DEFAULT_NICK;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::ServiceError;
use crate::models::{CommonResponse, NewUser, User};
use crate::schema::users::dsl::*;
use crate::utils::auth::{get_sub};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{delete, exists, insert_into, select};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::vec::Vec;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct AddUserParams {
    pub nick_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetUserNameParams {
    pub nick_name: String,
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /users
pub async fn get_users(db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_arr = web::block(move || get_all_users(db)).await??;
    let data = HashMap::from([("users".to_string(), user_arr)]);
    let response = CommonResponse {
        result: true,
        data: data,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

pub async fn get_my_info(req: HttpRequest, db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;

    let user_info = web::block(move || get_users_by_sub(db, user_sub)).await??;
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
    let user_info = web::block(move || db_get_user_by_id(db, user_id.into_inner())).await??;
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
    item: web::Json<SetUserNameParams>,
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
    Ok(
        web::block(move || delete_single_user(db, user_sub.as_str()))
            .await?
            .map(|result| HttpResponse::Ok().json(result))
            .map_err(|_| ServiceError::InternalServerError)?,
    )
}

// ============================================
// MARK: Internal Methods
// ============================================
fn get_all_users(pool: web::Data<Pool>) -> Result<Vec<User>, ServiceError> {
    let conn = &mut pool.get().unwrap();
    let items = users
        .load::<User>(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(items)
}

fn get_users_by_sub(pool: web::Data<Pool>, user_sub: String) -> Result<User, ServiceError> {
    let conn = &mut pool.get().unwrap();
    let items = users
        .filter(sub.eq(user_sub))
        .first::<User>(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(items)
}

fn db_get_user_by_id(pool: web::Data<Pool>, user_id: Uuid) -> Result<User, ServiceError> {
    let conn = &mut pool.get().unwrap();
    let items = users
        .find(user_id)
        .get_result::<User>(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(items)
}

fn add_single_user(
    db: web::Data<Pool>,
    item: web::Json<AddUserParams>,
    user_sub: &str,
) -> Result<User, ServiceError> {
    let conn = &mut db.get().unwrap();
    let nick: String = item.nick_name.as_deref().map_or_else(
        || {
            let user_count = users
                .count()
                .get_result::<i64>(conn)
                .map_err(|_| ServiceError::InternalDBError)?;
            Ok(format!("{DEFAULT_NICK}{user_count}"))
        },
        |n| Ok(n.to_string()),
    )?;

    // nick, sub 중복 체크
    let is_duplicate = select(exists(
        users
            .filter(nick_name.eq(nick.clone()))
            .or_filter(sub.eq(user_sub)),
    ))
    .get_result(conn)
    .map_err(|_| ServiceError::InternalDBError)?;

    if is_duplicate {
        return Err(ServiceError::DuplicatedError);
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
        .get_result(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(res)
}

fn update_single_user_nick(
    db: web::Data<Pool>,
    item: web::Json<SetUserNameParams>,
    user_sub: &str,
) -> Result<User, ServiceError> {
    let conn = &mut db.get().unwrap();
    let res = diesel::update(users.filter(sub.eq(user_sub)))
        .set(nick_name.eq(item.nick_name.as_str()))
        .get_result::<User>(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(res)
}

fn delete_single_user(db: web::Data<Pool>, user_sub: &str) -> Result<bool, ServiceError> {
    let conn = &mut db.get().unwrap();
    let count = delete(users.filter(sub.eq(user_sub)))
        .execute(conn)
        .map_err(|_| ServiceError::InternalDBError)?;
    Ok(count == 1)
}
