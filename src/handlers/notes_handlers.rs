use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::ServiceError;
use crate::models::{CommonResponse, Note, NewNote, Product, User};
use crate::schema::{notes, products, users, barcodes, product_images};
use crate::errors::handler_disel_error;
use crate::utils::auth::get_sub;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{insert_into, delete};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateNoteParams {
    pub product_id: Uuid,
    pub body: Option<String>,
    pub rating: i16,
    pub public_scope: i16,
    pub image_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateNoteParams {
    pub body: Option<String>,
    pub rating: i16,
    pub public_scope: i16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub product_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteDetailResponse {
    pub note: Note,
    pub product: Product,
    pub user: Option<User>,
    pub images: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteListItem {
    pub note: Note,
    pub product: Product,
    pub user: Option<User>,
    pub images: Vec<Uuid>,
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
    let note = web::block(move || db_create_note(db, item, user_sub)).await??;
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
    let data = HashMap::from([("notes".to_string(), notes_list)]);
    let response = CommonResponse {
        result: true,
        data: data,
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
    let notes_list = web::block(move || db_get_notes_by_user(db, user_id.into_inner(), query.into_inner())).await??;
    let data = HashMap::from([("notes".to_string(), notes_list)]);
    let response = CommonResponse {
        result: true,
        data: data,
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
    let note = web::block(move || db_update_note(db, note_id.into_inner(), item, user_sub)).await??;
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
    let _delete_result = web::block(move || db_delete_note(db, note_id.into_inner(), user_sub)).await??;
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
) -> Result<Note, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // product_id로 barcode_id 찾기 (첫 번째 바코드 사용)
    let barcode = barcodes::table
        .filter(barcodes::product_id.eq(item.product_id))
        .first::<crate::models::Barcode>(conn)
        .map_err(|e| handler_disel_error(e))?;

    let new_note_id = Uuid::new_v4();
    let new_note = NewNote {
        id: new_note_id,
        user_id: user.id,
        barcode_id: barcode.id,
        body: item.body.clone(),
        registerd: Utc::now().naive_utc().date(),
        rating: item.rating,
        public_scope: item.public_scope,
    };

    let note = insert_into(notes::table)
        .values(&new_note)
        .get_result::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 이미지들을 노트에 연결
    for image_id in &item.image_ids {
        diesel::update(product_images::table.find(image_id))
            .set(product_images::note_id.eq(new_note_id))
            .execute(conn)
            .map_err(|e| handler_disel_error(e))?;
    }

    Ok(note)
}

fn db_get_note_by_id(
    pool: web::Data<Pool>,
    note_id: Uuid,
) -> Result<NoteDetailResponse, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 노트 조회
    let note = notes::table
        .find(note_id)
        .first::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 바코드로 제품 조회
    let barcode = barcodes::table
        .find(note.barcode_id)
        .first::<crate::models::Barcode>(conn)
        .map_err(|e| handler_disel_error(e))?;

    let product = products::table
        .find(barcode.product_id)
        .first::<Product>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 유저 조회 (public_scope에 따라 옵셔널)
    let user = if note.public_scope == 0 {
        None
    } else {
        users::table
            .find(note.user_id)
            .first::<User>(conn)
            .ok()
    };

    // 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::note_id.eq(note_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(NoteDetailResponse {
        note,
        product,
        user,
        images: image_ids,
    })
}

fn db_get_notes_list(
    pool: web::Data<Pool>,
    query: NoteListQuery,
) -> Result<Vec<NoteListItem>, ServiceError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 노트 리스트 조회
    let mut notes_query = notes::table.into_boxed();

    // product_id 필터링
    if let Some(product_id) = query.product_id {
        // product_id로 barcode_id들 찾기
        let barcode_ids: Vec<Uuid> = barcodes::table
            .filter(barcodes::product_id.eq(product_id))
            .select(barcodes::id)
            .load::<Uuid>(conn)
            .map_err(|e| handler_disel_error(e))?;

        notes_query = notes_query.filter(notes::barcode_id.eq_any(barcode_ids));
    }

    let notes_list: Vec<Note> = notes_query
        .order(notes::registerd.desc())
        .offset(offset)
        .limit(per)
        .load::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 각 노트에 대한 상세 정보 조회
    let mut result = Vec::new();

    for note in notes_list {
        // 바코드로 제품 조회
        let barcode = barcodes::table
            .find(note.barcode_id)
            .first::<crate::models::Barcode>(conn)
            .map_err(|e| handler_disel_error(e))?;

        let product = products::table
            .find(barcode.product_id)
            .first::<Product>(conn)
            .map_err(|e| handler_disel_error(e))?;

        // 유저 조회
        let user = if note.public_scope == 0 {
            None
        } else {
            users::table
                .find(note.user_id)
                .first::<User>(conn)
                .ok()
        };

        // 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(|e| handler_disel_error(e))?;

        result.push(NoteListItem {
            note,
            product,
            user,
            images: image_ids,
        });
    }

    Ok(result)
}

fn db_get_notes_by_user(
    pool: web::Data<Pool>,
    user_id: Uuid,
    query: NoteListQuery,
) -> Result<Vec<NoteListItem>, ServiceError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 특정 유저의 노트 리스트 조회
    let notes_list: Vec<Note> = notes::table
        .filter(notes::user_id.eq(user_id))
        .order(notes::registerd.desc())
        .offset(offset)
        .limit(per)
        .load::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 각 노트에 대한 상세 정보 조회
    let mut result = Vec::new();

    for note in notes_list {
        // 바코드로 제품 조회
        let barcode = barcodes::table
            .find(note.barcode_id)
            .first::<crate::models::Barcode>(conn)
            .map_err(|e| handler_disel_error(e))?;

        let product = products::table
            .find(barcode.product_id)
            .first::<Product>(conn)
            .map_err(|e| handler_disel_error(e))?;

        // 유저 조회
        let user = if note.public_scope == 0 {
            None
        } else {
            users::table
                .find(note.user_id)
                .first::<User>(conn)
                .ok()
        };

        // 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(|e| handler_disel_error(e))?;

        result.push(NoteListItem {
            note,
            product,
            user,
            images: image_ids,
        });
    }

    Ok(result)
}

fn db_update_note(
    pool: web::Data<Pool>,
    note_id: Uuid,
    item: web::Json<UpdateNoteParams>,
    user_sub: String,
) -> Result<Note, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 노트 소유자 확인
    let note = notes::table
        .find(note_id)
        .first::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    if note.user_id != user.id {
        return Err(ServiceError::BadRequest("Not authorized".to_string()));
    }

    // 노트 업데이트
    let updated_note = diesel::update(notes::table.find(note_id))
        .set((
            notes::body.eq(&item.body),
            notes::rating.eq(item.rating),
            notes::public_scope.eq(item.public_scope),
        ))
        .get_result::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(updated_note)
}

fn db_delete_note(
    pool: web::Data<Pool>,
    note_id: Uuid,
    user_sub: String,
) -> Result<bool, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 노트 소유자 확인
    let note = notes::table
        .find(note_id)
        .first::<Note>(conn)
        .map_err(|e| handler_disel_error(e))?;

    if note.user_id != user.id {
        return Err(ServiceError::BadRequest("Not authorized".to_string()));
    }

    // 노트 삭제
    let count = delete(notes::table.find(note_id))
        .execute(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(count == 1)
}
