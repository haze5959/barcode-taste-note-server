use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{CommonResponse, Product, Report, NoteSimple, NOTE_SIMPLE_COLUMNS, NoteListQuery, NoteListResponse};
use crate::schema::{barcodes, favorites, flavor_tags, notes, product_images, products, reports};
use crate::handlers::notes_handlers::build_note_list_response;
use crate::utils::auth::get_auth_info;
use crate::utils::openai::get_embedding;
use crate::utils::r2::R2Client;

use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::Connection;
use diesel::ExpressionMethods;
use diesel::OptionalExtension;
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use std::io::Read;
use uuid::Uuid;
use log::{error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminProductMainImageResponse {
    pub image_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminProductQuery {
    pub product_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminUpdateProductParams {
    pub product_id: Uuid,
    pub name: Option<String>,
    pub desc: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<i16>,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminProductDetailsQuery {
    pub product_name: String,
    pub only_details: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminProductDetailsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminUpdateReportParams {
    pub id: Uuid,
    pub reply: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminMergeProductParams {
    pub product_id: Uuid,
    pub to_product_id: Uuid,
}

#[derive(Debug, MultipartForm)]
pub struct AdminImageUploadForm {
    #[multipart(limit = "1MB")]
    pub image: TempFile,
    pub image_id: Text<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminImageUrlUploadParams {
    pub image_id: Option<Uuid>,
    pub product_id: Option<Uuid>,
    pub add_image_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminDashboardResponse {
    pub registered_user_count: i64,
    pub monthly_registered_user_count: i64,
    pub product_count: i64,
    pub monthly_note_count: i64,
    pub daily_note_count: i64,
    pub not_reply_report_count: i64,
    pub premium_user_count: i64,
    pub web_active_users_30d: Option<i64>,
    pub ios_active_users_30d: Option<i64>,
    pub android_active_users_30d: Option<i64>,
    pub gemini_request_success_count: i64,
    pub gemini_request_failure_count: i64,
    pub barcode_request_success_count: i64,
    pub barcode_request_failure_count: i64,
}

fn validate_admin(req: &HttpRequest) -> Result<(), CommonResponseError> {
    let auth_info = get_auth_info(req.clone()).map_err(|_| CommonResponseError::AuthValidationFail)?;
    let user_sub = auth_info.sub;
    let admin_sub = std::env::var("ADMIN_SUB").unwrap_or_default();
    if user_sub != admin_sub {
        return Err(CommonResponseError::AuthValidationFail);
    }
    Ok(())
}

async fn fetch_ga_total_users(property_id: &str, platform: &str) -> Option<i64> {
    let provider = match gcp_auth::provider().await {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to create GCP provider: {}", e);
            return None;
        }
    };

    let token = match provider.token(&["https://www.googleapis.com/auth/analytics.readonly"]).await {
        Ok(t) => t.as_str().to_string(),
        Err(e) => {
            log::error!("Failed to get GCP token: {}", e);
            return None;
        }
    };

    let url = format!("https://analyticsdata.googleapis.com/v1beta/properties/{}:runReport", property_id);
    let body = serde_json::json!({
        "dateRanges": [{ "startDate": "30daysAgo", "endDate": "today" }],
        "metrics": [{ "name": "totalUsers" }],
        "dimensionFilter": {
            "filter": {
                "fieldName": "platform",
                "stringFilter": {
                    "value": platform
                }
            }
        }
    });

    let client = reqwest::Client::new();
    let res = client.post(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .ok()?;

    let json: serde_json::Value = match res.json().await {
        Ok(j) => j,
        Err(e) => {
            log::error!("Failed to parse GA response JSON: {}", e);
            return None;
        }
    };
    
    // 디버깅을 위해 응답 구조 로깅
    log::info!("GA Response structure for property {} and platform {}: {}", property_id, platform, json);
    
    // 데이터가 아예 없는 경우 (rows가 생략됨) 0으로 처리
    let rows = match json.get("rows") {
        Some(r) => r.as_array(),
        None => {
            log::info!("GA Property {} - Platform {} has no rows, returning 0", property_id, platform);
            return Some(0);
        }
    };

    let count_str = rows?
        .get(0)?
        .get("metricValues")?
        .as_array()?
        .get(0)?
        .get("value")?
        .as_str()?;
    
    count_str.parse::<i64>().ok()
}

// ============================================
// MARK: GET /admin/dashboard
// ============================================
pub async fn get_dashboard(
    req: HttpRequest,
    db: web::Data<Pool>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let db_clone = db.clone();
    let dashboard_data_future = web::block(move || db_get_dashboard(db_clone));
    
    let ga_pid = std::env::var("GA_PROPERTY_ID").ok();

    let web_future = async {
        if let Some(ref pid) = ga_pid { fetch_ga_total_users(&pid, "web").await } else { None }
    };
    
    let ios_future = async {
        if let Some(ref pid) = ga_pid { fetch_ga_total_users(&pid, "ios").await } else { None }
    };

    let android_future = async {
        if let Some(ref pid) = ga_pid { fetch_ga_total_users(&pid, "android").await } else { None }
    };

    let (db_res, web_count, ios_count, android_count) = tokio::join!(dashboard_data_future, web_future, ios_future, android_future);
    
    let yesterday_str = (chrono::Local::now() - chrono::Duration::days(1)).format("%Y%m%d").to_string();
    let logs_dir = format!("logs/{}", yesterday_str);
    
    let gemini_request_success_count = crate::utils::logger::count_lines_in_file(&format!("{}/gemini_requests_success.log", logs_dir)).await;
    let gemini_request_failure_count = crate::utils::logger::count_lines_in_file(&format!("{}/gemini_requests_failure.log", logs_dir)).await;
    let barcode_request_success_count = crate::utils::logger::count_lines_in_file(&format!("{}/barcode_requests_success.log", logs_dir)).await;
    let barcode_request_failure_count = crate::utils::logger::count_lines_in_file(&format!("{}/barcode_requests_failure.log", logs_dir)).await;

    let mut dashboard_data = db_res??;
    dashboard_data.web_active_users_30d = web_count;
    dashboard_data.ios_active_users_30d = ios_count;
    dashboard_data.android_active_users_30d = android_count;
    dashboard_data.gemini_request_success_count = gemini_request_success_count;
    dashboard_data.gemini_request_failure_count = gemini_request_failure_count;
    dashboard_data.barcode_request_success_count = barcode_request_success_count;
    dashboard_data.barcode_request_failure_count = barcode_request_failure_count;

    let response = CommonResponse {
        result: true,
        data: dashboard_data,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn db_get_dashboard(pool: web::Data<Pool>) -> Result<AdminDashboardResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let now = chrono::Utc::now();
    let thirty_days_ago = now - chrono::Duration::days(30);
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);

    let registered_user_count = crate::schema::users::table
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let monthly_registered_user_count = crate::schema::users::table
        .filter(crate::schema::users::registered.ge(thirty_days_ago))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let product_count = products::table
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let monthly_note_count = notes::table
        .filter(notes::registered.ge(thirty_days_ago))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let daily_note_count = notes::table
        .filter(notes::registered.ge(twenty_four_hours_ago))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let not_reply_report_count = reports::table
        .filter(reports::reply.is_null())
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    let premium_user_count = crate::schema::users::table
        .filter(crate::schema::users::premium_expire_at.ge(now))
        .count()
        .get_result::<i64>(conn)
        .map_err(handler_disel_error)?;

    Ok(AdminDashboardResponse {
        registered_user_count,
        monthly_registered_user_count,
        product_count,
        monthly_note_count,
        daily_note_count,
        not_reply_report_count,
        premium_user_count,
        web_active_users_30d: None,
        ios_active_users_30d: None,
        android_active_users_30d: None,
        gemini_request_success_count: 0,
        gemini_request_failure_count: 0,
        barcode_request_success_count: 0,
        barcode_request_failure_count: 0,
    })
}

// ============================================
// MARK: GET /admin/report
// ============================================
pub async fn get_reports(
    req: HttpRequest,
    db: web::Data<Pool>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let reports_list = web::block(move || db_get_pending_reports(db)).await??;
    
    let response = CommonResponse {
        result: true,
        data: reports_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn db_get_pending_reports(pool: web::Data<Pool>) -> Result<Vec<Report>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    
    let list = reports::table
        .filter(reports::state.eq(0))
        .load::<Report>(conn)
        .map_err(handler_disel_error)?;

    Ok(list)
}

// ============================================
// MARK: GET /admin/product/barcodes
// ============================================
pub async fn get_admin_product_barcodes(
    req: HttpRequest,
    db: web::Data<Pool>,
    query: web::Query<AdminProductQuery>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let barcode_ids = web::block(move || db_get_admin_product_barcodes(db, query.product_id)).await??;

    let response = CommonResponse {
        result: true,
        data: barcode_ids,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn db_get_admin_product_barcodes(pool: web::Data<Pool>, product_id: Uuid) -> Result<Vec<String>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let barcode_ids: Vec<String> = barcodes::table
        .filter(barcodes::product_id.eq(product_id))
        .select(barcodes::barcode_id)
        .load::<String>(conn)
        .map_err(handler_disel_error)?;

    Ok(barcode_ids)
}

// ============================================
// MARK: GET /admin/product/main_image
// ============================================
pub async fn get_admin_product_main_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    query: web::Query<AdminProductQuery>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let image_id = web::block(move || db_get_admin_product_main_image(db, query.product_id)).await??;
    
    let response = CommonResponse {
        result: true,
        data: AdminProductMainImageResponse { image_id },
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn db_get_admin_product_main_image(pool: web::Data<Pool>, product_id: Uuid) -> Result<Option<Uuid>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let main_image_id = product_images::table
        .filter(product_images::product_id.eq(product_id))
        .filter(product_images::note_id.is_null())
        .order(product_images::registered.asc())
        .select(product_images::id)
        .first::<Uuid>(conn)
        .optional()
        .map_err(handler_disel_error)?;

    Ok(main_image_id)
}

// ============================================
// MARK: GET /admin/product/details
// ============================================
pub async fn get_admin_product_details(
    req: HttpRequest,
    query: web::Query<AdminProductDetailsQuery>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let query_inner = query.into_inner();
    let product_name = query_inner.product_name;

    if let Some(info) = crate::utils::gemini::generate_product_info_with_gemini(&product_name).await {
        let response_data = AdminProductDetailsResponse {
            desc: if query_inner.only_details { None } else { Some(info.description) },
            details: info.details,
        };
        let response = CommonResponse {
            result: true,
            data: response_data,
            error: None,
        };
        Ok(HttpResponse::Ok().json(response))
    } else {
        let resp: CommonResponse<Option<()>> = CommonResponse {
            result: false,
            data: None,
            error: Some(CommonResponseError::InternalServerError as u8),
        };
        Ok(HttpResponse::Ok().json(resp))
    }
}

// ============================================
// MARK: PUT /admin/product
// ============================================
pub async fn update_admin_product(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<AdminUpdateProductParams>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let product_id = item.product_id;
    let name = item.name.clone();
    let desc = item.desc.clone();
    let type_ = item.type_;
    let details = item.details.clone();

    // 임베딩 갱신 필요 여부
    let new_embedding = if let Some(ref new_name) = name {
        get_embedding(new_name).await.ok()
    } else {
        None
    };

    let updated_product = web::block(move || {
        db_update_admin_product(db, product_id, name, desc, type_, details, new_embedding)
    })
    .await??;

    let response = CommonResponse {
        result: true,
        data: updated_product,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

#[derive(diesel::AsChangeset)]
#[diesel(table_name = products)]
struct AdminProductChangeset {
    name: Option<String>,
    desc: Option<String>,
    #[diesel(column_name = type_)]
    type_: Option<i16>,
    embedding: Option<Vector>,
    details: Option<serde_json::Value>,
}

fn db_update_admin_product(
    pool: web::Data<Pool>,
    product_id: Uuid,
    name: Option<String>,
    desc: Option<String>,
    type_: Option<i16>,
    details: Option<serde_json::Value>,
    new_embedding: Option<Vector>,
) -> Result<Product, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let changeset = AdminProductChangeset {
        name,
        desc,
        type_,
        embedding: new_embedding,
        details,
    };

    let updated_product = conn.transaction::<Product, CommonResponseError, _>(|conn| {
        let result = diesel::update(products::table.find(product_id))
            .set(&changeset)
            .returning(crate::models::PRODUCT_COLUMNS)
            .get_result::<Product>(conn)
            .map_err(handler_disel_error)?;
        
        Ok(result)
    })?;

    Ok(updated_product)
}

// ============================================
// MARK: PUT /admin/report
// ============================================
pub async fn update_admin_report(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<AdminUpdateReportParams>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let updated_report = web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::update(reports::table.find(item.id))
            .set((
                reports::reply.eq(&item.reply),
                reports::state.eq(1),
            ))
            .get_result::<Report>(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    Ok(HttpResponse::Ok().json(updated_report))
}

// ============================================
// MARK: POST /admin/product/merge
// ============================================
pub async fn merge_admin_product(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<AdminMergeProductParams>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    web::block(move || {
        let conn = &mut db.get().unwrap();
        conn.transaction::<(), diesel::result::Error, _>(|conn| {
            diesel::update(barcodes::table.filter(barcodes::product_id.eq(item.product_id)))
                .set(barcodes::product_id.eq(item.to_product_id))
                .execute(conn)?;
                
            diesel::update(product_images::table.filter(product_images::product_id.eq(item.product_id)))
                .set(product_images::product_id.eq(item.to_product_id))
                .execute(conn)?;
                
            diesel::update(favorites::table.filter(favorites::product_id.eq(item.product_id)))
                .set(favorites::product_id.eq(item.to_product_id))
                .execute(conn)?;
                
            diesel::update(notes::table.filter(notes::product_id.eq(item.product_id)))
                .set(notes::product_id.eq(item.to_product_id))
                .execute(conn)?;
                
            diesel::update(flavor_tags::table.filter(flavor_tags::product_id.eq(item.product_id)))
                .set(flavor_tags::product_id.eq(item.to_product_id))
                .execute(conn)?;
                
            diesel::delete(products::table.find(item.product_id))
                .execute(conn)?;
                
            Ok(())
        }).map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: DELETE /admin/products/{product_id}
// ============================================
pub async fn delete_admin_product(
    req: HttpRequest,
    db: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let product_id = path.into_inner();

    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::delete(products::table.find(product_id))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: POST /admin/image
// ============================================
pub async fn upload_admin_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    MultipartForm(form): MultipartForm<AdminImageUploadForm>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let image_id_str = form.image_id.0.clone();
    let old_image_id = Uuid::parse_str(&image_id_str)
        .map_err(|_| CommonResponseError::InvalidParameter)?;

    let temp_file_path = form.image.file.path();
    let mut image_file = std::fs::File::open(temp_file_path).map_err(|e| {
        error!("Failed to open temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    let mut image_bytes = Vec::new();
    image_file.read_to_end(&mut image_bytes).map_err(|e| {
        error!("Failed to read temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    // 1. DB에서 기존 product_images row 조회 → product_id 추출
    let db_clone = db.clone();
    let old_product_id = web::block(move || {
        let conn = &mut db_clone.get().unwrap();
        product_images::table
            .find(old_image_id)
            .select(product_images::product_id)
            .first::<Option<Uuid>>(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    // 2. R2에서 기존 이미지 삭제
    r2.delete_image(&format!("images/{}", old_image_id)).await?;

    // 3. DB에서 기존 product_images row 삭제
    let db_clone2 = db.clone();
    web::block(move || {
        let conn = &mut db_clone2.get().unwrap();
        diesel::delete(product_images::table.find(old_image_id))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    // 4. 새 UUID 생성 후 R2 업로드
    let new_image_id = Uuid::new_v4();
    r2.upload_image(&format!("images/{}", new_image_id), image_bytes, "image/jpeg").await?;

    // 5. 새 product_images row 삽입
    let new_image = crate::models::NewProductImage {
        id: new_image_id,
        product_id: old_product_id,
        note_id: None,
        user_id: None,
        registered: chrono::Utc::now(),
        public_scope: None,
    };
    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::insert_into(product_images::table)
            .values(&new_image)
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: POST /admin/image/url
// ============================================
pub async fn upload_admin_image_by_url(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    item: web::Json<AdminImageUrlUploadParams>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let old_image_id = item.image_id;
    let add_image_url = &item.add_image_url;

    // 이미지 다운로드
    let image_bytes = reqwest::get(add_image_url)
        .await
        .map_err(|e| {
            error!("Failed to download image from URL: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to download image")
        })?
        .bytes()
        .await
        .map_err(|e| {
            error!("Failed to read downloaded image bytes: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to read image bytes")
        })?
        .to_vec();

    let product_id = if let Some(pid) = item.product_id {
        Some(pid)
    } else {
        let oid = old_image_id.ok_or(CommonResponseError::InvalidParameter)?;

        // 1. DB에서 기존 product_images row 조회 → product_id 추출
        let db_clone = db.clone();
        let extracted_pid = web::block(move || {
            let conn = &mut db_clone.get().unwrap();
            product_images::table
                .find(oid)
                .select(product_images::product_id)
                .first::<Option<Uuid>>(conn)
                .map_err(handler_disel_error)
        })
        .await??;

        // 2. R2에서 기존 이미지 삭제
        r2.delete_image(&format!("images/{}", oid)).await?;

        // 3. DB에서 기존 product_images row 삭제
        let db_clone2 = db.clone();
        web::block(move || {
            let conn = &mut db_clone2.get().unwrap();
            diesel::delete(product_images::table.find(oid))
                .execute(conn)
                .map_err(handler_disel_error)
        })
        .await??;

        extracted_pid
    };

    // 4. 새 UUID 생성 후 R2 업로드
    let new_image_id = Uuid::new_v4();
    r2.upload_image(&format!("images/{}", new_image_id), image_bytes, "image/jpeg").await?;

    // 5. 새 product_images row 삽입
    let new_image = crate::models::NewProductImage {
        id: new_image_id,
        product_id: product_id,
        note_id: None,
        user_id: None,
        registered: chrono::Utc::now(),
        public_scope: None,
    };
    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::insert_into(product_images::table)
            .values(&new_image)
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: GET /admin/notes
// ============================================
pub async fn get_admin_notes(
    req: HttpRequest,
    db: web::Data<Pool>,
    query: web::Query<NoteListQuery>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let notes_list = web::block(move || db_get_admin_notes(db, query.into_inner())).await??;
    
    let response = CommonResponse {
        result: true,
        data: notes_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn db_get_admin_notes(
    pool: web::Data<Pool>,
    query: NoteListQuery,
) -> Result<Vec<NoteListResponse>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let mut notes_query = notes::table.into_boxed();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // product_id 필터링
    if let Some(product_id) = query.product_id {
        notes_query = notes_query.filter(notes::product_id.eq(product_id));
    }

    let notes_list: Vec<NoteSimple> = notes_query
        .select(NOTE_SIMPLE_COLUMNS)
        .order(notes::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<NoteSimple>(conn)
        .map_err(handler_disel_error)?;

    build_note_list_response(conn, notes_list)
}

// ============================================
// MARK: DELETE /admin/images/:id
// ============================================
pub async fn delete_admin_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let image_id = path.into_inner();

    // R2에서 이미지 삭제
    r2.delete_image(&format!("images/{}", image_id)).await?;

    // DB에서 이미지 삭제
    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::delete(product_images::table.find(image_id))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminBarcodePayload {
    pub barcode_id: String,
    pub product_id: Uuid,
}

// ============================================
// MARK: PUT /admin/barcode
// ============================================
pub async fn update_admin_barcode(
    req: HttpRequest,
    db: web::Data<Pool>,
    payload: web::Json<AdminBarcodePayload>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;
    let payload = payload.into_inner();

    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::update(barcodes::table.filter(barcodes::barcode_id.eq(payload.barcode_id)))
            .set(barcodes::product_id.eq(payload.product_id))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: POST /admin/barcode
// ============================================
pub async fn add_admin_barcode(
    req: HttpRequest,
    db: web::Data<Pool>,
    payload: web::Json<AdminBarcodePayload>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;
    let payload = payload.into_inner();

    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::insert_into(barcodes::table)
            .values((
                barcodes::id.eq(Uuid::new_v4()),
                barcodes::barcode_id.eq(payload.barcode_id),
                barcodes::product_id.eq(payload.product_id),
            ))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: DELETE /admin/barcode/:barcode_id
// ============================================
pub async fn delete_admin_barcode(
    req: HttpRequest,
    db: web::Data<Pool>,
    barcode_id_param: web::Path<String>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;
    let barcode_id = barcode_id_param.into_inner();

    web::block(move || {
        let conn = &mut db.get().unwrap();
        diesel::delete(barcodes::table.filter(barcodes::barcode_id.eq(barcode_id)))
            .execute(conn)
            .map_err(handler_disel_error)
    })
    .await??;

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: GET /admin/deleted/images
// ============================================

/// clean_image 배치(고아 이미지 → R2 deleted/images/ 이동)를 실행한 뒤,
/// R2 의 deleted/images/ 경로에 쌓인 파일명(UUID) 목록을 반환한다.
pub async fn get_admin_deleted_images(
    req: HttpRequest,
    r2: web::Data<R2Client>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    // 1. 고아 이미지 정리 배치 실행 (자식 프로세스 대기는 블로킹이므로 web::block 에서 수행)
    let output = web::block(|| {
        std::process::Command::new("./deploy_bin/barnote_batch")
            .arg("clean_image")
            .output()
    })
    .await?
    .map_err(|e| {
        error!("[Admin] clean_image 실행 실패: {}", e);
        CommonResponseError::InternalServerError
    })?;

    if !output.status.success() {
        error!(
            "[Admin] clean_image 비정상 종료 (code={:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(CommonResponseError::InternalServerError.into());
    }
    info!("[Admin] clean_image 완료: {}", String::from_utf8_lossy(&output.stdout).trim());

    // 2. R2 deleted/images/ 경로의 파일명(UUID) 목록 조회
    let prefix = "deleted/images/";
    let keys = r2.list_keys(prefix).await?;
    let names: Vec<String> = keys
        .iter()
        .filter_map(|key| key.strip_prefix(prefix))
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .collect();

    let response = CommonResponse {
        result: true,
        data: names,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: DELETE /admin/deleted/images
// ============================================

/// R2 의 deleted/images/ 경로에 보관 중인 이미지를 전부 삭제한다(영구 삭제).
pub async fn delete_admin_deleted_images(
    req: HttpRequest,
    r2: web::Data<R2Client>,
) -> Result<HttpResponse, Error> {
    validate_admin(&req)?;

    let keys = r2.list_keys("deleted/images/").await?;
    if !keys.is_empty() {
        r2.delete_keys(&keys).await?;
    }
    info!("[Admin] deleted/images 비우기 완료: {}건 삭제", keys.len());

    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}
