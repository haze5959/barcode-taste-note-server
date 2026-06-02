use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::models::{CommonResponse, ProductImage, NewProductImage};
use crate::schema::{product_images, users};
use crate::errors::handler_disel_error;
use crate::utils::auth::{get_auth_info, AuthInfo};
use crate::utils::db::get_user_id_by_sub;
use crate::utils::r2::R2Client;
use crate::handlers::users_handler::register_user;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use diesel::dsl::{insert_into, delete};
use chrono::Utc;
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::io::Read;
use log::error;

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageUploadResponse {
    pub id: Uuid,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub product_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
}

#[derive(Debug, MultipartForm)]
pub struct ImageUploadForm {
    #[multipart(limit = "1MB")]
    pub image: TempFile,
    pub id: Text<String>,
    pub product_id: Option<Text<String>>,
    pub note_id: Option<Text<String>>,
}

#[derive(Debug, MultipartForm)]
pub struct ProfileImageUploadForm {
    #[multipart(limit = "1MB")]
    pub image: TempFile,
    pub id: Text<String>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /images
/// 이미지 업로드 (multipart/form-data)
/// form fields: product_id (optional), note_id (optional), image (file)
pub async fn upload_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    MultipartForm(form): MultipartForm<ImageUploadForm>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req).ok();
    let image_id = Uuid::parse_str(&form.id.0)
        .map_err(|_| CommonResponseError::InvalidParameter)?;
    
    let product_id = form.product_id.and_then(|t| Uuid::parse_str(&t.0).ok());
    let note_id = form.note_id.and_then(|t| Uuid::parse_str(&t.0).ok());

    // 임시 파일에서 이미지 데이터 읽기
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

    let r2_for_db = r2.clone();
    let image = web::block(move || {
        db_add_product_image(db, r2_for_db, product_id, note_id, auth_info, image_id)
    })
    .await??;

    // R2 업로드
    r2.upload_image(&format!("images/{}", image_id), image_bytes, "image/jpeg").await?;

    let response = CommonResponse {
        result: true,
        data: image.id,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /images/profile
/// 프로필 이미지 업로드 (multipart/form-data)
pub async fn upload_profile_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    MultipartForm(form): MultipartForm<ProfileImageUploadForm>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let image_id = Uuid::parse_str(&form.id.0)
        .map_err(|_| CommonResponseError::InvalidParameter)?;

    // 이미지 데이터 읽기
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

    let (image_uuid, old_image_id) = web::block(move || {
        db_create_profile_image_with_db(db, auth_info, image_id)
    })
    .await??;

    // 기존 이미지가 있다면 R2에서 이동 (Soft Delete)
    if let Some(old_id) = old_image_id {
        if let Err(e) = r2.move_to_deleted(&format!("images/profile/{}", old_id)).await {
            error!("[R2 Soft Delete Error] Failed to move old profile image {}: {:?}", old_id, e);
        }
    }

    // R2 업로드
    r2.upload_image(&format!("images/profile/{}", image_id), image_bytes, "image/jpeg").await?;

    let response = CommonResponse {
        result: true,
        data: image_uuid,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /images
pub async fn get_images(
    db: web::Data<Pool>,
    query: web::Query<ImageListQuery>,
) -> Result<HttpResponse, Error> {
    let images = web::block(move || db_get_images(db, query.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: images,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for DELETE
// ============================================

/// Path: /images/{id}
pub async fn delete_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    image_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let image_uuid = image_id.into_inner();

    let r2_for_db = r2.clone();
    let _delete_result = web::block(move || db_delete_image(db, r2_for_db, image_uuid, auth_info)).await??;

    // R2에서 이미지를 삭제(이동)
    r2.move_to_deleted(&format!("images/{}", image_uuid)).await?;

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

fn db_get_images(
    pool: web::Data<Pool>,
    query: ImageListQuery,
) -> Result<Vec<Uuid>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let mut images_query = product_images::table.into_boxed()
        .filter(product_images::public_scope.is_null().or(product_images::public_scope.eq(2)));

    if let Some(product_id) = query.product_id {
        images_query = images_query.filter(product_images::product_id.eq(product_id));
    }
    if let Some(note_id) = query.note_id {
        images_query = images_query.filter(product_images::note_id.eq(note_id));
    }

    let images_list: Vec<Uuid> = images_query
        .select(product_images::id)
        .order(product_images::registered.desc())
        .offset(offset)
        .limit(per)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    Ok(images_list)
}

fn db_add_product_image(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    product_id: Option<Uuid>,
    note_id: Option<Uuid>,
    auth_info: Option<AuthInfo>,
    image_id: Uuid,
) -> Result<ProductImage, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id: Option<Uuid> = if let Some(auth_info) = auth_info {
        let id = match get_user_id_by_sub(conn, &auth_info.sub) {
            Ok(id) => id,
            Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
            Err(e) => return Err(e),
        };
        Some(id)
    } else {
        None
    };

    let new_image = NewProductImage {
        id: image_id,
        product_id,
        note_id,
        user_id: user_id,
        registered: Utc::now(),
        public_scope: None,
    };

    let image = insert_into(product_images::table)
        .values(&new_image)
        .get_result::<ProductImage>(conn)
        .map_err(handler_disel_error)?;

    Ok(image)
}

fn db_delete_image(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    image_id: Uuid,
    auth_info: AuthInfo,
) -> Result<bool, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2.clone())?.id,
        Err(e) => return Err(e),
    };

    // 이미지 소유자 확인
    let image = product_images::table
        .find(image_id)
        .first::<ProductImage>(conn)
        .map_err(handler_disel_error)?;

    if image.user_id != Some(user_id) {
        return Err(CommonResponseError::AuthValidationFail);
    }

    // DB에서 이미지 삭제
    let count = delete(product_images::table.find(image_id))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(count == 1)
}

pub fn db_create_profile_image_with_db(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
    new_image_id: Uuid,
) -> Result<(Uuid, Option<Uuid>), CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 조회
    let user = match users::table
        .select(crate::models::USER_COLUMNS)
        .filter(users::sub.eq(&auth_info.sub))
        .first::<crate::models::User>(conn)
    {
        Ok(u) => u,
        Err(diesel::result::Error::NotFound) => return Err(CommonResponseError::AuthValidationFail),
        Err(e) => return Err(handler_disel_error(e)),
    };

    // 유저 정보 업데이트
    diesel::update(users::table.find(user.id))
        .set(users::image_id.eq(Some(new_image_id)))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok((new_image_id, user.image_id))
}