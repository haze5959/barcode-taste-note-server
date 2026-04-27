use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{CommonResponse, NewFlavorTag, NewNote, Note, Product, ProductLite, User, NOTE_COLUMNS, NOTE_SIMPLE_COLUMNS, NoteSimple, NoteListQuery, NoteListResponse};
use crate::schema::{flavor_tags, notes, product_images, products, users};
use crate::utils::auth::{get_auth_info, AuthInfo};
use crate::utils::db::get_user_id_by_sub;
use crate::utils::r2::R2Client;
use crate::handlers::users_handler::register_user;
use actix_web::rt::spawn;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use chrono::{Utc, Datelike};
use diesel::Connection;
use diesel::dsl::{delete, insert_into};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::HashMap;
use chrono::TimeZone;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateNoteParams {
    pub product_id: Uuid,
    pub body: Option<String>,
    pub selected_flavors: Option<Vec<i16>>,
    pub rating: i16,
    pub public_scope: i16,
    pub image_ids: Vec<Uuid>,
    pub details: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateNoteParams {
    pub body: Option<String>,
    pub selected_flavors: Option<Vec<i16>>,
    pub rating: i16,
    pub public_scope: i16,
    pub image_ids: Vec<Uuid>,   
    pub details: Option<String>,
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NoteResponse {
    pub note: Note,
    pub product: Option<ProductLite>,
    pub user: Option<User>,
    pub image_ids: Option<Vec<Uuid>>,
    pub product_image_id: Option<Uuid>,
    pub flavors: Option<Vec<i16>>,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct NoteCalendarQuery {
    pub year: i32,
    pub month: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteRatingQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub rating: i16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiNoteListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub product_id: Option<Uuid>,
    pub order_by: Option<String>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /notes
pub async fn create_note(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    item: web::Json<CreateNoteParams>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let item_for_db = item.clone();
    let db_clone = db.clone();
    let r2_clone = r2.clone();
    let my_sub = auth_info.sub.clone();

    let note =
        web::block(move || db_create_note(db_clone, r2_clone, actix_web::web::Json(item_for_db), auth_info))
            .await??;

    // 비동기로 제품 정보 업데이트 (flavors, rating, note_count) 및 Push 발송
    let product_id = item.product_id;
    let rating = item.rating;
    let selected_flavors = item.selected_flavors.clone();
    let public_scope = item.public_scope;
    let db_clone = db.clone();

    spawn(async move {
        // 제품 업데이트
        let db_for_stats = db_clone.clone();
        let _ = web::block(move || {
            db_update_product_stats(db_for_stats, product_id, rating, selected_flavors)
        })
        .await;

        // 푸시 발송 (공개 노트인 경우)
        if public_scope != 0 {
            let db_for_push = db_clone.clone();
            let push_data = web::block(move || {
                let conn = &mut db_for_push.get().unwrap();
                use crate::schema::{users, follows, fcm_tokens};

                if let Ok(me) = users::table
                    .select(crate::models::USER_COLUMNS)
                    .filter(users::sub.eq(&my_sub))
                    .first::<User>(conn)
                {
                    // 나를 팔로우하는 유저 id 목록 조회
                    let follower_ids: Vec<Uuid> = follows::table
                        .filter(follows::following_user_id.eq(me.id))
                        .select(follows::user_id)
                        .load::<Uuid>(conn)
                        .unwrap_or_default();

                    if follower_ids.is_empty() {
                        return Ok::<_, CommonResponseError>(None);
                    }

                    // 팔로워들의 FCM 토큰 조회
                    let tokens: Vec<String> = fcm_tokens::table
                        .filter(fcm_tokens::user_id.eq_any(follower_ids))
                        .filter(fcm_tokens::is_active.eq(1))
                        .select(fcm_tokens::token)
                        .load::<String>(conn)
                        .unwrap_or_default();

                    if !tokens.is_empty() {
                        return Ok(Some((me.nick_name, me.id.to_string(), tokens)));
                    }
                }
                Ok(None)
            })
            .await;

            if let Ok(Ok(Some((nick_name, my_uid_str, tokens)))) = push_data {
                for token in tokens {
                    let nick = nick_name.clone();
                    let target_uid = my_uid_str.clone();
                    let db_for_fcm = db_clone.clone();
                    actix_rt::spawn(async move {
                        crate::utils::fcm::send_fcm_push(
                            db_for_fcm,
                            &token,
                            "notification_new_note",
                            vec![nick],
                            &target_uid,
                            "new_note",
                        ).await;
                    });
                }
            }
        }
    });

    let response = CommonResponse {
        result: true,
        data: note,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /notes/{id}
pub async fn get_note_by_id(
    db: web::Data<Pool>,
    note_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let note_detail = web::block(move || db_get_note_by_id(db, note_id.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: note_detail,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /notes
pub async fn get_notes_list(
    db: web::Data<Pool>,
    query: web::Query<NoteListQuery>,
) -> Result<HttpResponse, Error> {
    let notes_list = web::block(move || db_get_notes_list(db, query.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: notes_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /notes/user/{id}
pub async fn get_notes_by_user(
    db: web::Data<Pool>,
    user_id: web::Path<Uuid>,
    query: web::Query<NoteListQuery>,
) -> Result<HttpResponse, Error> {
    let notes_list =
        web::block(move || db_get_notes_by_user(db, user_id.into_inner(), query.into_inner()))
            .await??;
    let response = CommonResponse {
        result: true,
        data: notes_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /notes/calendar
pub async fn get_notes_calendar(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<NoteCalendarQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let calendar_data =
        web::block(move || db_get_notes_calendar(db, r2, auth_info, query.into_inner()))
            .await??;
    let response = CommonResponse {
        result: true,
        data: calendar_data,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /api/notes/rating (Authenticated)
pub async fn get_notes_by_rating(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<NoteRatingQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let notes_list =
        web::block(move || db_get_notes_by_rating(db, r2, auth_info, query.into_inner()))
            .await??;
    let response = CommonResponse {
        result: true,
        data: notes_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /api/notes (Authenticated)
pub async fn get_api_notes_list(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<ApiNoteListQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let notes_list =
        web::block(move || db_get_api_notes_list(db, r2, auth_info, query.into_inner()))
            .await??;
    let response = CommonResponse {
        result: true,
        data: notes_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for PUT
// ============================================

/// Path: /notes/{id}
pub async fn update_note(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    note_id: web::Path<Uuid>,
    item: web::Json<UpdateNoteParams>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let note =
        web::block(move || db_update_note(db, r2, note_id.into_inner(), item, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: note,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for DELETE
// ============================================

/// Path: /notes/{id}
pub async fn delete_note(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    note_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let _delete_result =
        web::block(move || db_delete_note(db, r2, note_id.into_inner(), auth_info)).await??;
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

fn db_create_note(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    item: web::Json<CreateNoteParams>,
    auth_info: AuthInfo,
) -> Result<Note, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    let parsed_details = item.details.as_deref().and_then(|d| serde_json::from_str(d).ok());

    let note = conn.transaction::<Note, CommonResponseError, _>(|conn| {
        // 기존 0점 노트 존재 여부 확인
        let existing_note_result = notes::table
            .filter(notes::user_id.eq(user_id))
            .filter(notes::product_id.eq(item.product_id))
            .filter(notes::rating.eq(0))
            .first::<Note>(conn);

        let note = match existing_note_result {
            Ok(ref existing) => {
                // 기존 노트 업데이트
                diesel::update(notes::table.find(existing.id))
                    .set((
                        notes::body.eq(&item.body),
                        notes::registered.eq(Utc::now()),
                        notes::rating.eq(item.rating),
                        notes::public_scope.eq(item.public_scope),
                        notes::details.eq(parsed_details.clone()),
                    ))
                    .get_result::<Note>(conn)?
            }
            Err(diesel::result::Error::NotFound) => {
                // 새 노트 생성
                let new_note_id = Uuid::new_v4();
                let new_note = NewNote {
                    id: new_note_id,
                    user_id,
                    product_id: item.product_id,
                    body: item.body.clone(),
                    registered: Utc::now(),
                    rating: item.rating,
                    public_scope: item.public_scope,
                    details: parsed_details.clone(),
                };

                insert_into(notes::table)
                    .values(&new_note)
                    .get_result::<Note>(conn)?
            }
            Err(e) => return Err(handler_disel_error(e)),
        };

        // 이미지들을 노트에 연결
        for image_id in &item.image_ids {
            diesel::update(product_images::table.find(image_id))
                .set((
                    product_images::user_id.eq(user_id),
                    product_images::note_id.eq(note.id),
                    product_images::product_id.eq(note.product_id),
                ))
                .execute(conn)?;
        }

        // Flavor tags 생성
        if let Some(flavors) = &item.selected_flavors {
            for flavor_val in flavors {
                let new_flavor = NewFlavorTag {
                    id: Uuid::new_v4(),
                    flavor: *flavor_val,
                    product_id: note.product_id,
                    note_id: note.id,
                };
                insert_into(flavor_tags::table)
                    .values(&new_flavor)
                    .execute(conn)?;
            }
        }

        Ok(note)
    })?;
    Ok(note)
}

fn db_get_note_by_id(
    pool: web::Data<Pool>,
    note_id: Uuid,
) -> Result<NoteResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 노트 조회
    let note = notes::table
        .select(NOTE_COLUMNS)
        .find(note_id)
        .first::<Note>(conn)
        .map_err(handler_disel_error)?;

    let product = products::table
        .find(note.product_id)
        .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
        .first::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    // 유저 조회 (public_scope에 따라 옵셔널)
    let user = users::table.select(crate::models::USER_COLUMNS)
        .find(note.user_id)
        .first::<User>(conn)
        .ok();

    // 이미지 ID들 조회
    let image_ids_vec: Vec<Uuid> = product_images::table
        .filter(product_images::note_id.eq(note_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    let mut product_image_id = None;
    let image_ids = if image_ids_vec.is_empty() {
        product_image_id = product_images::table
            .filter(product_images::note_id.is_null())
            .filter(product_images::product_id.eq(note.product_id))
            .order(product_images::registered.asc())
            .select(product_images::id)
            .first::<Uuid>(conn)
            .ok();
        None
    } else {
        Some(image_ids_vec)
    };

    // 해당 노트의 flavor_tags에서 flavor 값 리스트 조회
    let flavor_values: Vec<i16> = flavor_tags::table
        .filter(flavor_tags::note_id.eq(note_id))
        .select(flavor_tags::flavor)
        .load::<i16>(conn)
        .map_err(handler_disel_error)?;

    let flavors = if flavor_values.is_empty() {
        None
    } else {
        Some(flavor_values)
    };

    Ok(NoteResponse {
        note,
        product: Some(product),
        user,
        image_ids,
        product_image_id,
        flavors,
    })
}

fn db_get_notes_list(
    pool: web::Data<Pool>,
    query: NoteListQuery,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let mut notes_query = notes::table.into_boxed();

    match query.order_by.as_deref() {
        Some("rating") => {
            notes_query = notes_query.order(notes::rating.desc());
        }
        _ => {
            notes_query = notes_query.order(notes::registered.desc());
        }
    }

    // ids 파라미터가 있으면 페이징 무시하고 해당 ID들만 조회
    if let Some(ref ids_str) = query.ids {
        let parsed_ids: Vec<Uuid> = ids_str
            .split(',')
            .filter_map(|s| Uuid::parse_str(s.trim()).ok())
            .collect();
            
        if !parsed_ids.is_empty() {
            notes_query = notes_query.filter(notes::id.eq_any(parsed_ids));
            // 페이징 없이 전체 조회 (최대 100개 정도로 제한)
            let notes_list: Vec<NoteSimple> = notes_query
                .select(NOTE_SIMPLE_COLUMNS)
                .limit(100)
                .load::<NoteSimple>(conn)
                .map_err(handler_disel_error)?;
            return build_note_list_response(conn, notes_list);
        }
    }

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // product_id 필터링
    if let Some(product_id) = query.product_id {
        notes_query = notes_query.filter(notes::product_id.eq(product_id));
    }

    let notes_list: Vec<NoteSimple> = notes_query
        .select(NOTE_SIMPLE_COLUMNS)
        .filter(notes::rating.ne(0))
        .filter(notes::public_scope.ne(0))
        .offset(offset)
        .limit(per)
        .load::<NoteSimple>(conn)
        .map_err(handler_disel_error)?;

    build_note_list_response(conn, notes_list)
}

pub(crate) fn build_note_list_response(
    conn: &mut diesel::PgConnection,
    notes_list: Vec<NoteSimple>,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    // 각 노트에 대한 상세 정보 조회
    let mut result = Vec::new();

    for note in notes_list {
        let product = products::table
                .find(note.product_id)
                .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
                .first::<ProductLite>(conn)
                .ok();

        // 유저 조회
        let user = users::table.select(crate::models::USER_COLUMNS)
            .find(note.user_id)
            .first::<User>(conn)
            .ok();

        // 이미지 ID들 조회 (최대 3개)
        let image_ids_vec: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        let mut product_image_id = None;
        let image_ids = if image_ids_vec.is_empty() {
            product_image_id = product_images::table
                .filter(product_images::note_id.is_null())
                .filter(product_images::product_id.eq(note.product_id))
                .order(product_images::registered.asc())
                .select(product_images::id)
                .first::<Uuid>(conn)
                .ok();
            None
        } else {
            Some(image_ids_vec)
        };

        result.push(NoteListResponse {
            note,
            product,
            user,
            image_ids,
            product_image_id,
            flavors: None,
        });
    }

    Ok(result)
}

fn db_get_notes_by_user(
    pool: web::Data<Pool>,
    user_id: Uuid,
    query: NoteListQuery,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 정렬 방식 결정
    let mut notes_query = notes::table
        .select(NOTE_SIMPLE_COLUMNS)
        .filter(notes::user_id.eq(user_id))
        .filter(notes::rating.ne(0))
        .filter(notes::public_scope.ne(0))
        .into_boxed();

    match query.order_by.as_deref() {
        Some("rating") => {
            notes_query = notes_query.order(notes::rating.desc());
        }
        _ => {
            // default: "registered"
            notes_query = notes_query.order(notes::registered.desc());
        }
    }

    // 특정 유저의 노트 리스트 조회
    let notes_list: Vec<NoteSimple> = notes_query
        .offset(offset)
        .limit(per)
        .load::<NoteSimple>(conn)
        .map_err(handler_disel_error)?;

    // 각 노트에 대한 상세 정보 조회
    build_note_list_response(conn, notes_list)
}

fn db_update_note(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    note_id: Uuid,
    item: web::Json<UpdateNoteParams>,
    auth_info: AuthInfo,
) -> Result<Note, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2.clone())?.id,
        Err(e) => return Err(e),
    };

    // 노트 소유자 확인
    let note = notes::table
        .find(note_id)
        .first::<Note>(conn)
        .map_err(handler_disel_error)?;

    if note.user_id != user_id {
        return Err(CommonResponseError::AuthValidationFail);
    }

    // 현재 노트에 연결된 이미지 ID들 조회
    let current_image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::note_id.eq(note_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 제거할 이미지들 (현재 있지만 새 리스트에 없음)
    let images_to_remove: Vec<Uuid> = current_image_ids
        .iter()
        .filter(|id| !item.image_ids.contains(id))
        .cloned()
        .collect();

    // 추가할 이미지들 (새 리스트에 있지만 현재 없음)
    let images_to_add: Vec<Uuid> = item
        .image_ids
        .iter()
        .filter(|id| !current_image_ids.contains(id))
        .cloned()
        .collect();

    let parsed_details: Option<serde_json::Value> = item.details.as_deref().and_then(|d| serde_json::from_str(d).ok());

    let updated_note = conn.transaction::<Note, CommonResponseError, _>(|conn| {
        // 노트 업데이트
        let updated_note = diesel::update(notes::table.find(note_id))
            .set((
                notes::body.eq(&item.body),
                notes::rating.eq(item.rating),
                notes::public_scope.eq(item.public_scope),
                notes::details.eq(parsed_details.clone()),
            ))
            .get_result::<Note>(conn)?;

        // 제거: product_images의 row 제거
        for image_id in images_to_remove {
            // R2에서 이미지 이동 (Soft Delete)
            let rt = actix_rt::Runtime::new().unwrap();
            let _ = rt.block_on(r2.move_to_deleted(&format!("images/{}", image_id)));
            
            // DB에서 이미지 삭제
            delete(product_images::table.find(image_id)).execute(conn)?;
        }

        // 추가: note_id를 현재 노트로 설정
        for image_id in images_to_add {
            diesel::update(product_images::table.find(image_id))
                .set((
                    product_images::note_id.eq(note_id),
                    product_images::product_id.eq(note.product_id),
                ))
                .execute(conn)?;
        }

        // Flavor tags 업데이트
        if let Some(flavors) = &item.selected_flavors {
            // 현재 flavor tags 조회
            let current_flavors: Vec<i16> = flavor_tags::table
                .filter(flavor_tags::note_id.eq(note_id))
                .select(flavor_tags::flavor)
                .load::<i16>(conn)?;

            // 제거할 flavor tags
            let flavors_to_remove: Vec<i16> = current_flavors
                .iter()
                .filter(|f| !flavors.contains(f))
                .cloned()
                .collect();

            // 추가할 flavor tags
            let flavors_to_add: Vec<i16> = flavors
                .iter()
                .filter(|f| !current_flavors.contains(f))
                .cloned()
                .collect();

            // 제거 실행
            if !flavors_to_remove.is_empty() {
                delete(
                    flavor_tags::table
                        .filter(flavor_tags::note_id.eq(note_id))
                        .filter(flavor_tags::flavor.eq_any(flavors_to_remove)),
                )
                .execute(conn)?;
            }

            // 추가 실행
            for flavor_val in flavors_to_add {
                let new_flavor = NewFlavorTag {
                    id: Uuid::new_v4(),
                    flavor: flavor_val,
                    product_id: note.product_id,
                    note_id: note_id,
                };
                insert_into(flavor_tags::table)
                    .values(&new_flavor)
                    .execute(conn)?;
            }
        }

        Ok(updated_note)
    })?;

    Ok(updated_note)
}

fn db_delete_note(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    note_id: Uuid,
    auth_info: AuthInfo,
) -> Result<bool, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2.clone())?.id,
        Err(e) => return Err(e),
    };

    // 노트 소유자 확인
    let note = notes::table
        .find(note_id)
        .first::<Note>(conn)
        .map_err(handler_disel_error)?;

    if note.user_id != user_id {
        return Err(CommonResponseError::AuthValidationFail);
    }

    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::note_id.eq(note_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    for image_id in image_ids {
        // R2에서 이미지 이동 (Soft Delete)
        let rt = actix_rt::Runtime::new().unwrap();
        let _ = rt.block_on(r2.move_to_deleted(&format!("images/{}", image_id)));
    }

    // 노트 삭제
    let count = delete(notes::table.find(note_id))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(count == 1)
}

fn db_update_product_stats(
    pool: web::Data<Pool>,
    product_id: Uuid,
    _new_rating: i16,
    new_flavors: Option<Vec<i16>>,
) -> Result<(), CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 노트 카운트 조회 (rating이 0이 아닌 노트만)
    let note_count_from_db: i64 = notes::table
        .filter(notes::product_id.eq(product_id))
        .filter(notes::rating.ne(0))
        .filter(notes::public_scope.ne(0))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    // 해당 제품의 모든 노트 rating 합산 후 0이 아닌 노트 수로 나눠 정수 평균 계산
    let rating_sum: i64 = notes::table
        .filter(notes::product_id.eq(product_id))
        .filter(notes::rating.ne(0))
        .filter(notes::public_scope.ne(0))
        .select(diesel::dsl::sum(notes::rating))
        .first::<Option<i64>>(conn)
        .map_err(handler_disel_error)?
        .unwrap_or(0);
    let new_avg_rating: f32 = if note_count_from_db > 0 {
        (rating_sum / note_count_from_db) as f32
    } else {
        0.0
    };

    // Flavors 업데이트
    let product = products::table
        .select(crate::models::PRODUCT_COLUMNS)
        .find(product_id)
        .first::<Product>(conn)
        .map_err(handler_disel_error)?;

    let mut current_flavors_json = product.flavor_infos.unwrap_or(serde_json::json!({}));
    if let Some(flavors) = new_flavors {
        if let Some(obj) = current_flavors_json.as_object_mut() {
            for flavor_id in flavors {
                let key = flavor_id.to_string();
                let count = obj.get(&key).and_then(|v| v.as_i64()).unwrap_or(0);
                obj.insert(key, serde_json::Value::from(count + 1));
            }
        }
    }

    // DB 업데이트
    diesel::update(products::table.find(product_id))
        .set((
            products::rating.eq(new_avg_rating),
            products::note_count.eq(note_count_from_db as i32),
            products::flavor_infos.eq(current_flavors_json),
        ))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(())
}

fn db_get_notes_calendar(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    auth_info: AuthInfo,
    query: NoteCalendarQuery,
) -> Result<HashMap<String, Vec<Uuid>>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    // 해당 월의 시작일과 종료일 계산 (UTC 기준)
    let start_date = Utc.with_ymd_and_hms(query.year, query.month, 1, 0, 0, 0)
        .single()
        .ok_or(CommonResponseError::InvalidParameter)?;
    
    let next_month = if query.month == 12 { 1 } else { query.month + 1 };
    let next_month_year = if query.month == 12 { query.year + 1 } else { query.year };
    
    let end_date = Utc.with_ymd_and_hms(next_month_year, next_month, 1, 0, 0, 0)
        .single()
        .ok_or(CommonResponseError::InvalidParameter)?;

    // 해당 유저의 특정 기간 노트 조회
    let notes_in_month = notes::table
        .select((notes::id, notes::registered))
        .filter(notes::user_id.eq(user_id))
        .filter(notes::registered.ge(start_date))
        .filter(notes::registered.lt(end_date))
        .load::<(Uuid, chrono::DateTime<Utc>)>(conn)
        .map_err(handler_disel_error)?;

    let mut calendar_map: HashMap<String, Vec<Uuid>> = HashMap::new();

    for (note_id, registered) in notes_in_month {
        let day_str = registered.day().to_string();
        calendar_map.entry(day_str).or_default().push(note_id);
    }

    Ok(calendar_map)
}

fn db_get_api_notes_list(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    auth_info: AuthInfo,
    query: ApiNoteListQuery,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let mut notes_query = notes::table
        .select(NOTE_SIMPLE_COLUMNS)
        .filter(notes::user_id.eq(user_id))
        .into_boxed();

    if let Some(product_id) = query.product_id {
        notes_query = notes_query.filter(notes::product_id.eq(product_id));
    }

    match query.order_by.as_deref() {
        Some("rating") => {
            notes_query = notes_query.order(notes::rating.desc());
        }
        _ => {
            notes_query = notes_query.order(notes::registered.desc());
        }
    }

    let notes_list: Vec<NoteSimple> = notes_query
        .offset(offset)
        .limit(per)
        .load::<NoteSimple>(conn)
        .map_err(handler_disel_error)?;

    build_note_list_response(conn, notes_list)
}

fn db_get_notes_by_rating(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    auth_info: AuthInfo,
    query: NoteRatingQuery,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    let notes_list: Vec<NoteSimple> = notes::table
        .select(NOTE_SIMPLE_COLUMNS)
        .filter(notes::user_id.eq(user_id))
        .filter(notes::rating.eq(query.rating))
        .order(notes::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<NoteSimple>(conn)
        .map_err(handler_disel_error)?;

    build_note_list_response(conn, notes_list)
}
