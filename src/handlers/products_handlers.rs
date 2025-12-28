use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::ServiceError;
use crate::models::{CommonResponse, Product, NewProduct, Barcode, NewBarcode};
use crate::schema::{products, barcodes, product_images, favorites};
use crate::errors::handler_disel_error;
use actix_web::{Error, HttpResponse, web};
use diesel::dsl::{insert_into, count};
use diesel::expression_methods::*;
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
    pub images: Vec<Uuid>,
    pub favorite_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductListItem {
    pub product: Product,
    pub images: Vec<Uuid>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /products
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
    let product_detail = web::block(move || db_get_product_by_id(db, in_product_id.into_inner())).await??;
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
    let product_detail = web::block(move || db_get_product_by_barcode(db, in_barcode_id.into_inner())).await??;
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

// ============================================
// MARK: Internal Methods
// ============================================

fn db_create_product(
    pool: web::Data<Pool>,
    item: web::Json<CreateProductParams>,
) -> Result<Product, ServiceError> {
    let conn = &mut pool.get().unwrap();

    let new_product_id = Uuid::new_v4();
    let new_product = NewProduct {
        id: new_product_id,
        name: &item.name,
        desc: item.desc.as_deref(),
        type_: item.type_,
    };

    let product = insert_into(products::table)
        .values(&new_product)
        .get_result::<Product>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // barcode_id가 제공된 경우 바코드 생성
    if let Some(ref barcode_str) = item.barcode_id {
        let new_barcode = NewBarcode {
            id: Uuid::new_v4(),
            barcode_id: barcode_str,
            product_id: new_product_id,
        };

        insert_into(barcodes::table)
            .values(&new_barcode)
            .execute(conn)
            .map_err(|e| handler_disel_error(e))?;
    }

    Ok(product)
}

fn db_get_product_by_id(
    pool: web::Data<Pool>,
    in_product_id: Uuid,
) -> Result<ProductDetailResponse, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 제품 조회
    let product = products::table
        .find(in_product_id)
        .first::<Product>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 제품 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::product_id.eq(in_product_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(in_product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(ProductDetailResponse {
        product,
        images: image_ids,
        favorite_count,
    })
}

fn db_get_product_by_barcode(
    pool: web::Data<Pool>,
    barcode_id_str: String,
) -> Result<ProductDetailResponse, ServiceError> {
    let conn = &mut pool.get().unwrap();

    // 바코드로 제품 ID 찾기
    let barcode = barcodes::table
        .filter(barcodes::barcode_id.eq(barcode_id_str))
        .first::<Barcode>(conn)
        .map_err(|e| handler_disel_error(e))?;

    let product_id = barcode.product_id;

    // 제품 조회
    let product = products::table
        .find(product_id)
        .first::<Product>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 제품 이미지 ID들 조회
    let image_ids: Vec<Uuid> = product_images::table
        .filter(product_images::product_id.eq(product_id))
        .select(product_images::id)
        .load::<Uuid>(conn)
        .map_err(|e| handler_disel_error(e))?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(|e| handler_disel_error(e))?;

    Ok(ProductDetailResponse {
        product,
        images: image_ids,
        favorite_count,
    })
}

fn db_get_products_list(
    pool: web::Data<Pool>,
    query: ProductListQuery,
) -> Result<Vec<ProductListItem>, ServiceError> {
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
        .map_err(|e| handler_disel_error(e))?;

    // 각 제품에 대한 이미지 ID들 조회 (최대 3개)
    let mut result = Vec::new();

    for product in products_list {
        // 제품 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::product_id.eq(product.id))
            .select(product_images::id)
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(|e| handler_disel_error(e))?;

        result.push(ProductListItem {
            product,
            images: image_ids,
        });
    }

    Ok(result)
}
