use pgvector::Vector;
use serde::{Deserialize, Serialize};
use std::env;

/// Cohere 임베딩 모델 (런타임과 동일하게 다국어 light, 384차원)
const COHERE_MODEL: &str = "embed-multilingual-light-v3.0";
/// Cohere Embed v2 API 엔드포인트
const COHERE_EMBED_URL: &str = "https://api.cohere.com/v2/embed";

/// Cohere Embed 요청 바디
#[derive(Serialize)]
struct CohereEmbedRequest<'a> {
    model: &'a str,
    texts: &'a [String],
    input_type: &'a str,
    embedding_types: Vec<&'a str>,
}

/// Cohere Embed 응답 바디 (필요한 필드만 매핑)
#[derive(Deserialize)]
struct CohereEmbedResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    float: Vec<Vec<f32>>,
}

/// 문서(저장)용 임베딩을 배치로 생성한다 — input_type=search_document.
/// texts 는 최대 96개(Cohere 제한)까지 한 번의 호출로 처리하며,
/// 입력 순서와 동일한 순서로 임베딩 벡터 목록을 반환한다.
pub async fn embed_documents(
    client: &reqwest::Client,
    texts: &[String],
) -> Result<Vec<Vector>, Box<dyn std::error::Error>> {
    let api_key = env::var("COHERE_API_KEY").map_err(|_| "COHERE_API_KEY is missing")?;

    let request_body = CohereEmbedRequest {
        model: COHERE_MODEL,
        texts,
        input_type: "search_document",
        embedding_types: vec!["float"],
    };

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

    // embed-multilingual-light-v3.0 은 항목별 384차원 f32 벡터를 반환한다.
    Ok(body.embeddings.float.into_iter().map(Vector::from).collect())
}
