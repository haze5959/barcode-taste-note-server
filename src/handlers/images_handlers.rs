use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::ServiceError;
use crate::models::{CommonResponse, ProductImage, NewProductImage, User};
use crate::schema::{product_images, users};
use crate::errors::handler_disel_error;
use crate::utils::auth::get_sub;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use diesel::dsl::{insert_into, delete};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::io::{Read, Write};

const IMAGE_DIR: &str = "./static/images";
const DELETED_DIR: &str = "./static/images/deleted";

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageUploadResponse {
    pub id: Uuid,
    pub url: String,
}

#[derive(Debug, MultipartForm)]
pub struct ImageUploadForm {
    #[multipart(limit = "10MB")]
    pub image: TempFile,
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
        db_create_image_with_file(db, product_id, note_id, user_sub, image_bytes)
    })
    .await??;

    let response = CommonResponse {
        result: true,
        data: ImageUploadResponse {
            id: image.id,
            url: format!("/static/images/{}.jpeg", image.id),
        },
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
    move_image_to_deleted(image_uuid).await?;

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

fn db_create_image_with_file(
    pool: web::Data<Pool>,
    product_id: Option<Uuid>,
    note_id: Option<Uuid>,
    user_sub: String,
    image_bytes: Vec<u8>,
) -> Result<ProductImage, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    let new_image_id = Uuid::new_v4();
    let new_image = NewProductImage {
        id: new_image_id,
        product_id,
        note_id,
        user_id: Some(user.id),
    };

    let image = insert_into(product_images::table)
        .values(&new_image)
        .get_result::<ProductImage>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 이미지 폴더 생성 (없으면)
    std::fs::create_dir_all(IMAGE_DIR).map_err(|e| {
        eprintln!("Failed to create image directory: {}", e);
        ServiceError::InternalServerError
    })?;

    // 이미지 파일 저장
    let file_path = format!("{}/{}.jpeg", IMAGE_DIR, new_image_id);
    let mut file = std::fs::File::create(&file_path).map_err(|e| {
        eprintln!("Failed to create image file: {}", e);
        ServiceError::InternalServerError
    })?;

    file.write_all(&image_bytes).map_err(|e| {
        eprintln!("Failed to write image file: {}", e);
        ServiceError::InternalServerError
    })?;

    Ok(image)
}

fn db_delete_image(
    pool: web::Data<Pool>,
    image_id: Uuid,
    user_sub: String,
) -> Result<bool, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 이미지 소유자 확인
    let image = product_images::table
        .find(image_id)
        .first::<ProductImage>(conn)
        .map_err(|e| handler_disel_error(e))?;

    if image.user_id != Some(user.id) {
        return Err(ServiceError::BadRequest("Not authorized".to_string()));
    }

    // DB에서 이미지 삭제
    let count = delete(product_images::table.find(image_id))
        .execute(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(count == 1)
}

async fn move_image_to_deleted(image_id: Uuid) -> Result<(), Error> {
    // deleted 폴더 생성 (없으면)
    std::fs::create_dir_all(DELETED_DIR).map_err(|e| {
        eprintln!("Failed to create deleted directory: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to create deleted directory")
    })?;

    let source_path = format!("{}/{}.jpeg", IMAGE_DIR, image_id);
    let dest_path = format!("{}/{}.jpeg", DELETED_DIR, image_id);

    // 파일이 존재하는 경우에만 이동
    if std::path::Path::new(&source_path).exists() {
        std::fs::rename(&source_path, &dest_path).map_err(|e| {
            eprintln!("Failed to move image to deleted folder: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to move image")
        })?;
    }

    Ok(())
}
