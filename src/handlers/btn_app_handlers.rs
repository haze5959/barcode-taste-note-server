use crate::Pool;
use crate::constants::HOME_INFO_LENGTH;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{CommonResponse, User, ProductLite, Note};
use crate::schema::{product_images, users, products, notes};
use crate::handlers::products_handlers::ProductListItem;
use crate::handlers::notes_handlers::NoteResponse;
use actix_web::{Error, HttpResponse, web};
use diesel::expression_methods::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct HomeResponse {
    pub recent_notes: Vec<NoteResponse>,
    pub recent_products: Vec<ProductListItem>
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /btn/home
pub async fn get_home_info(db: web::Data<Pool>) -> Result<HttpResponse, Error> {
    let db1 = db.clone();
    let db2 = db.clone();

    let (notes_list, products_list) = futures::try_join!(
        web::block(move || db_get_notes_list(db1)),
        web::block(move || db_get_products_list(db2)),
    )?;

    let notes_list = notes_list?;
    let products_list = products_list?;
    
    let data = HomeResponse {
        recent_notes: notes_list,
        recent_products: products_list
    };

    let response = CommonResponse {
        result: true,
        data: data,
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
