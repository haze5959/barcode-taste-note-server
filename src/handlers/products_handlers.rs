use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{Barcode, CommonResponse, NewBarcode, NewProduct, Product};
use crate::schema::{barcodes, favorites, product_images, products, users};
use crate::utils::auth::get_sub;
use chrono::Utc;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{count, insert_into};
use diesel::expression_methods::*;
use diesel::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProductParams {
    pub name: String,
    pub desc: Option<String>,
    #[serde(rename = "type")]
    pub type_: i16,
    pub barcode_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductDetailResponse {
    pub product: Product,
    pub image_ids: Vec<Uuid>,
    pub favorite_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductListItem {
    pub product: Product,
    pub image_ids: Vec<Uuid>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /products``
pub async fn create_product(
    db: web::Data<Pool>,
    item: web::Json<CreateProductParams>,
) -> Result<HttpResponse, Error> {
    let product = web::block(move || db_create_product(db, item)).await??;
    let response = CommonResponse {
        result: true,
        data: product,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Handler for GET
// ============================================

/// Path: /products/{id}
pub async fn get_product_by_id(
    db: web::Data<Pool>,
    in_product_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let product_detail =
        web::block(move || db_get_product_by_id(db, in_product_id.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: product_detail,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products/barcode/{barcode_id}
pub async fn get_product_by_barcode(
    db: web::Data<Pool>,
    in_barcode_id: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let product_detail =
        web::block(move || db_get_product_by_barcode(db, in_barcode_id.into_inner())).await??;
    let response = CommonResponse {
        result: true,
        data: product_detail,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products
pub async fn get_products_list(
    db: web::Data<Pool>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let products_list = web::block(move || db_get_products_list(db, query.into_inner())).await??;
    let data = HashMap::from([("products".to_string(), products_list)]);
    let response = CommonResponse {
        result: true,
        data: data,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products/favorite
pub async fn get_favorite_products_list(
    req: HttpRequest,
    db: web::Data<Pool>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let products_list = web::block(move || db_get_my_favorite_products_list(db, query.into_inner(), user_sub)).await??;
    let data = HashMap::from([("products".to_string(), products_list)]);
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

fn db_create_product(
    pool: web::Data<Pool>,
    item: web::Json<CreateProductParams>,
) -> Result<Product, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let new_product_id = Uuid::new_v4();
    let new_product = NewProduct {
        id: new_product_id,
        name: &item.name,
        desc: item.desc.as_deref(),
        type_: item.type_,
        registered: Utc::now(),
    };

    let product = conn.transaction::<Product, CommonResponseError, _>(|conn| {
        // product insert
        let product = insert_into(products::table)
            .values(&new_product)
            .get_result::<Product>(conn)?;

        // barcode_id가 제공된 경우 바코드 생성
        if let Some(ref barcode_str) = item.barcode_id {
            let new_barcode = NewBarcode {
                id: Uuid::new_v4(),
                barcode_id: barcode_str,
                product_id: new_product_id,
            };

            insert_into(barcodes::table)
                .values(&new_barcode)
                .execute(conn)?;
        }

        Ok(product)
    })?;

    Ok(product)
}

fn db_get_product_by_id(
    pool: web::Data<Pool>,
    in_product_id: Uuid,
) -> Result<ProductDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 제품 조회
    let product = products::table
        .find(in_product_id)
        .first::<Product>(conn)
        .map_err(handler_disel_error)?;

    // 제품 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::product_id.eq(in_product_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(in_product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    Ok(ProductDetailResponse {
        product,
        image_ids: image_ids,
        favorite_count,
    })
}

fn db_get_product_by_barcode(
    pool: web::Data<Pool>,
    barcode_id_str: String,
) -> Result<ProductDetailResponse, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 바코드로 제품 ID 찾기
    let barcode = barcodes::table
        .filter(barcodes::barcode_id.eq(barcode_id_str))
        .first::<Barcode>(conn)
        .map_err(handler_disel_error)?;

    let product_id = barcode.product_id;

    // 제품 조회
    let product = products::table
        .find(product_id)
        .first::<Product>(conn)
        .map_err(handler_disel_error)?;

    // 제품 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::product_id.eq(product_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    Ok(ProductDetailResponse {
        product,
        image_ids: image_ids,
        favorite_count,
    })
}

fn db_get_products_list(
    pool: web::Data<Pool>,
    query: ProductListQuery,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 제품 리스트 조회
    let mut products_query = products::table.into_boxed();

    // 이름 필터링
    if let Some(name_filter) = query.name {
        products_query = products_query.filter(products::name.like(format!("%{}%", name_filter)));
    }

    let products_list: Vec<Product> = products_query
        .offset(offset)
        .limit(per)
        .load::<Product>(conn)
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

fn db_get_my_favorite_products_list(
    pool: web::Data<Pool>,
    query: ProductListQuery,
    user_sub: String,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 유저 ID 조회
    let user_id = users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    let favorite_product_ids: Vec<Uuid> = favorites::table
        .filter(favorites::user_id.eq(user_id))
        .select(favorites::product_id)
        .offset(offset)
        .limit(per)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 제품 리스트 조회
    let mut products_query = products::table
        .filter(products::id.eq_any(&favorite_product_ids))
        .into_boxed();

    // 이름 필터링
    if let Some(name_filter) = query.name {
        products_query = products_query.filter(products::name.like(format!("%{}%", name_filter)));
    }

    let products_list: Vec<Product> = products_query
        .load::<Product>(conn)
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
