use crate::Pool;
use lazy_static::lazy_static;
use std::sync::Mutex;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::models::{CommonResponse, Follow, NewFollow, NewUser, User, USER_COLUMNS};
use crate::schema::users::dsl::*;
use crate::schema::favorites;
use crate::schema::follows;
use crate::schema::notes;
use crate::utils::auth::{get_auth_info, AuthInfo};
use crate::utils::db::get_user_id_by_sub;
use crate::utils::r2::R2Client;
use crate::errors::handler_disel_error;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{count, delete, exists, insert_into, select};
use diesel::expression_methods::*;
use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use std::vec::Vec;
use uuid::Uuid;

lazy_static! {
    static ref REGISTER_MUTEX: Mutex<()> = Mutex::new(());
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddUserParams {
    pub nick_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchUserQuery {
    pub nick_name: Option<String>,
    // 클라이언트는 `index`로 페이지 번호를 보낸다. `page`/`index` 둘 다 허용.
    #[serde(alias = "index")]
    pub page: Option<i64>,
    pub per: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FollowParams {
    pub user_id: Uuid,
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
    pub follower_count: i64,
    pub needed_review_product: Option<bool>,
    pub is_following: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FcmTokenRequest {
    pub token: String,
    pub user_id: Uuid,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MyPageResponse {
    pub my_info: UserDetailResponse,
    pub product_ids: Vec<Uuid>,
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
pub async fn get_my_info(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req.clone())?;
    let user_info = web::block(move || get_user_info_by_sub(db, r2, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// Path: /users/mypage
pub async fn get_my_page(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req.clone())?;
    
    let db_for_block = db.clone();
    let r2_for_block = r2.clone();
    let result = web::block(move || {
        let info = get_user_info_by_sub(db_for_block.clone(), r2_for_block.clone(), auth_info.clone())?;
        let favs = get_user_favorites_by_sub(db_for_block, r2_for_block, auth_info)?;
        Ok::<_, CommonResponseError>(MyPageResponse {
            my_info: info,
            product_ids: favs,
        })
    }).await??;
    
    let response = CommonResponse {
        result: true,
        data: result,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: GET /api/users/search
pub async fn search_users(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<SearchUserQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let result = web::block(move || db_search_users(db, r2, query.into_inner(), auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: result,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// Path: /users/favorites
pub async fn get_my_favorites(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req.clone())?;
    let user_info = web::block(move || get_user_favorites_by_sub(db, r2, auth_info)).await??;
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

/// Path: GET /api/users/{id} - 인증된 유저 기준 is_following 포함
pub async fn get_user_by_id_with_auth(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    user_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let user_info = web::block(move || {
        db_get_user_info_by_id_with_auth(db, r2, user_id.into_inner(), auth_info)
    })
    .await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: GET /api/users/follower
pub async fn get_followers(
    req: HttpRequest,
    db: web::Data<Pool>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let result = web::block(move || db_get_followers(db, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: result,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: GET /api/users/following
pub async fn get_followings(
    req: HttpRequest,
    db: web::Data<Pool>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    // 팔로잉 목록은 내가 팔로우한 유저이므로 is_following = true 하드코딩
    let result = web::block(move || db_get_followings(db, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: result,
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
    r2: web::Data<R2Client>,
    item: web::Json<AddUserParams>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req.clone())?;
    let user_info = web::block(move || add_single_user(db, r2, item, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: user_info,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: POST /api/users/following
pub async fn follow_user(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<FollowParams>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let target_user_id_str = item.user_id.to_string();
    let db_for_fcm_base = db.clone();
    let db_res = web::block(move || db_follow_user(db, auth_info, item.into_inner())).await??;
    
    if let Some((my_nick, tokens)) = db_res {
        for token in tokens {
            let nick_copy = my_nick.clone();
            let target_uid_copy = target_user_id_str.clone();
            let db_for_fcm = db_for_fcm_base.clone();
            actix_rt::spawn(async move {
                crate::utils::fcm::send_fcm_push(
                    db_for_fcm,
                    &token,
                    "notification_new_follower",
                    vec![nick_copy],
                    &target_uid_copy,
                    "new_follower",
                ).await;
            });
        }
    }

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: POST /api/users/fcm_token
pub async fn set_fcm_token(
    db: web::Data<Pool>,
    item: web::Json<FcmTokenRequest>,
) -> Result<HttpResponse, Error> {
    let _ = web::block(move || db_set_fcm_token(db, item.into_inner())).await??;
    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
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
    let auth_info = get_auth_info(req)?;
    let user_info =
        web::block(move || update_single_user_nick(db, item, auth_info)).await??;
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
    let auth_info = get_auth_info(req)?;
    let delete_result = web::block(move || delete_single_user(db, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: delete_result,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: DELETE /api/users/following/:id
pub async fn unfollow_user(
    req: HttpRequest,
    db: web::Data<Pool>,
    unfollow_user_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let _ = web::block(move || db_unfollow_user(db, auth_info, unfollow_user_id.into_inner())).await??;
    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
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
            .filter(notes::rating.ne(0))
            .select(count(notes::id))
            .first(conn)
            .map_err(handler_disel_error)?;

        let follower_count: i64 = follows::table
            .filter(follows::following_user_id.eq(user.id))
            .count()
            .get_result(conn)
            .map_err(handler_disel_error)?;

        result.push(UserDetailResponse {
            user,
            note_count,
            follower_count,
            needed_review_product: None,
            is_following: None,
        });
    }

    Ok(result)
}

fn db_search_users(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: SearchUserQuery,
    auth_info: AuthInfo,
) -> Result<Vec<UserDetailResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let my_user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(uid) => uid,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let mut user_query = users
        .select(USER_COLUMNS)
        .filter(id.ne(my_user_id))
        .into_boxed();

    if let Some(nick) = query.nick_name {
        if !nick.trim().is_empty() {
            let search_pattern = format!("%{}%", nick);
            user_query = user_query.filter(crate::schema::users::nick_name.ilike(search_pattern));
        }
    }

    let items = user_query
        .order(crate::schema::users::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<User>(conn)
        .map_err(handler_disel_error)?;

    let mut result = Vec::new();

    for user in items {
        let note_count: i64 = notes::table
            .filter(notes::user_id.eq(user.id))
            .filter(notes::rating.ne(0))
            .select(count(notes::id))
            .first(conn)
            .map_err(handler_disel_error)?;

        let follower_count: i64 = follows::table
            .filter(follows::following_user_id.eq(user.id))
            .count()
            .get_result(conn)
            .map_err(handler_disel_error)?;

        // 내가 해당 유저를 팔로우하는지 확인 (exists 사용)
        let is_following: bool = select(exists(
            follows::table
                .filter(follows::user_id.eq(my_user_id))
                .filter(follows::following_user_id.eq(user.id)),
        ))
        .get_result(conn)
        .unwrap_or(false);

        result.push(UserDetailResponse {
            user,
            note_count,
            follower_count,
            needed_review_product: None,
            is_following: Some(is_following),
        });
    }

    Ok(result)
}

fn get_user_info_by_sub(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    auth_info: AuthInfo,
) -> Result<UserDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let user_sub = auth_info.sub.clone();
    let user_id = match get_user_id_by_sub(conn, &user_sub) {
        Ok(uid) => uid,
        Err(CommonResponseError::RecordNotFound) => {
            register_user(conn, None, auth_info, r2)?.id
        },
        Err(e) => return Err(e),
    };
    let user: User = users
        .select(USER_COLUMNS)
        .find(user_id)
        .first::<User>(conn)
        .map_err(handler_disel_error)?;

    let note_count: i64 = notes::table
        .filter(notes::user_id.eq(user.id))
        .filter(notes::rating.ne(0))
        .select(count(notes::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    let needed_review_product: bool = select(exists(
        notes::table
            .filter(notes::user_id.eq(user.id))
            .filter(notes::rating.eq(0)),
    ))
    .get_result(conn)
    .unwrap_or(false);

    let follower_count: i64 = follows::table
        .filter(follows::following_user_id.eq(user.id))
        .count()
        .get_result(conn)
        .map_err(handler_disel_error)?;

    Ok(UserDetailResponse {
        user,
        note_count,
        follower_count,
        needed_review_product: Some(needed_review_product),
        is_following: None,
    })
}

fn get_user_favorites_by_sub(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    auth_info: AuthInfo,
) -> Result<Vec<Uuid>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(uid) => uid,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info, r2)?.id,
        Err(e) => return Err(e),
    };

    let favorite_product_ids: Vec<Uuid> = favorites::table
        .filter(favorites::user_id.eq(user_id))
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
        .filter(notes::rating.ne(0))
        .select(count(notes::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    let follower_count: i64 = follows::table
        .filter(follows::following_user_id.eq(user.id))
        .count()
        .get_result(conn)
        .map_err(handler_disel_error)?;

    Ok(UserDetailResponse {
        user,
        note_count,
        follower_count,
        needed_review_product: None,
        is_following: None,
    })
}

/// sub로 내 user_id를 조회하여 is_following 포함한 특정 유저 정보 반환
fn db_get_user_info_by_id_with_auth(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    user_id: Uuid,
    auth_info: AuthInfo,
) -> Result<UserDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let my_user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(uid) => uid,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    let user = users
        .select(USER_COLUMNS)
        .find(user_id)
        .get_result::<User>(conn)
        .map_err(handler_disel_error)?;

    let note_count: i64 = notes::table
        .filter(notes::user_id.eq(user.id))
        .filter(notes::rating.ne(0))
        .select(count(notes::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    let follower_count: i64 = follows::table
        .filter(follows::following_user_id.eq(user.id))
        .count()
        .get_result(conn)
        .map_err(handler_disel_error)?;

    // 내가 해당 유저를 팔로우하는지 확인 (exists 사용)
    let is_following: bool = select(exists(
        follows::table
            .filter(follows::user_id.eq(my_user_id))
            .filter(follows::following_user_id.eq(user.id)),
    ))
    .get_result(conn)
    .unwrap_or(false);

    Ok(UserDetailResponse {
        user,
        note_count,
        follower_count,
        needed_review_product: None,
        is_following: Some(is_following),
    })
}

fn add_single_user(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    item: web::Json<AddUserParams>,
    auth_info: AuthInfo,
) -> Result<User, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    register_user(conn, item.nick_name.clone(), auth_info.clone(), r2)
}

pub(crate) fn register_user(
    conn: &mut PgConnection,
    provided_nick_name: Option<String>,
    auth_info: AuthInfo,
    r2: web::Data<R2Client>,
) -> Result<User, CommonResponseError> {
    let _lock = REGISTER_MUTEX.lock().unwrap();

    // 0. 이미 등록되었는지 다시 한 번 확인 (Data Race 방지용 Double-Check)
    if let Ok(uid) = get_user_id_by_sub(conn, &auth_info.sub) {
        return users
            .select(USER_COLUMNS)
            .find(uid)
            .first::<User>(conn)
            .map_err(handler_disel_error);
    }

    // 1. 유저 ID 미리 생성 (닉네임 생성기에 사용)
    let new_uuid = Uuid::new_v4();

    let user_sub = auth_info.sub.as_str();
    let _token = auth_info.token.as_deref();
    let nick: String = if let Some(n) = provided_nick_name.as_deref() {
        n.to_string()
    } else {
        let mut candidate = crate::utils::nickname::generate_nickname(&auth_info.locale, &new_uuid);
        while select(exists(users.filter(nick_name.eq(&candidate))))
            .get_result::<bool>(conn)
            .unwrap_or(true) 
        {
            candidate = crate::utils::nickname::generate_nickname(&auth_info.locale, &Uuid::new_v4());
        }
        candidate
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
  
    // 2. 프로필 이미지 업로드
    let rt = actix_rt::Runtime::new().unwrap();
    let is_uploaded = rt.block_on(upload_profile_from_auth0(r2, auth_info.clone(), new_uuid));

    // 3. 유저 추가
    let new_user = NewUser {
        id: new_uuid,
        nick_name: &nick,
        sub: user_sub,
        image_id: if is_uploaded { Some(new_uuid) } else { None },
        registered: Some(chrono::Utc::now()),
    };
    
    let res = insert_into(users)
        .values(&new_user)
        .returning(USER_COLUMNS)
        .get_result::<User>(conn)
        .map_err(handler_disel_error)?;

    // 신규 가입 성공 → 운영자에게 가입 알림 (10분 윈도우 동안 누적해 한 번에 발송)
    crate::utils::logger::notify_user_registered();

    Ok(res)
}

async fn upload_profile_from_auth0(r2: web::Data<R2Client>, auth_info: AuthInfo, user_id: Uuid) -> bool {
    let token = auth_info.token.as_deref();
    if let Some(tok) = token {
        if let Ok(authority) = std::env::var("AUTHORITY") {
            let url = format!("{}userinfo", authority);
            let client = reqwest::Client::new();
            if let Ok(res_api) = client.get(&url).bearer_auth(tok).send().await {
                if let Ok(json) = res_api.json::<serde_json::Value>().await {
                    if let Some(picture_url) = json.get("picture").and_then(|v| v.as_str()) {
                        if let Ok(pic_res) = client.get(picture_url).send().await {
                            if let Ok(bytes) = pic_res.bytes().await {
                                return r2.upload_image(&format!("images/profile/{}", user_id), bytes.to_vec(), "image/jpeg").await.is_ok();
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

fn update_single_user_nick(
    db: web::Data<Pool>,
    item: web::Json<SetUserParams>,
    auth_info: AuthInfo,
) -> Result<User, CommonResponseError> {
    let conn = &mut db.get().unwrap();

    if item.nick_name.is_none() && item.intro.is_none() {
        let user = users
            .select(USER_COLUMNS)
            .filter(sub.eq(&auth_info.sub))
            .first::<User>(conn)
            .map_err(handler_disel_error)?;
        return Ok(user);
    }

    let params = item.into_inner();
    let res = diesel::update(users.filter(sub.eq(&auth_info.sub)))
        .set(&params)
        .returning(USER_COLUMNS)
        .get_result::<User>(conn)
        .map_err(handler_disel_error)?;
    Ok(res)
}

fn delete_single_user(db: web::Data<Pool>, auth_info: AuthInfo) -> Result<bool, CommonResponseError> {
    let conn = &mut db.get().unwrap();
    let count = delete(users.filter(sub.eq(&auth_info.sub)))
        .execute(conn)
        .map_err(handler_disel_error)?;
    Ok(count == 1)
}

// ============================================
// MARK: Internal Methods for Follow
// ============================================

/// follows 테이블에 팔로우 row 삽입
fn db_follow_user(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
    params: FollowParams,
) -> Result<Option<(String, Vec<String>)>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let me: User = users
        .select(USER_COLUMNS)
        .filter(sub.eq(&auth_info.sub))
        .first::<User>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => CommonResponseError::RecordNotFound,
            _ => handler_disel_error(e),
        })?;

    let my_user_id = me.id;

    // 이미 팔로우 중인지 확인
    let already_follows: i64 = follows::table
        .filter(follows::user_id.eq(my_user_id))
        .filter(follows::following_user_id.eq(params.user_id))
        .count()
        .get_result(conn)
        .map_err(handler_disel_error)?;

    if already_follows > 0 {
        return Ok(None);
    }

    let new_follow = NewFollow {
        id: Uuid::new_v4(),
        user_id: my_user_id,
        following_user_id: params.user_id,
    };

    insert_into(follows::table)
        .values(&new_follow)
        .execute(conn)
        .map_err(handler_disel_error)?;

    use crate::schema::fcm_tokens;

    let target_tokens: Vec<String> = fcm_tokens::table
        .filter(fcm_tokens::user_id.eq(params.user_id))
        .filter(fcm_tokens::is_active.eq(1))
        .select(fcm_tokens::token)
        .load::<String>(conn)
        .unwrap_or_default();

    Ok(Some((me.nick_name, target_tokens)))
}

/// 나를 팔로우하는 유저 목록 (follows.following_user_id = 내 user_id)
fn db_get_followers(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
) -> Result<Vec<UserDetailResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let my_user_id = get_user_id_by_sub(conn, &auth_info.sub)?;

    // 나를 팔로잉하는 user_id 목록
    let follower_user_ids: Vec<Uuid> = follows::table
        .filter(follows::following_user_id.eq(my_user_id))
        .select(follows::user_id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // is_following: 내가 해당 팔로워를 맞팔하고 있는지 확인
    db_build_user_detail_items(conn, follower_user_ids, my_user_id, false)
}

/// 내가 팔로우하는 유저 목록 (follows.user_id = 내 user_id)
fn db_get_followings(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
) -> Result<Vec<UserDetailResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let my_user_id = get_user_id_by_sub(conn, &auth_info.sub)?;

    // 내가 팔로잉하는 user_id 목록
    let following_user_ids: Vec<Uuid> = follows::table
        .filter(follows::user_id.eq(my_user_id))
        .select(follows::following_user_id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 팔로잉 목록은 내가 팔로우한 유저이므로 is_following = true 하드코딩
    db_build_user_detail_items(conn, following_user_ids, my_user_id, true)
}

/// 팔로우 row 삭제 (follows.id = follow_id, follows.user_id = 내 user_id 확인 후 삭제)
fn db_unfollow_user(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
    unfollow_user_id: Uuid,
) -> Result<(), CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let my_user_id = get_user_id_by_sub(conn, &auth_info.sub)?;

    // 해당 row가 나의 팔로우 row인지 확인
    let follow: Follow = follows::table
        .filter(follows::user_id.eq(my_user_id))
        .filter(follows::following_user_id.eq(unfollow_user_id))
        .first::<Follow>(conn)
        .map_err(handler_disel_error)?;

    delete(&follow)
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(())
}

/// user_id 목록으로 UserDetailResponse 목록 조회 (follower_count 내림차순 정렬)
/// - my_user_id: 내 user_id (is_following 계산용)
/// - force_is_following: true이면 is_following을 true로 하드코딩 (팔로잉 목록용)
fn db_build_user_detail_items(
    conn: &mut diesel::PgConnection,
    user_ids: Vec<Uuid>,
    my_user_id: Uuid,
    force_is_following: bool,
) -> Result<Vec<UserDetailResponse>, CommonResponseError> {
    let mut items: Vec<UserDetailResponse> = Vec::new();

    for uid in user_ids {
        let user: User = users
            .select(USER_COLUMNS)
            .find(uid)
            .first::<User>(conn)
            .map_err(handler_disel_error)?;

        let note_count: i64 = notes::table
            .filter(notes::user_id.eq(uid))
            .filter(notes::rating.ne(0))
            .select(count(notes::id))
            .first(conn)
            .map_err(handler_disel_error)?;

        // 해당 유저를 팔로우하는 수 (follows.following_user_id = uid)
        let follower_count: i64 = follows::table
            .filter(follows::following_user_id.eq(uid))
            .count()
            .get_result(conn)
            .map_err(handler_disel_error)?;

        // force_is_following이 true이면 하드코딩, 아니면 exists로 DB 조회
        let is_following = if force_is_following {
            Some(true)
        } else {
            let exists_result: bool = select(exists(
                follows::table
                    .filter(follows::user_id.eq(my_user_id))
                    .filter(follows::following_user_id.eq(uid)),
            ))
            .get_result(conn)
            .unwrap_or(false);
            Some(exists_result)
        };

        items.push(UserDetailResponse {
            user,
            note_count,
            follower_count,
            needed_review_product: None,
            is_following,
        });
    }

    // follower_count 내림차순 정렬
    items.sort_by(|a, b| b.follower_count.cmp(&a.follower_count));

    Ok(items)
}

fn db_set_fcm_token(
    pool: web::Data<Pool>,
    params: FcmTokenRequest,
) -> Result<(), CommonResponseError> {
    use crate::schema::fcm_tokens;
    use diesel::upsert::excluded;
    let conn = &mut pool.get().unwrap();

    let new_fcm = crate::models::NewFcmToken {
        token: &params.token,
        user_id: params.user_id,
        is_active: params.is_active.map(|v| if v { 1 } else { 0 }).unwrap_or(1),
        updated_at: chrono::Utc::now(),
    };

    if params.is_active.is_some() {
        // Upsert (ON CONFLICT DO UPDATE) 
        insert_into(fcm_tokens::table)
            .values(&new_fcm)
            .on_conflict(fcm_tokens::token)
            .do_update()
            .set((
                fcm_tokens::user_id.eq(excluded(fcm_tokens::user_id)),
                fcm_tokens::is_active.eq(excluded(fcm_tokens::is_active)),
                fcm_tokens::updated_at.eq(excluded(fcm_tokens::updated_at)),
            ))
            .execute(conn)
            .map_err(handler_disel_error)?;
    } else {
        // Upsert without updating is_active if it is missing
        insert_into(fcm_tokens::table)
            .values(&new_fcm)
            .on_conflict(fcm_tokens::token)
            .do_update()
            .set((
                fcm_tokens::user_id.eq(excluded(fcm_tokens::user_id)),
                fcm_tokens::updated_at.eq(excluded(fcm_tokens::updated_at)),
            ))
            .execute(conn)
            .map_err(handler_disel_error)?;
    }

    Ok(())
}
