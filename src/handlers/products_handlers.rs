use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{Barcode, CommonResponse, NewBarcode, NewProduct, Product, ProductLite, NewFavorite};
use crate::schema::{barcodes, favorites, product_images, products, users};
use crate::utils::auth::get_sub;
use crate::handlers::users_handler::register_user;
use chrono::Utc;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{count, insert_into, delete};
use diesel::expression_methods::*;
use diesel::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProductParams {
    pub name: String,
    pub desc: Option<String>,
    #[serde(rename = "type")]
    pub type_: i16,
    pub barcode_id: Option<String>,
    pub image_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<i16>,
    pub order_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductDetailResponse {
    pub product: Product,
    pub image_ids: Vec<Uuid>,
    pub favorite_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductListItem {
    pub product: ProductLite,
    pub image_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetFavoriteParams {
    pub product_id: Uuid,
    pub is_favorite: bool,
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
    let response = CommonResponse {
        result: true,
        data: products_list,
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
    let response = CommonResponse {
        result: true,
        data: products_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products/favorite
pub async fn set_product_favorite(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<SetFavoriteParams>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let _ = web::block(move || db_set_product_favorite(db, item, user_sub)).await??;
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

        // image_id가 제공된 경우 이미지 연결
        if let Some(image_id) = item.image_id {
            diesel::update(product_images::table.find(image_id))
                .set(product_images::product_id.eq(new_product_id))
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

    // 타입 필터링
    if let Some(type_filter) = query.type_ {
        products_query = products_query.filter(products::type_.eq(type_filter));
    }

    // 정렬
    if let Some(order_by) = query.order_by {
        if order_by == "rating" {
            products_query = products_query.order(products::rating.desc());
        } else {
            // default: registered
            products_query = products_query.order(products::registered.desc());
        }
    } else {
        // default: registered
        products_query = products_query.order(products::registered.desc());
    }

    let products_list: Vec<ProductLite> = products_query
        .select((products::id, products::name, products::type_, products::rating, products::registered))
        .offset(offset)
        .limit(per)
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
    let user_id = match users::table
        .filter(users::sub.eq(&user_sub))
        .select(users::id)
        .first::<Uuid>(conn)
    {
        Ok(id) => id,
        Err(diesel::result::Error::NotFound) => register_user(conn, None, &user_sub)?.id,
        Err(e) => return Err(handler_disel_error(e)),
    };

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

    let products_list: Vec<ProductLite> = products_query
        .select((products::id, products::name, products::type_, products::rating, products::registered))
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

fn db_set_product_favorite(
    pool: web::Data<Pool>,
    item: web::Json<SetFavoriteParams>,
    user_sub: String,
) -> Result<(), CommonResponseError> {
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

    if item.is_favorite {
        // 이미 좋아요가 있는지 확인
        let existing_favorite = favorites::table
            .filter(favorites::user_id.eq(user_id))
            .filter(favorites::product_id.eq(item.product_id))
            .count()
            .get_result::<i64>(conn)
            .map_err(handler_disel_error)?;

        if existing_favorite == 0 {
             let new_favorite = NewFavorite {
                id: Uuid::new_v4(),
                product_id: item.product_id,
                user_id: user_id,
            };

            insert_into(favorites::table)
                .values(&new_favorite)
                .execute(conn)
                .map_err(handler_disel_error)?;
        }
    } else {
        delete(favorites::table)
            .filter(favorites::user_id.eq(user_id))
            .filter(favorites::product_id.eq(item.product_id))
            .execute(conn)
            .map_err(handler_disel_error)?;
    }

    Ok(())
}
