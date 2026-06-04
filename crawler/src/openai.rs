use pgvector::Vector;
use serde::{Deserialize, Serialize};
use std::env;

/// Cohere 임베딩 모델 (다국어 light, 384차원 벡터 반환 — 짧은 제품명 검색에 적합)
const COHERE_MODEL: &str = "embed-multilingual-light-v3.0";
/// Cohere Embed v2 API 엔드포인트
const COHERE_EMBED_URL: &str = "https://api.cohere.com/v2/embed";

/// Cohere Embed 요청 바디
#[derive(Serialize)]
struct CohereEmbedRequest<'a> {
    model: &'a str,
    texts: Vec<&'a str>,
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
    float: Vec<Vec<f32>>,
}

/// 저장(문서)용 임베딩 생성 — Cohere input_type=search_document.
/// 크롤러는 수집한 제품을 DB에 적재만 하므로 문서 임베딩만 생성한다.
pub async fn get_embedding(text: &str) -> Result<Vector, Box<dyn std::error::Error>> {
    let api_key = env::var("COHERE_API_KEY").map_err(|_| "COHERE_API_KEY is missing")?;

    let request_body = CohereEmbedRequest {
        model: COHERE_MODEL,
        texts: vec![text],
        input_type: "search_document",
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

    // embed-multilingual-light-v3.0 은 384차원 f32 벡터를 반환한다.
    let embedding = body
        .embeddings
        .float
        .into_iter()
        .next()
        .ok_or("Cohere 응답에 임베딩이 없습니다")?;

    Ok(Vector::from(embedding))
}
