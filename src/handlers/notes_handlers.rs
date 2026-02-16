use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::handlers::users_handler::register_user;
use crate::models::{CommonResponse, NewFlavorTag, NewNote, Note, Product, ProductLite, User};
use crate::schema::{flavor_tags, notes, product_images, products, users};
use crate::utils::auth::get_sub;
use crate::utils::image_file::move_image_to_deleted;
use actix_web::rt::spawn;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use chrono::Utc;
use diesel::Connection;
use diesel::dsl::{delete, insert_into};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateNoteParams {
    pub product_id: Uuid,
    pub body: Option<String>,
    pub selected_flavors: Option<Vec<i16>>,
    pub rating: i16,
    pub public_scope: i16,
    pub image_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateNoteParams {
    pub body: Option<String>,
    pub selected_flavors: Option<Vec<i16>>,
    pub rating: i16,
    pub public_scope: i16,
    pub image_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub product_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteResponse {
    pub note: Note,
    pub product: Option<ProductLite>,
    pub user: Option<User>,
    pub image_ids: Vec<Uuid>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /notes
pub async fn create_note(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<CreateNoteParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let item_for_db = item.clone();
    let db_clone = db.clone();
    let note =
        web::block(move || db_create_note(db_clone, actix_web::web::Json(item_for_db), user_sub))
            .await??;

    // 비동기로 제품 정보 업데이트 (flavors, rating, note_count)
    let product_id = item.product_id;
    let rating = item.rating;
    let selected_flavors = item.selected_flavors.clone();
    let db_clone = db.clone();

    spawn(async move {
        let _ = web::block(move || {
            db_update_product_stats(db_clone, product_id, rating, selected_flavors)
        })
        .await;
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

// ============================================
// MARK: Handler for PUT
// ============================================

/// Path: /notes/{id}
pub async fn update_note(
    req: HttpRequest,
    db: web::Data<Pool>,
    note_id: web::Path<Uuid>,
    item: web::Json<UpdateNoteParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let note =
        web::block(move || db_update_note(db, note_id.into_inner(), item, user_sub)).await??;
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
    note_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let _delete_result =
        web::block(move || db_delete_note(db, note_id.into_inner(), user_sub)).await??;
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
    item: web::Json<CreateNoteParams>,
    user_sub: String,
) -> Result<Note, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?.id,
        Err(e) => return Err(handler_disel_error(e)),
    };

    let new_note_id = Uuid::new_v4();
    let new_note = NewNote {
        id: new_note_id,
        user_id,
        product_id: item.product_id,
        body: item.body.clone(),
        registered: Utc::now(),
        rating: item.rating,
        public_scope: item.public_scope,
    };

    let note = conn.transaction::<Note, CommonResponseError, _>(|conn| {
        let note = insert_into(notes::table)
            .values(&new_note)
            .get_result::<Note>(conn)?;

        // 이미지들을 노트에 연결
        for image_id in &item.image_ids {
            diesel::update(product_images::table.find(image_id))
                .set((
                    product_images::note_id.eq(new_note.id),
                    product_images::product_id.eq(new_note.product_id),
                ))
                .execute(conn)?;
        }

        // Flavor tags 생성
        if let Some(flavors) = &item.selected_flavors {
            for flavor_val in flavors {
                let new_flavor = NewFlavorTag {
                    id: Uuid::new_v4(),
                    flavor: *flavor_val,
                    product_id: new_note.product_id,
                    note_id: new_note.id,
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
        .find(note_id)
        .first::<Note>(conn)
        .map_err(handler_disel_error)?;

    let product = products::table
        .find(note.product_id)
        .select((products::id, products::name, products::type_, products::rating, products::registered))
        .first::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    // 유저 조회 (public_scope에 따라 옵셔널)
    let user = users::table.select((users::id, users::nick_name, users::intro, users::image_id))
        .find(note.user_id)
        .first::<User>(conn)
        .ok();

    // 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::note_id.eq(note_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    Ok(NoteResponse {
        note,
        product: Some(product),
        user,
        image_ids: image_ids,
    })
}

fn db_get_notes_list(
    pool: web::Data<Pool>,
    query: NoteListQuery,
) -> Result<Vec<NoteResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 노트 리스트 조회
    let mut notes_query = notes::table.into_boxed();

    // product_id 필터링
    if let Some(product_id) = query.product_id {
        notes_query = notes_query.filter(notes::product_id.eq(product_id));
    }

    let notes_list: Vec<Note> = notes_query
        .order(notes::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<Note>(conn)
        .map_err(handler_disel_error)?;

    // 각 노트에 대한 상세 정보 조회
    let mut result = Vec::new();

    for note in notes_list {
        if note.public_scope == 0 {
            continue;
        }
        let product = products::table
                .find(note.product_id)
                .select((products::id, products::name, products::type_, products::rating, products::registered))
                .first::<ProductLite>(conn)
                .ok();

        // 유저 조회
        let user = users::table.select((users::id, users::nick_name, users::intro, users::image_id))
            .find(note.user_id)
            .first::<User>(conn)
            .ok();

        // 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        result.push(NoteResponse {
            note,
            product,
            user,
            image_ids: image_ids,
        });
    }

    Ok(result)
}

fn db_get_notes_by_user(
    pool: web::Data<Pool>,
    user_id: Uuid,
    query: NoteListQuery,
) -> Result<Vec<NoteResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 특정 유저의 노트 리스트 조회
    let notes_list: Vec<Note> = notes::table
        .filter(notes::user_id.eq(user_id))
        .order(notes::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<Note>(conn)
        .map_err(handler_disel_error)?;

    // 각 노트에 대한 상세 정보 조회
    let mut result = Vec::new();

    for note in notes_list {
        let product = products::table
            .find(note.product_id)
            .select((products::id, products::name, products::type_, products::rating, products::registered))
            .first::<ProductLite>(conn)
            .ok();

        // 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        result.push(NoteResponse {
            note,
            product,
            user: None,
            image_ids: image_ids,
        });
    }

    Ok(result)
}

fn db_update_note(
    pool: web::Data<Pool>,
    note_id: Uuid,
    item: web::Json<UpdateNoteParams>,
    user_sub: String,
) -> Result<Note, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?.id,
        Err(e) => return Err(handler_disel_error(e)),
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

    let updated_note = conn.transaction::<Note, CommonResponseError, _>(|conn| {
        // 노트 업데이트
        let updated_note = diesel::update(notes::table.find(note_id))
            .set((
                notes::body.eq(&item.body),
                notes::rating.eq(item.rating),
                notes::public_scope.eq(item.public_scope),
            ))
            .get_result::<Note>(conn)?;

        // 제거: product_images의 row 제거
        for image_id in images_to_remove {
            // 이미지 파일을 deleted 폴더로 이동
            let _file_delete_result = move_image_to_deleted(image_id);
            // DB에서 이미지 삭제
            delete(product_images::table.find(image_id)).execute(conn)?;
        }

        // 추가: note_id를 현재 노트로 설정
        for image_id in images_to_add {
            diesel::update(product_images::table.find(image_id))
                .set(product_images::note_id.eq(note_id))
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
    note_id: Uuid,
    user_sub: String,
) -> Result<bool, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match users::table
        .select(users::id)
        .filter(users::sub.eq(&user_sub))
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?.id,
        Err(e) => return Err(handler_disel_error(e)),
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
        // 이미지 파일을 deleted 폴더로 이동
        let _file_delete_result = move_image_to_deleted(image_id);
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
    new_rating: i16,
    new_flavors: Option<Vec<i16>>,
) -> Result<(), CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 1. 제품 정보 조회
    let product = products::table
        .find(product_id)
        .first::<Product>(conn)
        .map_err(handler_disel_error)?;

    // 2. 노트 카운트 조회 (DB에서 현재 제품에 대한 노트 수)
    let note_count_from_db = notes::table
        .filter(notes::product_id.eq(product_id))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    // 3. Rating 업데이트
    // 공식: (count * 기존 rating + new_rating) / (count + 1)
    let old_rating = product.rating.unwrap_or(0.0);
    let old_count = note_count_from_db - 1;
    let new_avg_rating =
        ((old_count as f32 * old_rating) + new_rating as f32) / note_count_from_db as f32;

    // 4. Flavors 업데이트
    let mut current_flavors_json = product.flavors.unwrap_or(serde_json::json!({}));
    if let Some(flavors) = new_flavors {
        if let Some(obj) = current_flavors_json.as_object_mut() {
            for flavor_id in flavors {
                let key = flavor_id.to_string();
                let count = obj.get(&key).and_then(|v| v.as_i64()).unwrap_or(0);
                obj.insert(key, serde_json::Value::from(count + 1));
            }
        }
    }

    // 5. DB 업데이트
    diesel::update(products::table.find(product_id))
        .set((
            products::rating.eq(new_avg_rating),
            products::note_count.eq(note_count_from_db as i32),
            products::flavors.eq(current_flavors_json),
        ))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(())
}
