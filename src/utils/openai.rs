use pgvector::Vector;
use serde::{Deserialize, Serialize};
use std::env;

/// Cohere 임베딩 모델 (다국어, 1024차원 벡터 반환 — 한글↔영문 교차언어 검색 품질 우선)
const COHERE_MODEL: &str = "embed-multilingual-v3.0";
/// Cohere Embed v2 API 엔드포인트
const COHERE_EMBED_URL: &str = "https://api.cohere.com/v2/embed";

/// Cohere Embed 요청 바디
#[derive(Serialize)]
struct CohereEmbedRequest<'a> {
    model: &'a str,
    texts: Vec<&'a str>,
    // search_document(저장용) / search_query(검색용) 구분 — v3 모델은 필수
    input_type: &'a str,
    embedding_types: Vec<&'a str>,
}

/// Cohere Embed 응답 바디 (필요한 필드만 매핑, 나머지는 무시)
#[derive(Deserialize)]
struct CohereEmbedResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    // embedding_types=["float"] 요청 시 반환되는 부동소수 임베딩
    float: Vec<Vec<f32>>,
}

/// 저장(문서)용 임베딩 생성 — Cohere input_type=search_document.
/// 벡터 DB에 적재할 제품 텍스트 임베딩에 사용한다.
pub async fn get_embedding(text: &str) -> Result<Vector, Box<dyn std::error::Error>> {
    request_embedding(text, "search_document").await
}

/// 검색(쿼리)용 임베딩 생성 — Cohere input_type=search_query.
/// 사용자 검색어를 벡터화해 저장된 문서 임베딩과 비교(유사도 검색)할 때 사용한다.
pub async fn get_query_embedding(text: &str) -> Result<Vector, Box<dyn std::error::Error>> {
    request_embedding(text, "search_query").await
}

/// Cohere Embed v2 API 호출 공통 로직.
/// input_type 값에 따라 문서/쿼리 임베딩을 구분해서 생성한다.
async fn request_embedding(
    text: &str,
    input_type: &str,
) -> Result<Vector, Box<dyn std::error::Error>> {
    let api_key = env::var("COHERE_API_KEY").map_err(|_| "COHERE_API_KEY is missing")?;

    let request_body = CohereEmbedRequest {
        model: COHERE_MODEL,
        texts: vec![text],
        input_type,
        embedding_types: vec!["float"],
    };

    let client = reqwest::Client::new();
    let res = client
        .post(COHERE_EMBED_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request_body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Cohere Embed API 오류 ({}): {}", status, err_text).into());
    }

    let body: CohereEmbedResponse = res.json().await?;

    // embed-multilingual-v3.0 은 1024차원 f32 벡터를 반환한다.
    let embedding = body
        .embeddings
        .float
        .into_iter()
        .next()
        .ok_or("Cohere 응답에 임베딩이 없습니다")?;

    Ok(Vector::from(embedding))
}
