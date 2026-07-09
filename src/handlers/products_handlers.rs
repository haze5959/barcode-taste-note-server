use crate::Pool;
use crate::diesel::QueryDsl;
use crate::diesel::RunQueryDsl;
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::models::{Barcode, CommonResponse, NewBarcode, NewProduct, Product, ProductLite, NewFavorite};
use crate::schema::{barcodes, favorites, product_images, products};
use crate::utils::auth::{get_auth_info, AuthInfo};
use crate::utils::db::get_user_id_by_sub;
use crate::utils::r2::R2Client;
use crate::handlers::users_handler::register_user;
use chrono::Utc;
use actix_web::{Error, HttpRequest, HttpResponse, web};
use diesel::dsl::{count, insert_into, delete};
use diesel::expression_methods::*;
use diesel::{Connection, OptionalExtension};
use pgvector::VectorExpressionMethods;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use log::error;

/// 제품명 벡터 검색의 L2 거리 임계값 (이 값 미만이면 유사 제품으로 간주).
///
/// Cohere embed-multilingual-v3.0(1024차원, 정규화 벡터)으로 실측한 거리 분포
/// (영문↔영문 + 한글 검색어→영문 제품명 교차언어 포함):
///   - 매칭(동일 제품, 교차언어 포함):  L2 0.78 ~ 1.04  (예: '하이네켄'→'Heineken' 0.99, '기네스'→'Guinness' 1.04)
///   - 비매칭(서로 다른 제품):          L2 1.08 ~ 1.21  (주류끼리 의미가 가까운 0.99 예외 1건)
/// 교차언어 매칭(최대 1.04)까지 포함하고 명확한 비매칭(≥1.075)은 제외하도록 1.07을 컷오프로 사용한다.
/// LIKE 실패 시의 폴백 검색이라 재현율을 우선하며, 결과는 거리순 정렬이므로 근소한 오탐은 하위에 노출된다.
/// 운영 데이터 분포에 따라 추후 보정 가능.
const PRODUCT_SEARCH_L2_THRESHOLD: f64 = 1.07;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProductParams {
    pub name: String,
    pub desc: Option<String>,
    #[serde(rename = "type")]
    pub type_: i16,
    pub barcode_id: Option<String>,
    pub image_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
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
    pub my_note_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProductListItem {
    pub product: ProductLite,
    pub image_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProductTastedListItem {
    pub product: ProductLite,
    pub image_ids: Vec<Uuid>,
    pub my_rating: i16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetFavoriteParams {
    pub product_id: Uuid,
    pub is_favorite: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteByUserIdQuery {
    pub user_id: Uuid,
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<i16>,
    pub order_by: Option<String>,
}

// ============================================
// MARK: Handler for POST
// ============================================

/// Path: /products
pub async fn create_product(
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    item: web::Json<CreateProductParams>,
) -> Result<HttpResponse, Error> {
    let mut item_inner = item.into_inner();

    if crate::utils::block_list::PRODUCT_BLOCK_LIST.contains(&item_inner.name.to_lowercase().as_str()) {
        let resp: CommonResponse<Option<()>> = CommonResponse {
            result: false,
            data: None,
            error: Some(CommonResponseError::BlockedProduct as u8),
        };
        return Ok(HttpResponse::Ok().json(resp));
    }

    let db_check = db.clone();
    let name_check = item_inner.name.clone();
    let barcode_check = item_inner.barcode_id.clone();

    // 동일한 이름의 제품이 이미 있으면 바코드만 연결하고 바로 반환
    if let Some(product) = web::block(move || db_check_and_attach_barcode(&mut db_check.get().unwrap(), &name_check, barcode_check.as_deref())).await?? {
        let response = CommonResponse { result: true, data: product, error: None };
        return Ok(HttpResponse::Ok().json(response));
    }

    // desc가 없거나 비어있는 경우 Gemini로 미리 채우기
    if item_inner.desc.as_ref().map_or(true, |s: &String| s.trim().is_empty()) {
        if let Some(info) = crate::utils::gemini::generate_product_info_with_gemini(&item_inner.name).await {
            item_inner.desc = Some(info.description);
            item_inner.details = info.details;
        }
    }

    // image_id가 없는 경우 Google 이미지 검색으로 자동 채우기
    if item_inner.image_id.is_none() {
        if let Some(img_url) = crate::utils::scraper::search_duckduckgo_image_url(&item_inner.name).await {
            let new_uuid = Uuid::new_v4();
            if crate::utils::scraper::download_image(&r2, &img_url, new_uuid).await.is_ok() {
                let new_image = crate::models::NewProductImage {
                    id: new_uuid,
                    product_id: None,
                    note_id: None,
                    user_id: None,
                    registered: chrono::Utc::now(),
                    public_scope: None,
                };
                let db_clone_img = db.clone();
                let _ = web::block(move || {
                    let conn = &mut db_clone_img.get().unwrap();
                    diesel::insert_into(crate::schema::product_images::table)
                        .values(&new_image)
                        .execute(conn)
                }).await;
                item_inner.image_id = Some(new_uuid);
            }
        }
    }

    // 제품 이름을 이용해 임베딩(Vector) 값 비동기 추출
    let embedding = match crate::utils::openai::get_embedding(&item_inner.name).await {
        Ok(vec) => Some(vec),
        Err(e) => {
            error!("[Cohere Embedding Error] {}", e);
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
    r2: web::Data<R2Client>,
    item: web::Json<AiProductRequest>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let user_sub = auth_info.sub;
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
    let ai_result = match crate::utils::gemini::analyze_image_with_gemini(&r2, &item_inner.image_id.to_string()).await {
        Ok(res) => res,
        Err(e) => {
            error!("[Gemini Error] {}", e);
            let resp: CommonResponse<Option<()>> = CommonResponse {
                result: false,
                data: None,
                error: Some(CommonResponseError::FailedToAnalyzeImage as u8),
            };
            return Ok(HttpResponse::Ok().json(resp));
        }
    };

    // 3. Category Mapping
    let type_ = crate::utils::scraper::parse_category(&ai_result.category);

    // 4. Vector Embedding
    let embedding = match crate::utils::openai::get_embedding(&ai_result.name).await {
        Ok(vec) => Some(vec),
        Err(e) => {
            error!("[Cohere Embedding Error For AI Model] {}", e);
            None
        }
    };

    // 5. DuckDuckGo 이미지 검색으로 대표 이미지 교체
    //    - 성공 시: 새 이미지를 R2에 업로드 & DB insert → 기존 이미지(item_inner.image_id) R2+DB 삭제
    //    - 실패 시: 기존 item_inner.image_id 그대로 사용
    let final_image_id = if let Some(img_url) = crate::utils::scraper::search_duckduckgo_image_url(&ai_result.name).await {
        let new_uuid = Uuid::new_v4();
        if crate::utils::scraper::download_image(&r2, &img_url, new_uuid).await.is_ok() {
            let new_image = crate::models::NewProductImage {
                id: new_uuid,
                product_id: None,
                note_id: None,
                user_id: None,
                registered: chrono::Utc::now(),
                public_scope: None,
            };
            let db_clone_img = db.clone();
            let insert_ok = web::block(move || {
                let conn = &mut db_clone_img.get().unwrap();
                diesel::insert_into(crate::schema::product_images::table)
                    .values(&new_image)
                    .execute(conn)
            }).await.is_ok();

            if insert_ok {
                // 기존 이미지(Gemini 분석용으로 업로드한 이미지) R2+DB에서 제거
                let old_image_id = item_inner.image_id;
                let db_clone_del = db.clone();
                let r2_clone_del = r2.clone();
                let _ = web::block(move || {
                    let rt = actix_rt::Runtime::new().unwrap();
                    let _ = rt.block_on(r2_clone_del.move_to_deleted(&format!("images/{}", old_image_id)));
                    let conn = &mut db_clone_del.get().unwrap();
                    delete(crate::schema::product_images::table.find(old_image_id)).execute(conn)
                }).await;

                new_uuid
            } else {
                item_inner.image_id
            }
        } else {
            item_inner.image_id
        }
    } else {
        item_inner.image_id
    };

    // 6. DB Query & Insertion
    let create_params = CreateProductParams {
        name: ai_result.name,
        desc: Some(ai_result.description),
        type_,
        barcode_id: item_inner.barcode_id,
        image_id: Some(final_image_id),
        details: ai_result.details,
    };

    let db_clone = db.clone();
    let product = web::block(move || db_create_product_by_ai(db_clone, create_params, embedding)).await??;

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
        web::block(move || db_get_product_by_id(db, in_product_id.into_inner(), None)).await??;
    let response = CommonResponse {
        result: true,
        data: product_detail,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /api/products/{id} (Authenticated)
pub async fn get_product_by_id_with_auth(
    req: HttpRequest,
    db: web::Data<Pool>,
    in_product_id: web::Path<Uuid>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let user_sub = auth_info.sub;
    let product_detail =
        web::block(move || db_get_product_by_id(db, in_product_id.into_inner(), Some(user_sub))).await??;
    let response = CommonResponse {
        result: true,
        data: product_detail,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BarcodeQuery {
    pub skip_record: Option<bool>,
}

/// Path: /products/barcode/{barcode_id}
pub async fn get_product_by_barcode(
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    in_barcode_id: web::Path<String>,
    query: web::Query<BarcodeQuery>,
) -> Result<HttpResponse, Error> {
    let skip_record = query.into_inner().skip_record.unwrap_or(false);
    process_get_product_by_barcode(db, r2, in_barcode_id.into_inner(), None, skip_record).await
}

/// Path: /api/products/barcode/{barcode_id} (Authenticated)
pub async fn get_product_by_barcode_with_auth(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    in_barcode_id: web::Path<String>,
    query: web::Query<BarcodeQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let user_sub = auth_info.sub;
    let skip_record = query.into_inner().skip_record.unwrap_or(false);
    process_get_product_by_barcode(db, r2, in_barcode_id.into_inner(), Some(user_sub), skip_record).await
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AutocompleteQuery {
    pub search: String,
    #[serde(rename = "type")]
    pub type_: Option<i16>,
}

/// Path: /products/autocomplete
pub async fn get_products_autocomplete(
    db: web::Data<Pool>,
    query: web::Query<AutocompleteQuery>,
) -> Result<HttpResponse, Error> {
    let query_inner = query.into_inner();
    let search = query_inner.search.trim().to_string();
    let type_filter = query_inner.type_;

    if search.is_empty() {
        let response: CommonResponse<Vec<String>> = CommonResponse { result: true, data: vec![], error: None };
        return Ok(HttpResponse::Ok().json(response));
    }

    // 1단계: LIKE 검색
    let search_clone = search.clone();
    let db_clone = db.clone();
    let results = web::block(move || db_autocomplete_by_like(db_clone, &search_clone, type_filter)).await??;
    if !results.is_empty() {
        let response = CommonResponse { result: true, data: results, error: None };
        return Ok(HttpResponse::Ok().json(response));
    }

    // 2단계: LIKE 결과 없음 → 이름 임베딩 유사도 검색 (search_query)
    let embedding = match crate::utils::openai::get_query_embedding(&search).await {
        Ok(vec) => vec,
        Err(e) => {
            error!("[Cohere Embedding Error] {}", e);
            let response: CommonResponse<Vec<String>> = CommonResponse { result: true, data: vec![], error: None };
            return Ok(HttpResponse::Ok().json(response));
        }
    };
    let results = web::block(move || db_autocomplete_by_embedding(db, embedding, type_filter)).await??;
    let response = CommonResponse { result: true, data: results, error: None };
    Ok(HttpResponse::Ok().json(response))
}

fn db_autocomplete_by_like(
    db: web::Data<Pool>,
    search: &str,
    type_filter: Option<i16>,
) -> Result<Vec<String>, CommonResponseError> {
    let conn = &mut db.get().unwrap();
    let like_pattern = format!("%{}%", search.to_lowercase());

    let mut query = products::table
        .into_boxed()
        .filter(
            diesel::dsl::sql::<diesel::sql_types::Bool>("LOWER(name) LIKE ")
                .bind::<diesel::sql_types::Text, _>(like_pattern)
        )
        .order((products::note_count.desc(), products::rating.desc().nulls_last()))
        .limit(10)
        .select(products::name);

    if let Some(t) = type_filter {
        query = query.filter(products::type_.eq(t));
    }

    query.load::<String>(conn).map_err(handler_disel_error)
}

/// 자동완성 벡터 검색: 이름 임베딩 L2 유사도 기준 상위 제품명 조회
fn db_autocomplete_by_embedding(
    db: web::Data<Pool>,
    embedding: pgvector::Vector,
    type_filter: Option<i16>,
) -> Result<Vec<String>, CommonResponseError> {
    let conn = &mut db.get().unwrap();

    let mut query = products::table
        .into_boxed()
        .filter(products::embedding.is_not_null())
        .filter(products::embedding.l2_distance(embedding.clone()).lt(PRODUCT_SEARCH_L2_THRESHOLD))
        .order(products::embedding.l2_distance(embedding))
        .limit(10)
        .select(products::name);

    if let Some(t) = type_filter {
        query = query.filter(products::type_.eq(t));
    }

    query.load::<String>(conn).map_err(handler_disel_error)
}

/// Path: /products
pub async fn get_products_list(
    db: web::Data<Pool>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let query_inner = query.into_inner();

    // 이름 검색어가 있는 경우: LIKE 검색 우선
    if let Some(ref name) = query_inner.name {
        let name_clone = name.clone();
        let db_clone = db.clone();
        let query_clone = ProductListQuery {
            page: query_inner.page,
            per: query_inner.per,
            name: query_inner.name.clone(),
            type_: query_inner.type_,
            order_by: query_inner.order_by.clone(),
        };

        // 1단계: LIKE 검색
        let like_results = web::block(move || db_search_products_by_like(db_clone, query_clone)).await??;
        if !like_results.is_empty() {
            let response = CommonResponse { result: true, data: like_results, error: None };
            return Ok(HttpResponse::Ok().json(response));
        }

        // 2단계: LIKE 결과 없음 → 이름 임베딩 벡터 검색
        // 검색어이므로 쿼리용 임베딩(search_query) 사용
        let embedding = match crate::utils::openai::get_query_embedding(&name_clone).await {
            Ok(vec) => vec,
            Err(e) => {
                error!("[Cohere Embedding Error] {}", e);
                let response = CommonResponse { result: true, data: Vec::<ProductListItem>::new(), error: None };
                return Ok(HttpResponse::Ok().json(response));
            }
        };
        let query_for_vec = ProductListQuery {
            page: query_inner.page,
            per: query_inner.per,
            name: query_inner.name,
            type_: query_inner.type_,
            order_by: query_inner.order_by,
        };
        let vec_results = web::block(move || db_search_products_by_vector(db, query_for_vec, embedding)).await??;
        let response = CommonResponse { result: true, data: vec_results, error: None };
        return Ok(HttpResponse::Ok().json(response));
    }

    // 이름 검색어 없는 경우: 일반 목록 조회
    let products_list = web::block(move || db_get_products_list_default(db, query_inner)).await??;
    let response = CommonResponse {
        result: true,
        data: products_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: api/products/favorite
pub async fn get_favorite_products_list(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let query_inner = query.into_inner();
    
    let embedding = if let Some(ref name) = query_inner.name {
        // 검색어이므로 쿼리용 임베딩(search_query) 사용
        match crate::utils::openai::get_query_embedding(name).await {
            Ok(vec) => Some(vec),
            Err(e) => {
                error!("[Cohere Embedding Error] {}", e);
                None
            }
        }
    } else {
        None
    };

    let products_list = web::block(move || db_get_my_favorite_products_list(db, r2, query_inner, auth_info, embedding)).await??;
    let response = CommonResponse {
        result: true,
        data: products_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /products/favorite?user_id={uuid} (인증 불필요)
pub async fn get_favorite_products_list_by_user_id(
    db: web::Data<Pool>,
    query: web::Query<FavoriteByUserIdQuery>,
) -> Result<HttpResponse, Error> {
    let query_inner = query.into_inner();
    let user_id = query_inner.user_id;
    let product_query = ProductListQuery {
        page: query_inner.page,
        per: query_inner.per,
        name: None,
        type_: query_inner.type_,
        order_by: query_inner.order_by,
    };

    let products_list = web::block(move || db_get_favorite_products_by_user_id(db, product_query, user_id, None)).await??;
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
    r2: web::Data<R2Client>,
    item: web::Json<SetFavoriteParams>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let _ = web::block(move || db_set_product_favorite(db, r2, item, auth_info)).await??;
    let response: CommonResponse<Option<()>> = CommonResponse {
        result: true,
        data: None,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// Path: /api/products/tasted (Authenticated)
pub async fn get_tasted_products_list(
    req: HttpRequest,
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: web::Query<ProductListQuery>,
) -> Result<HttpResponse, Error> {
    let auth_info = get_auth_info(req)?;
    let query_inner = query.into_inner();
    let tasted_products_list = web::block(move || db_get_tasted_products(db, r2, query_inner, auth_info)).await??;
    let response = CommonResponse {
        result: true,
        data: tasted_products_list,
        error: None,
    };
    Ok(HttpResponse::Ok().json(response))
}

// ============================================
// MARK: Internal Methods
// ============================================

// 동일 이름 제품 존재 시 바코드 연결 후 반환, 없으면 None 반환
fn db_check_and_attach_barcode(
    conn: &mut diesel::PgConnection,
    product_name: &str,
    barcode_id: Option<&str>,
) -> Result<Option<Product>, CommonResponseError> {
    let existing_product = products::table
        .select(crate::models::PRODUCT_COLUMNS)
        .filter(diesel::dsl::sql::<diesel::sql_types::Bool>("LOWER(name) = LOWER(").bind::<diesel::sql_types::Text, _>(product_name).sql(")"))
        .first::<Product>(conn)
        .optional()
        .map_err(handler_disel_error)?;

    let product = match existing_product {
        None => return Ok(None),
        Some(p) => p,
    };

    // 기존 제품 존재 — 바코드가 있다면 중복 검사 후 연결
    if let Some(barcode_str) = barcode_id {
        let already_exists = barcodes::table
            .filter(barcodes::barcode_id.eq(barcode_str))
            .first::<Barcode>(conn)
            .optional()
            .map_err(handler_disel_error)?
            .is_some();

        if !already_exists {
            let new_barcode = NewBarcode {
                id: Uuid::new_v4(),
                barcode_id: barcode_str,
                product_id: product.id,
            };
            insert_into(barcodes::table)
                .values(&new_barcode)
                .execute(conn)
                .map_err(handler_disel_error)?;
        }
    }

    Ok(Some(product))
}

fn db_create_product(
    pool: web::Data<Pool>,
    item: CreateProductParams,
    embedding: Option<pgvector::Vector>,
) -> Result<Product, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let new_product_id = Uuid::new_v4();
    let new_product = NewProduct {
        id: new_product_id,
        name: &item.name,
        desc: item.desc.as_deref(),
        type_: item.type_,
        registered: Utc::now(),
        embedding: embedding,
        details: item.details,
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

    // 동일 이름 제품 존재 시 바코드 연결 후 반환
    if let Some(product) = db_check_and_attach_barcode(conn, &item.name, item.barcode_id.as_deref())? {
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
        details: item.details,
    };

    let product = conn.transaction::<Product, CommonResponseError, _>(|conn| {
        let product = insert_into(products::table).values(&new_product).returning(crate::models::PRODUCT_COLUMNS).get_result::<Product>(conn)?;

        if let Some(ref barcode_str) = item.barcode_id {
            let new_barcode = NewBarcode { id: Uuid::new_v4(), barcode_id: barcode_str, product_id: new_product_id };
            insert_into(barcodes::table).values(&new_barcode).execute(conn)?;
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
    sub: Option<String>,
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
        .filter(product_images::public_scope.is_null().or(product_images::public_scope.eq(2)))
        .select(product_images::id)
        .order((product_images::note_id.desc(), product_images::registered.asc()))
        .limit(10)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(in_product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    // 나의 노트 ID들 조회
    let mut my_note_ids = None;
    if let Some(user_sub) = sub {
        let uid_opt = match get_user_id_by_sub(conn, &user_sub) {
            Ok(uid) => Some(uid),
            Err(CommonResponseError::RecordNotFound) => None,
            Err(e) => return Err(e),
        };

        if let Some(uid) = uid_opt {
            let note_ids: Vec<Uuid> = crate::schema::notes::table
                .filter(crate::schema::notes::product_id.eq(in_product_id))
                .filter(crate::schema::notes::user_id.eq(uid))
                .select(crate::schema::notes::id)
                .load::<Uuid>(conn)
                .map_err(handler_disel_error)?;
            my_note_ids = Some(note_ids);
        } else {
            my_note_ids = Some(Vec::new());
        }
    }

    Ok(ProductDetailResponse {
        product,
        image_ids: image_ids,
        favorite_count,
        my_note_ids,
    })
}

fn db_get_product_by_barcode(
    pool: web::Data<Pool>,
    barcode_id_str: String,
    sub: Option<String>,
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
        .filter(product_images::public_scope.is_null().or(product_images::public_scope.eq(2)))
        .select(product_images::id)
        .order((product_images::note_id.desc(), product_images::registered.asc()))
        .limit(10)
        .load::<Uuid>(conn)
        .map_err(handler_disel_error)?;

    // 좋아요 수 조회
    let favorite_count: i64 = favorites::table
        .filter(favorites::product_id.eq(product_id))
        .select(count(favorites::id))
        .first(conn)
        .map_err(handler_disel_error)?;

    // 나의 노트 ID들 조회
    let mut my_note_ids = None;
    if let Some(user_sub) = sub {
        let uid_opt = match get_user_id_by_sub(conn, &user_sub) {
            Ok(uid) => Some(uid),
            Err(CommonResponseError::RecordNotFound) => None,
            Err(e) => return Err(e),
        };

        if let Some(uid) = uid_opt {
            let note_ids: Vec<Uuid> = crate::schema::notes::table
                .filter(crate::schema::notes::product_id.eq(product_id))
                .filter(crate::schema::notes::user_id.eq(uid))
                .select(crate::schema::notes::id)
                .load::<Uuid>(conn)
                .map_err(handler_disel_error)?;
            my_note_ids = Some(note_ids);
        } else {
            my_note_ids = Some(Vec::new());
        }
    }

    Ok(ProductDetailResponse {
        product,
        image_ids: image_ids,
        favorite_count,
        my_note_ids,
    })
}

/// LIKE 검색 (LOWER 적용): 이름 포함 여부로 제품 조회
fn db_search_products_by_like(
    pool: web::Data<Pool>,
    query: ProductListQuery,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let name = query.name.unwrap_or_default();
    let like_pattern = format!("%{}%", name.to_lowercase());

    let mut like_query = products::table
        .into_boxed()
        .filter(
            diesel::dsl::sql::<diesel::sql_types::Bool>("LOWER(name) LIKE ")
                .bind::<diesel::sql_types::Text, _>(like_pattern)
        )
        .order(products::registered.desc());

    if let Some(type_filter) = query.type_ {
        like_query = like_query.filter(products::type_.eq(type_filter));
    }

    let results: Vec<ProductLite> = like_query
        .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
        .offset(offset)
        .limit(per)
        .load::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    build_product_list_items(conn, results)
}

/// 벡터 검색: l2_distance 기반 유사도 조회
fn db_search_products_by_vector(
    pool: web::Data<Pool>,
    query: ProductListQuery,
    embedding: pgvector::Vector,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let mut vec_query = products::table
        .into_boxed()
        .filter(products::embedding.is_not_null())
        .filter(products::embedding.l2_distance(embedding.clone()).lt(PRODUCT_SEARCH_L2_THRESHOLD))
        .order(products::embedding.l2_distance(embedding));

    if let Some(type_filter) = query.type_ {
        vec_query = vec_query.filter(products::type_.eq(type_filter));
    }

    let results: Vec<ProductLite> = vec_query
        .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
        .offset(offset)
        .limit(per)
        .load::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    build_product_list_items(conn, results)
}

/// 이름 검색 없는 일반 목록 조회
fn db_get_products_list_default(
    pool: web::Data<Pool>,
    query: ProductListQuery,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();
    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let mut products_query = products::table.into_boxed();

    if let Some(type_filter) = query.type_ {
        products_query = products_query.filter(products::type_.eq(type_filter));
    }

    if let Some(ref order_by) = query.order_by {
        if order_by == "rating" {
            products_query = products_query.order(products::rating.desc());
        } else {
            products_query = products_query.order(products::registered.desc());
        }
    } else {
        products_query = products_query.order(products::registered.desc());
    }

    let products_list: Vec<ProductLite> = products_query
        .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
        .offset(offset)
        .limit(per)
        .load::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    build_product_list_items(conn, products_list)
}

/// 제품 목록에 이미지 ID를 붙여 ProductListItem 벡터로 변환
fn build_product_list_items(
    conn: &mut diesel::PgConnection,
    products_list: Vec<ProductLite>,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let mut result = Vec::new();
    for product in products_list {
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::product_id.eq(product.id))
            .select(product_images::id)
            .order((product_images::note_id.desc(), product_images::registered.asc()))
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;
        result.push(ProductListItem { product, image_ids });
    }
    Ok(result)
}

/// Shared logic for getting product by barcode
async fn process_get_product_by_barcode(
    db: web::Data<Pool>,
    r2: web::Data<R2Client>,
    barcode_str: String,
    sub: Option<String>,
    skip_record: bool,
) -> Result<HttpResponse, Error> {
    let db_clone = db.clone();
    let bc_clone = barcode_str.clone();
    let sub_clone = sub.clone();

    let product_detail_result =
        web::block(move || db_get_product_by_barcode(db_clone, bc_clone, sub_clone)).await?;

    match product_detail_result {
        Ok(detail) => {
            crate::utils::logger::log_barcode_request(true, &barcode_str, Some(&detail.product.name)).await;
            if !skip_record {
                crate::utils::logger::record_success_barcode(&barcode_str);
            }
            let response = CommonResponse {
                result: true,
                data: detail,
                error: None,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Err(crate::errors::CommonResponseError::RecordNotFound) => {
            // 이미 스크래핑 실패 이력이 있는 바코드면, 재시도하지 않고 실패 횟수만 +1 후 실패 응답
            if crate::utils::logger::check_and_increment_fail_barcode(&barcode_str) {
                let response: CommonResponse<Option<()>> = CommonResponse {
                    result: false,
                    data: None,
                    error: Some(crate::errors::CommonResponseError::RecordNotFound as u8),
                };
                return Ok(HttpResponse::Ok().json(response));
            }

            if let Some(scraped) = crate::utils::scraper::scrape_barcode_lookup(&barcode_str).await {
                crate::utils::logger::log_barcode_request(true, &barcode_str, Some(&scraped.name)).await;
                // 1. Download image if exists
                let mut image_id = None;
                if let Some(img_url) = scraped.image_url {
                    let new_uuid = uuid::Uuid::new_v4();
                    if crate::utils::scraper::download_image(&r2, &img_url, new_uuid).await.is_ok() {
                        image_id = Some(new_uuid);
                        let new_image = crate::models::NewProductImage {
                            id: new_uuid,
                            product_id: None,
                            note_id: None,
                            user_id: None,
                            registered: chrono::Utc::now(),
                            public_scope: None,
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
                    details: scraped.details,
                };

                let db_clone_check = db.clone();
                let name_check = create_params.name.clone();
                let barcode_check = create_params.barcode_id.clone();

                // 동일 이름 제품이 있으면 바코드만 연결하고 바로 조회
                if let Ok(Some(_)) = web::block(move || db_check_and_attach_barcode(&mut db_clone_check.get().unwrap(), &name_check, barcode_check.as_deref())).await? {
                    let db_clone_fetch = db.clone();
                    let bc_fetch1 = barcode_str.clone();
                    let sub_fetch1 = sub.clone();
                    let new_detail = web::block(move || db_get_product_by_barcode(db_clone_fetch, bc_fetch1, sub_fetch1)).await?;
                    match new_detail {
                        Ok(detail) => {
                            if !skip_record {
                                crate::utils::logger::record_success_barcode(&barcode_str);
                            }
                            let response = CommonResponse { result: true, data: detail, error: None };
                            return Ok(HttpResponse::Ok().json(response));
                        }
                        Err(e) => {
                            let response: CommonResponse<Option<()>> = CommonResponse { result: false, data: None, error: Some(e as u8) };
                            return Ok(HttpResponse::Ok().json(response));
                        }
                    }
                }

                let db_clone_create = db.clone();
                let created_product = web::block(move || {
                    db_create_product(db_clone_create, create_params, embedding)
                }).await?;

                match created_product {
                    Ok(_prod) => {
                        // After creating, query details again
                        let db_clone_fetch = db.clone();
                        let bc_fetch2 = barcode_str.clone();
                        let sub_fetch2 = sub.clone();
                        let new_detail = web::block(move || db_get_product_by_barcode(db_clone_fetch, bc_fetch2, sub_fetch2)).await?;
                        match new_detail {
                            Ok(detail) => {
                                if !skip_record {
                                    crate::utils::logger::record_success_barcode(&barcode_str);
                                }
                                let response = CommonResponse { result: true, data: detail, error: None };
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
                // 스크래핑까지 실패 → 실패 바코드 목록에 신규 추가 (다음 요청부터는 스크래핑 생략)
                if !skip_record {
                    crate::utils::logger::record_fail_barcode(&barcode_str);
                }
                crate::utils::logger::log_barcode_request(false, &barcode_str, None).await;
                let response: CommonResponse<Option<()>> = CommonResponse {
                    result: false,
                    data: None,
                    error: Some(crate::errors::CommonResponseError::RecordNotFound as u8),
                };
                Ok(HttpResponse::Ok().json(response))
            }
        }
        Err(e) => {
            let response: CommonResponse<Option<()>> = CommonResponse { result: false, data: None, error: Some(e as u8) };
            Ok(HttpResponse::Ok().json(response))
        }
    }
}

/// sub를 통해 user_id를 조회한 뒤, 공통 함수에 위임
fn db_get_my_favorite_products_list(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: ProductListQuery,
    auth_info: AuthInfo,
    embedding: Option<pgvector::Vector>,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // sub로 유저 ID 조회 (없으면 자동 등록)
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    db_get_favorite_products_by_user_id(pool, query, user_id, embedding)
}

/// user_id(UUID)를 직접 받아 즐겨찾기 제품 목록을 반환하는 공통 함수
fn db_get_favorite_products_by_user_id(
    pool: web::Data<Pool>,
    query: ProductListQuery,
    user_id: Uuid,
    embedding: Option<pgvector::Vector>,
) -> Result<Vec<ProductListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

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

    // 타입 필터링
    if let Some(type_filter) = query.type_ {
        products_query = products_query.filter(products::type_.eq(type_filter));
    }

    // 이름 임베딩 값이 있으면 거리 순 정렬
    if let Some(emb) = embedding {
        products_query = products_query.filter(products::embedding.is_not_null());
        products_query = products_query.filter(products::embedding.l2_distance(emb.clone()).lt(PRODUCT_SEARCH_L2_THRESHOLD));
        products_query = products_query.order(products::embedding.l2_distance(emb));
    }

    let products_list: Vec<ProductLite> = products_query
        .select((products::id, products::name, products::type_, products::rating, products::registered, products::note_count))
        .load::<ProductLite>(conn)
        .map_err(handler_disel_error)?;

    // 각 제품에 대한 이미지 ID들 조회 (최대 3개)
    let mut result = Vec::new();

    for product in products_list {
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::product_id.eq(product.id))
            .select(product_images::id)
            .order((product_images::note_id.desc(), product_images::registered.asc()))
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
    r2: web::Data<R2Client>,
    item: web::Json<SetFavoriteParams>,
    auth_info: AuthInfo,
) -> Result<(), CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    // 유저 ID 조회
    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
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

fn db_get_tasted_products(
    pool: web::Data<Pool>,
    r2: web::Data<R2Client>,
    query: ProductListQuery,
    auth_info: AuthInfo,
) -> Result<Vec<ProductTastedListItem>, CommonResponseError> {
    let conn = &mut pool.get().unwrap();

    let page = query.page.unwrap_or(1);
    let per = query.per.unwrap_or(10);
    let offset = (page - 1) * per;

    let user_id = match get_user_id_by_sub(conn, &auth_info.sub) {
        Ok(id) => id,
        Err(CommonResponseError::RecordNotFound) => register_user(conn, None, auth_info.clone(), r2)?.id,
        Err(e) => return Err(e),
    };

    // 2. Note와 Product를 Join하여 필터링 및 DB 상 페이징 실행 (PostgreSQL DISTINCT ON 지원 활용 불가 시 대안)
    // Diesel에서는 GROUP BY로 복잡한 최신 row 가져오기 및 Join, limit 처리가 까다로움.
    // PostgreSQL 전용 `DISTINCT ON`을 수동 SQL(raw sql)로 사용하거나 Query builder를 조합.
    let mut sql_query_str = String::from(
        "SELECT p.id, p.name, p.type as type_, p.rating as p_rating, p.registered, p.note_count, \
         n.rating as n_rating \
         FROM ( \
            SELECT DISTINCT ON (product_id) product_id, rating, registered \
            FROM notes \
            WHERE user_id = $1 \
            ORDER BY product_id, registered DESC \
         ) n \
         INNER JOIN products p ON p.id = n.product_id ",
    );

    if let Some(type_filter) = query.type_ {
        sql_query_str.push_str(&format!("WHERE p.type = {} ", type_filter));
    }

    sql_query_str.push_str(&format!(
        "ORDER BY n.registered DESC LIMIT {} OFFSET {}",
        per, offset
    ));

    #[derive(QueryableByName)]
    struct RawTastedProduct {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
        #[diesel(sql_type = diesel::sql_types::Text)]
        name: String,
        #[diesel(sql_type = diesel::sql_types::Int2)]
        type_: i16,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Float4>)]
        p_rating: Option<f32>,
        #[diesel(sql_type = diesel::sql_types::Timestamptz)]
        registered: chrono::DateTime<Utc>,
        #[diesel(sql_type = diesel::sql_types::Int4)]
        note_count: i32,
        #[diesel(sql_type = diesel::sql_types::Int2)]
        n_rating: i16,
    }

    let raw_results: Vec<RawTastedProduct> = diesel::sql_query(sql_query_str)
        .bind::<diesel::sql_types::Uuid, _>(user_id)
        .load::<RawTastedProduct>(conn)
        .map_err(handler_disel_error)?;

    let mut result = Vec::new();
    for raw in raw_results {
        let product = ProductLite {
            id: raw.id,
            name: raw.name,
            type_: raw.type_,
            rating: raw.p_rating,
            registered: raw.registered,
            note_count: raw.note_count,
        };

        // 제품 이미지 ID들 조회 (최대 3개)
        let image_ids: Vec<Uuid> = product_images::table
            .filter(product_images::product_id.eq(product.id))
            .select(product_images::id)
            .order((product_images::note_id.desc(), product_images::registered.asc()))
            .limit(3)
            .load::<Uuid>(conn)
            .map_err(handler_disel_error)?;

        result.push(ProductTastedListItem { 
            product, 
            image_ids,
            my_rating: raw.n_rating 
        });
    }

    Ok(result)
}
