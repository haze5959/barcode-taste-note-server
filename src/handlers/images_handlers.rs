use crate::Pool;
use crate::constants::IMAGE_DIR;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::models::{CommonResponse, ProductImage, NewProductImage};
use crate::schema::{product_images, users};
use crate::errors::handler_disel_error;
use crate::handlers::users_handler::register_user;
use crate::utils::auth::{get_auth_info, AuthInfo};
use crate::utils::db::get_user_id_by_sub;
use crate::utils::image_file::move_image_to_deleted;
use chrono::Utc;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use diesel::dsl::{insert_into, delete};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::io::{Read, Write};

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
    MultipartForm(form): MultipartForm<ImageUploadForm>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let image_id = Uuid::parse_str(&form.id.0)
        .map_err(|_| CommonResponseError::InvalidParameter)?;
    
    let product_id = form.product_id.and_then(|t| Uuid::parse_str(&t.0).ok());
    let note_id = form.note_id.and_then(|t| Uuid::parse_str(&t.0).ok());

    // 임시 파일에서 이미지 데이터 읽기
    let temp_file_path = form.image.file.path();
    let mut image_file = std::fs::File::open(temp_file_path).map_err(|e| {
        eprintln!("Failed to open temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    let mut image_bytes = Vec::new();
    image_file.read_to_end(&mut image_bytes).map_err(|e| {
        eprintln!("Failed to read temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    let image = web::block(move || {
        db_create_image_with_file(db, product_id, note_id, Some(auth_info), image_id, image_bytes)
    })
    .await??;

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
    MultipartForm(form): MultipartForm<ProfileImageUploadForm>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let image_id = Uuid::parse_str(&form.id.0)
        .map_err(|_| CommonResponseError::InvalidParameter)?;

    // 임시 파일에서 이미지 데이터 읽기
    let temp_file_path = form.image.file.path();
    let mut image_file = std::fs::File::open(temp_file_path).map_err(|e| {
        eprintln!("Failed to open temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    let mut image_bytes = Vec::new();
    image_file.read_to_end(&mut image_bytes).map_err(|e| {
        eprintln!("Failed to read temp file: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to read uploaded file")
    })?;

    let image_uuid = web::block(move || {
        db_create_profile_image_with_file(db, auth_info, image_id, image_bytes)
    })
    .await??;

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
    image_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let image_uuid = image_id.into_inner();

    let _delete_result = web::block(move || db_delete_image(db, image_uuid, auth_info)).await??;

    // 이미지 파일을 deleted 폴더로 이동
    let _file_delete_result = move_image_to_deleted(image_uuid);

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

    let mut images_query = product_images::table.into_boxed();

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

fn db_create_image_with_file(
    pool: web::Data<Pool>,
    product_id: Option<Uuid>,
    note_id: Option<Uuid>,
    auth_info: Option<AuthInfo>,
    image_id: Uuid,
    image_bytes: Vec<u8>,
) -> Result<ProductImage, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id: Option<Uuid> = if let Some(auth_info) = auth_info {
        let id = match get_user_id_by_sub(conn, &auth_info.sub) {
            Ok(id) => id,
            Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), pool.clone())?.id,
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
    };

    let image = insert_into(product_images::table)
        .values(&new_image)
        .get_result::<ProductImage>(conn)
        .map_err(handler_disel_error)?;

    // 이미지 폴더 생성 (없으면)
    std::fs::create_dir_all(IMAGE_DIR).map_err(|e| {
        eprintln!("Failed to create image directory: {}", e);
        CommonResponseError::InternalServerError
    })?;

    // 이미지 파일 저장
    let file_path = format!("{}/{}", IMAGE_DIR, image_id);
    let mut file: std::fs::File = std::fs::File::create(&file_path).map_err(|e| {
        eprintln!("Failed to create image file: {}", e);
        CommonResponseError::InternalServerError
    })?;

    file.write_all(&image_bytes).map_err(|e| {
        eprintln!("Failed to write image file: {}", e);
        CommonResponseError::InternalServerError
    })?;

    Ok(image)
}

fn db_delete_image(
    pool: web::Data<Pool>,
    image_id: Uuid,
    auth_info: AuthInfo,
) -> Result<bool, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), pool.clone())?.id,
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

pub fn db_create_profile_image_with_file(
    pool: web::Data<Pool>,
    auth_info: AuthInfo,
    new_image_id: Uuid,
    image_bytes: Vec<u8>,
) -> Result<Uuid, CommonResponseError> {
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

    // 기존 이미지가 있으면 deleted 폴더로 이동
    if let Some(existing_image_id) = user.image_id {
        let original_path = format!("static/images/profile/{}", existing_image_id);
        let deleted_dir = "static/images/profile/deleted";
        let deleted_path = format!("{}/{}", deleted_dir, existing_image_id);

        if std::path::Path::new(&original_path).exists() {
            let _ = std::fs::create_dir_all(deleted_dir);
            if std::fs::rename(&original_path, &deleted_path).is_err() {
                if std::fs::copy(&original_path, &deleted_path).is_ok() {
                    let _ = std::fs::remove_file(&original_path);
                }
            }
        }
    }

    // 새 이미지 저장
    let profile_dir = "static/images/profile";
    std::fs::create_dir_all(profile_dir).map_err(|e| {
        eprintln!("Failed to create profile image directory: {}", e);
        CommonResponseError::InternalServerError
    })?;

    let file_path = format!("{}/{}", profile_dir, new_image_id);
    let mut file = std::fs::File::create(&file_path).map_err(|e| {
        eprintln!("Failed to create profile image file: {}", e);
        CommonResponseError::InternalServerError
    })?;

    file.write_all(&image_bytes).map_err(|e| {
        eprintln!("Failed to write profile image file: {}", e);
        CommonResponseError::InternalServerError
    })?;

    // 유저 정보 업데이트
    diesel::update(users::table.find(user.id))
        .set(users::image_id.eq(Some(new_image_id)))
        .execute(conn)
        .map_err(handler_disel_error)?;

    Ok(new_image_id)
}