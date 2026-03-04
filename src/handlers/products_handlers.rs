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
use diesel::{Connection, OptionalExtension};
use pgvector::VectorExpressionMethods;
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
pub struct AiProductRequest {
    pub image_id: Uuid,
    pub barcode_id: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    let item_inner = item.into_inner();

    // 제품 이름을 이용해 임베딩(Vector) 값 비동기 추출
    let embedding = match crate::utils::openai::get_embedding(&item_inner.name).await {
        Ok(vec) => Some(vec),
        Err(e) => {
            eprintln!("[OpenAI Embedding Error] {}", e);
            None
        }
    };

    let product = web::block(move || db_create_product(db, item_inner, embedding)).await??;
    let response = CommonResponse {
        result: true,
        data: product,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products/ai
pub async fn create_product_by_ai(
    req: HttpRequest,
    db: web::Data<Pool>,
    item: web::Json<AiProductRequest>,
) -> Result<HttpResponse, Error> {
    let user_sub = get_sub(req)?;
    let item_inner = item.into_inner();

    // 1. Rate Limiting Check
    if !crate::utils::rate_limit::check_and_increment_api_usage(&user_sub, 10) {
        let resp: CommonResponse<Option<()>> = CommonResponse {
            result: false,
            data: None,
            error: Some(CommonResponseError::ExceedMaxCount as u8),
        };
        return Ok(HttpResponse::Ok().json(resp));
    }

    // 2. Gemini Analysis
    let ai_result = match crate::utils::gemini::analyze_image_with_gemini(&item_inner.image_id.to_string()).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[Gemini Error] {}", e);
            let resp: CommonResponse<Option<()>> = CommonResponse {
                result: false,
                data: None,
                error: Some(CommonResponseError::FailedToAnalyzeImage as u8),
            };
            return Ok(HttpResponse::Ok().json(resp));
        }
    };

    // 3. Category Mapping
    let tags_str = ai_result.category.to_lowercase();
    let type_ = if tags_str.contains("whisky") || tags_str.contains("whiskies") { 0 }
    else if tags_str.contains("wine") || tags_str.contains("wines") { 1 }
    else if tags_str.contains("beer") || tags_str.contains("beers") { 2 }
    else if tags_str.contains("soju") || tags_str.contains("sake") { 3 }
    else if tags_str.contains("liqueur") || tags_str.contains("liqueurs") || tags_str.contains("spirit") || tags_str.contains("spirits") { 4 }
    else if tags_str.contains("cocktail") || tags_str.contains("cocktails") { 5 }
    else if tags_str.contains("coffee") || tags_str.contains("coffees") { 6 }
    else if tags_str.contains("beverage") || tags_str.contains("beverages") { 7 }
    else { 8 };

    // 4. Vector Embedding
    let embedding = match crate::utils::openai::get_embedding(&ai_result.name).await {
        Ok(vec) => Some(vec),
        Err(e) => {
            eprintln!("[OpenAI Embedding Error For AI Model] {}", e);
            None
        }
    };

    // 5. DB Query & Insertion
    let create_params = CreateProductParams {
        name: ai_result.name,
        desc: Some(ai_result.description),
        type_,
        barcode_id: item_inner.barcode_id,
        image_id: None,
    };

    let db_clone = db.clone();
    let product = web::block(move || db_create_product_by_ai(db_clone, create_params, embedding)).await??;

    // 6. Download Representative Image (if provided by Gemini)
    if let Some(ref image_url) = ai_result.image_url {
        if !image_url.is_empty() {
            let new_uuid = uuid::Uuid::new_v4();
            match crate::utils::scraper::download_image(image_url, new_uuid).await {
                Ok(_) => {
                    if let Ok(mut conn) = db.get() {
                        use diesel::prelude::*;
                        let new_image = crate::models::NewProductImage {
                            id: new_uuid,
                            product_id: Some(product.id),
                            note_id: None,
                            user_id: None,
                            registered: chrono::Utc::now(),
                        };
                        let _ = diesel::insert_into(crate::schema::product_images::table)
                            .values(&new_image)
                            .execute(&mut conn);
                    }
                }
                Err(e) => {
                    eprintln!("[Gemini Image Download Error] {}", e);
                }
            }
        }
    }

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
    let barcode_str = in_barcode_id.into_inner();
    let db_clone = db.clone();
    let bc_clone = barcode_str.clone();

    let product_detail_result =
        web::block(move || db_get_product_by_barcode(db_clone, bc_clone)).await?;

    match product_detail_result {
        Ok(detail) => {
            let response = CommonResponse {
                result: true,
                data: detail,
                error: None,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Err(crate::errors::CommonResponseError::RecordNotFound) => {
            // Fallback to scraping barcodelookup.com
            if let Some(scraped) = crate::utils::scraper::scrape_barcode_lookup(&barcode_str).await {
                // 1. Download image if exists
                let mut image_id = None;
                if let Some(img_url) = scraped.image_url {
                    let new_uuid = uuid::Uuid::new_v4();
                    if crate::utils::scraper::download_image(&img_url, new_uuid).await.is_ok() {
                        image_id = Some(new_uuid);
                        
                        let new_image = crate::models::NewProductImage {
                            id: new_uuid,
                            product_id: None,
                            note_id: None,
                            user_id: None,
                            registered: chrono::Utc::now(),
                        };
                        let db_clone_img = db.clone();
                        let _ = web::block(move || {
                            let conn = &mut db_clone_img.get().unwrap();
                            diesel::insert_into(crate::schema::product_images::table)
                                .values(&new_image)
                                .execute(conn)
                        }).await;
                    }
                }

                // 2. Generate vector embedding
                let embedding = crate::utils::openai::get_embedding(&scraped.name).await.ok();

                // 3. Create Product
                let create_params = CreateProductParams {
                    name: scraped.name,
                    desc: scraped.desc,
                    type_: scraped.type_,
                    image_id,
                    barcode_id: Some(barcode_str.clone()),
                };

                let db_clone_create = db.clone();
                let created_product = web::block(move || {
                    db_create_product(db_clone_create, create_params, embedding)
                }).await?;

                match created_product {
                    Ok(_prod) => {
                        // After creating, we query it again to build ProductDetailResponse
                        let db_clone_fetch = db.clone();
                        let new_detail = web::block(move || db_get_product_by_barcode(db_clone_fetch, barcode_str)).await?;
                        match new_detail {
                            Ok(detail) => {
                                let response = CommonResponse {
                                    result: true,
                                    data: detail,
                                    error: None,
                                };
                                Ok(HttpResponse::Ok().json(response))
                            }
                            Err(e) => {
                                let response: CommonResponse<Option<()>> = CommonResponse { result: false, data: None, error: Some(e as u8) };
                                Ok(HttpResponse::Ok().json(response))
                            }
                        }
                    }
                    Err(e) => {
                        let response: CommonResponse<Option<()>> = CommonResponse { result: false, data: None, error: Some(e as u8) };
                        Ok(HttpResponse::Ok().json(response))
                    }
                }
            } else {
                let response: CommonResponse<Option<()>> = CommonResponse {
                    result: false,
                    data: None,
                    error: Some(crate::errors::CommonResponseError::RecordNotFound as u8),
                };
                Ok(HttpResponse::Ok().json(response))
            }
        }
        Err(e) => {
            let response: CommonResponse<Option<()>> = CommonResponse {
                result: false,
                data: None,
                error: Some(e as u8),
            };
            Ok(HttpResponse::Ok().json(response))
        }
    }
}

/// Path: /products
pub async fn get_products_list(
    db: web::Data<Pool>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let query_inner = query.into_inner();
    
    let embedding = if let Some(ref name) = query_inner.name {
        let translated_name = crate::utils::translator::translate_to_english_if_cjk(name).await;
        match crate::utils::openai::get_embedding(&translated_name).await {
            Ok(vec) => Some(vec),
            Err(e) => {
                eprintln!("[OpenAI Embedding Error] {}", e);
                None
            }
        }
    } else {
        None
    };

    let products_list = web::block(move || db_get_products_list(db, query_inner, embedding)).await??;
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
    let query_inner = query.into_inner();
    
    let embedding = if let Some(ref name) = query_inner.name {
        let translated_name = crate::utils::translator::translate_to_english_if_cjk(name).await;
        match crate::utils::openai::get_embedding(&translated_name).await {
            Ok(vec) => Some(vec),
            Err(e) => {
                eprintln!("[OpenAI Embedding Error] {}", e);
                None
            }
        }
    } else {
        None
    };

    let products_list = web::block(move || db_get_my_favorite_products_list(db, query_inner, user_sub, embedding)).await??;
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
    item: CreateProductParams,
    embedding: Option<pgvector::Vector>,
) -> Result<Product, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 동일한 이름의 제품이 있는지 확인
    let existing_product = products::table
        .select(crate::models::PRODUCT_COLUMNS)
        .filter(products::name.eq(&item.name))
        .first::<Product>(conn)
        .optional()
        .map_err(handler_disel_error)?;

    if let Some(product) = existing_product {
        // 이미 동일한 이름의 제품이 있는 경우
        if let Some(ref barcode_str) = item.barcode_id {
            // barcode_id가 있다면 기존 제품에 새 바코드만 연결
            let new_barcode = NewBarcode {
                id: Uuid::new_v4(),
                barcode_id: barcode_str,
                product_id: product.id,
            };

            insert_into(barcodes::table)
                .values(&new_barcode)
                .execute(conn)
                .map_err(handler_disel_error)?;

            return Ok(product);
        } else {
            // barcode_id가 없다면 중복 에러 반환
            return Err(CommonResponseError::DuplicatedError);
        }
    }

    let new_product_id = Uuid::new_v4();
    let new_product = NewProduct {
        id: new_product_id,
        name: &item.name,
        desc: item.desc.as_deref(),
        type_: item.type_,
        registered: Utc::now(),
        embedding: embedding,
    };

    let product = conn.transaction::<Product, CommonResponseError, _>(|conn| {
        // product insert
        let product = insert_into(products::table)
            .values(&new_product)
            .returning(crate::models::PRODUCT_COLUMNS)
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

fn db_create_product_by_ai(
    pool: web::Data<Pool>,
    item: CreateProductParams,
    embedding: Option<pgvector::Vector>,
) -> Result<Product, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let existing_product = products::table
        .select(crate::models::PRODUCT_COLUMNS)
        .filter(products::name.eq(&item.name))
        .first::<Product>(conn)
        .optional()
        .map_err(handler_disel_error)?;

    if let Some(product) = existing_product {
        // 동일한 제품이 이미 존재 시 새로 만들지 않고 바코드만 연동
        if let Some(ref barcode_str) = item.barcode_id {
            // 바코드 ID가 이미 존재하는지 검사 (중복 시도 방지)
            let existing_barcode = barcodes::table
                .filter(barcodes::barcode_id.eq(barcode_str))
                .first::<Barcode>(conn)
                .optional()
                .map_err(handler_disel_error)?;
            
            if existing_barcode.is_none() {
                let new_barcode = NewBarcode {
                    id: Uuid::new_v4(),
                    barcode_id: barcode_str,
                    product_id: product.id,
                };
                insert_into(barcodes::table).values(&new_barcode).execute(conn).map_err(handler_disel_error)?;
            }
        }
        return Ok(product);
    }

    // 없으면 완전 신규 생성 로직 동일하게 적용
    let new_product_id = Uuid::new_v4();
    let new_product = NewProduct {
        id: new_product_id,
        name: &item.name,
        desc: item.desc.as_deref(),
        type_: item.type_,
        registered: Utc::now(),
        embedding: embedding,
    };

    let product = conn.transaction::<Product, CommonResponseError, _>(|conn| {
        let product = insert_into(products::table).values(&new_product).returning(crate::models::PRODUCT_COLUMNS).get_result::<Product>(conn)?;

        if let Some(ref barcode_str) = item.barcode_id {
            let new_barcode = NewBarcode { id: Uuid::new_v4(), barcode_id: barcode_str, product_id: new_product_id };
            insert_into(barcodes::table).values(&new_barcode).execute(conn)?;
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
        .select(crate::models::PRODUCT_COLUMNS)
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
        .select(crate::models::PRODUCT_COLUMNS)
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
    embedding: Option<pgvector::Vector>,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    // 제품 리스트 조회
    let mut products_query = products::table.into_boxed();

    // 타입 필터링
    if let Some(type_filter) = query.type_ {
        products_query = products_query.filter(products::type_.eq(type_filter));
    }

    // 이름이 있어서 벡터 검색을 수행하는 경우
    if let Some(emb) = embedding {
        // 관련된 결과만 나오도록 l2_distance 임계값(예: 0.9) 이하만 필터링
        products_query = products_query.filter(products::embedding.is_not_null());
        products_query = products_query.filter(products::embedding.l2_distance(emb.clone()).lt(0.9));
        products_query = products_query.order(products::embedding.l2_distance(emb));
    } else {
        // 일반 검색 (이름 검색어 없는 경우)
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
    embedding: Option<pgvector::Vector>,
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

    // 이름 임베딩 값이 있으면 거리 순 정렬
    if let Some(emb) = embedding {
        products_query = products_query.filter(products::embedding.is_not_null());
        products_query = products_query.filter(products::embedding.l2_distance(emb.clone()).lt(0.9));
        products_query = products_query.order(products::embedding.l2_distance(emb));
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
