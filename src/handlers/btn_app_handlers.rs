use crate::Pool;
use crate::constants::HOME_INFO_LENGTH;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::handlers::users_handler::register_user;
use crate::models::{CommonResponse, User, ProductLite, Note, Report, NewReport};
use crate::schema::{product_images, reports, users, products, notes};
use crate::handlers::products_handlers::ProductListItem;
use crate::handlers::notes_handlers::NoteResponse;
use crate::utils::auth::get_sub;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use chrono::Utc;
use lazy_static::lazy_static;
use std::sync::RwLock;
use std::time::{Instant, Duration};
use diesel::dsl::insert_into;
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HomeResponse {
    pub recent_notes: Vec<NoteResponse>,
    pub recent_products: Vec<ProductListItem>,
    pub product_count: i64,
}

lazy_static! {
    static ref HOME_CACHE: RwLock<Option<(HomeResponse, Instant)>> = RwLock::new(None);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateReportParams {
    pub product_id: Option<Uuid>,
    pub body: Option<String>,
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /btn/home
pub async fn get_home_info(db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    // 1. Check cache first (Read Lock)
    {
        if let Ok(cache) = HOME_CACHE.read() {
            if let Some((cached_data, timestamp)) = &*cache {
                // If cache is less than 10 minutes (600 seconds) old, return it
                if timestamp.elapsed() < Duration::from_secs(600) {
                    let response = CommonResponse {
                        result: true,
                        data: cached_data.clone(),
                        error: None,
                    };
                    return Ok(HttpResponse::Ok().json(response));
                }
            }
        }
    }

    // 2. Cache miss or expired, fetch from DB
    let db1 = db.clone();
    let db2 = db.clone();
    let db3 = db.clone();

    let (notes_list, products_list, product_count) = futures::try_join!(
        web::block(move || db_get_notes_list(db1)),
        web::block(move || db_get_products_list(db2)),
        web::block(move || db_get_product_count(db3)),
    )?;

    let notes_list = notes_list?;
    let products_list = products_list?;
    let product_count = product_count?;
    
    let data = HomeResponse {
        recent_notes: notes_list,
        recent_products: products_list,
        product_count,
    };

    // 3. Update cache (Write Lock)
    {
        if let Ok(mut cache) = HOME_CACHE.write() {
            *cache = Some((data.clone(), Instant::now()));
        }
    }

    let response = CommonResponse {
        result: true,
        data: data,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /api/btn/report (GET)
pub async fn get_my_reports(
    req: HttpRequest,
    db: web::Data<Pool>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let reports = web::block(move || db_get_my_reports(db, user_sub)).await??;
    let response = CommonResponse {
        result: true,
        data: reports,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /api/btn/report (POST)
pub async fn create_report(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<CreateReportParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let report = web::block(move || db_create_report(db, item, user_sub)).await??;
    let response = CommonResponse {
        result: true,
        data: report,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Internal Methods
// ============================================

fn db_get_products_list(pool: web::Data<Pool>) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 제품 리스트 조회
    let products_list: Vec<ProductLite> = products::table
        .select((products::id, products::name, products::type_, products::rating, products::registered))
        .order(products::registered.desc())
        .limit(HOME_INFO_LENGTH)
        .load::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    // 각 제품에 대한 이미지 ID들 조회 (최대 3개)
    let mut result = Vec::new();

    for product in products_list {
        // 제품 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::product_id.eq(product.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        result.push(ProductListItem {
            product,
            image_ids: image_ids,
        });
    }

    Ok(result)
}

fn db_get_notes_list(pool: web::Data<Pool>) -> Result<Vec<NoteResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 노트 리스트 조회
    let notes_list: Vec<Note> = notes::table
        .order(notes::registered.desc())
        // .filter(notes::public_scope.eq(1))
        .limit(HOME_INFO_LENGTH)
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

        // 유저 조회
        let user = users::table.select((users::id, users::nick_name, users::intro, users::image_id))
            .find(note.user_id)
            .first::<User>(conn)
            .ok();

        // 이미지 ID들 조회 (최대 3개)
        let mut image_ids_vec: Vec<Uuid> = product_images::table
            .filter(product_images::note_id.eq(note.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        if image_ids_vec.is_empty() {
            if let Some(id) = product_images::table
                .filter(product_images::note_id.is_null())
                .filter(product_images::product_id.eq(note.product_id))
                .select(product_images::id)
                .first::<Uuid>(conn)
                .ok()
            {
                image_ids_vec.push(id);
            }
        }

        let image_ids = if image_ids_vec.is_empty() {
            None
        } else {
            Some(image_ids_vec)
        };

        result.push(NoteResponse {
            note,
            product,
            user,
            image_ids,
            flavors: None,
        });
    }

    Ok(result)
}

fn db_get_my_reports(
    pool: web::Data<Pool>,
    user_sub: String,
) -> Result<Vec<Report>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // sub로 user_id 조회
    let user_id = match users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?.id,
        Err(e) => return Err(handler_disel_error(e)),
    };

    // user_id에 해당하는 reports 전부 조회
    let result = reports::table
        .filter(reports::user_id.eq(user_id))
        .load::<Report>(conn)
        .map_err(handler_disel_error)?;

    Ok(result)
}

fn db_create_report(
    pool: web::Data<Pool>,
    item: web::Json<CreateReportParams>,
    user_sub: String,
) -> Result<Report, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // sub로 user_id 조회 (못 찾으면 실패)
    let user_id = match users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => return Err(CommonResponseError::AuthValidationFail),
        Err(e) => return Err(handler_disel_error(e)),
    };

    // product_id 유무에 따라 type 결정: 있으면 0, 없으면 1
    let report_type: i16 = if item.product_id.is_some() { 0 } else { 1 };

    let new_report = NewReport {
        id: Uuid::new_v4(),
        product_id: item.product_id,
        user_id,
        body: item.body.clone(),
        state: Some(0),
        reply: None,
        registered: Some(Utc::now()),
        type_: report_type,
    };

    let report = insert_into(reports::table)
        .values(&new_report)
        .get_result::<Report>(conn)
        .map_err(handler_disel_error)?;

    Ok(report)
}

fn db_get_product_count(pool: web::Data<Pool>) -> Result<i64, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    products::table
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)
}
