use crate::Pool;
use crate::constants::IMAGE_DIR;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::models::{CommonResponse, ProductImage, NewProductImage};
use crate::schema::{product_images, users};
use crate::errors::handler_disel_error;
use crate::handlers::users_handler::register_user;
use crate::utils::auth::get_sub;
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
    #[multipart(limit = "10MB")]
    pub image: TempFile,
    pub id: Text<String>,
    pub product_id: Option<Text<String>>,
    pub note_id: Option<Text<String>>,
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
    let user_sub = get_sub(req)?;
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
        db_create_image_with_file(db, product_id, note_id, Some(user_sub), image_id, image_bytes)
    })
    .await??;

    let response = CommonResponse {
        result: true,
        data: image.id,
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
    let user_sub = get_sub(req)?;
    let image_uuid = image_id.into_inner();

    let _delete_result = web::block(move || db_delete_image(db, image_uuid, user_sub)).await??;

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
    user_sub: Option<String>,
    image_id: Uuid,
    image_bytes: Vec<u8>,
) -> Result<ProductImage, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id: Option<Uuid> = if let Some(user_sub) = user_sub {
        let id = match users::table
            .filter(users::sub.eq(&user_sub))
            .select(users::id)
            .first::<Uuid>(conn) {
            Ok(id) => id,
            Err(diesel::result::Error::NotFound) => {
                register_user(conn, None, &user_sub)?.id
            }
            Err(e) => return Err(handler_disel_error(e)),
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
    user_sub: String,
) -> Result<bool, CommonResponseError> {
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