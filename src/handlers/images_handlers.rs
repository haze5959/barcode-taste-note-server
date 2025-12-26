use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::ServiceError;
use crate::models::{CommonResponse, ProductImage, NewProductImage, User};
use crate::schema::{product_images, users, barcodes};
use crate::errors::handler_disel_error;
use crate::utils::auth::get_sub;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{insert_into, delete};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateImageParams {
    pub product_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /images
pub async fn create_image(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<CreateImageParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let image = web::block(move || db_create_image(db, item, user_sub)).await??;
    let response = CommonResponse {
        result: true,
        data: image,
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
    let _delete_result = web::block(move || db_delete_image(db, image_id.into_inner(), user_sub)).await??;
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

fn db_create_image(
    pool: web::Data<Pool>,
    item: web::Json<CreateImageParams>,
    user_sub: String,
) -> Result<ProductImage, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user = users::table
        .filter(users::sub.eq(&user_sub))
        .first::<User>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // product_id가 제공된 경우 barcode_id 찾기
    let barcode_id = if let Some(product_id) = item.product_id {
        let barcode = barcodes::table
            .filter(barcodes::product_id.eq(product_id))
            .first::<crate::models::Barcode>(conn)
            .map_err(|e| handler_disel_error(e))?;
        barcode.id
    } else if item.note_id.is_some() {
        // note_id가 제공된 경우, 해당 노트의 barcode_id 사용
        // (실제로는 note에서 barcode_id를 가져와야 하지만, 여기서는 임시로 처리)
        // TODO: note에서 barcode_id 가져오기
        return Err(ServiceError::BadRequest("product_id or note with barcode_id is required".to_string()));
    } else {
        return Err(ServiceError::BadRequest("product_id or note_id is required".to_string()));
    };

    let new_image_id = Uuid::new_v4();
    let new_image = NewProductImage {
        id: new_image_id,
        barcode_id,
        note_id: item.note_id,
        user_id: Some(user.id),
    };

    let image = insert_into(product_images::table)
        .values(&new_image)
        .get_result::<ProductImage>(conn)
        .map_err(|e| handler_disel_error(e))?;

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

    // 이미지 삭제
    let count = delete(product_images::table.find(image_id))
        .execute(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(count == 1)
}
