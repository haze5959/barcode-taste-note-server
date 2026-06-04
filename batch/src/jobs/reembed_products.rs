use crate::cohere;
use crate::db;
use diesel::pg::PgConnection;
use std::time::Duration;

/// 한 번의 Cohere 호출로 처리할 제품 수 (Cohere texts 최대 96개 제한).
const BATCH_SIZE: usize = 96;
/// 호출 사이 대기 시간(초). 분당 약 96건 → 무료 한도(월 1,000회) 및 레이트 리밋 보호.
const SLEEP_BETWEEN_CALLS_SECS: u64 = 60;

/// 전체 product 재임베딩 백필.
/// - embedding 이 NULL 인 product 만 대상으로 하므로 중단 후 재실행해도 이어서 처리된다.
/// - 제품명을 96개씩 묶어 Cohere Embed(search_document)를 1회 호출하고, 호출 간 60초 대기한다.
pub async fn run(conn: &mut PgConnection) {
    println!("[reembed_products] 시작 (모델: embed-multilingual-light-v3.0, float 384차원)");

    // 1. 임베딩이 비어 있는 product 조회
    let targets = match db::get_products_without_embedding(conn) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("[reembed_products] 대상 조회 실패: {}", e);
            return;
        }
    };

    // 2. 이름이 비어 있는 항목은 임베딩 불가 → 제외
    let targets: Vec<(uuid::Uuid, String)> = targets
        .into_iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .collect();

    let total = targets.len();
    if total == 0 {
        println!("[reembed_products] 재임베딩 대상이 없습니다. 종료.");
        return;
    }

    let chunk_count = total.div_ceil(BATCH_SIZE);
    println!(
        "[reembed_products] 대상 {}건, 배치 {}개(최대 {}건/호출), 예상 소요 약 {}분",
        total, chunk_count, BATCH_SIZE, chunk_count
    );

    let client = reqwest::Client::new();
    let mut done = 0usize;
    let mut failed = 0usize;

    for (chunk_index, chunk) in targets.chunks(BATCH_SIZE).enumerate() {
        // 첫 호출 외에는 호출 간 대기 (레이트 리밋 보호)
        if chunk_index > 0 {
            tokio::time::sleep(Duration::from_secs(SLEEP_BETWEEN_CALLS_SECS)).await;
        }

        let names: Vec<String> = chunk.iter().map(|(_, name)| name.trim().to_string()).collect();

        // 3. Cohere 배치 임베딩 (1회 호출)
        let embeddings = match cohere::embed_documents(&client, &names).await {
            Ok(v) => v,
            Err(e) => {
                // 실패한 청크는 NULL 로 남아 다음 실행 때 재시도된다
                eprintln!(
                    "[reembed_products] 배치 {}/{} 임베딩 실패(건너뜀): {}",
                    chunk_index + 1,
                    chunk_count,
                    e
                );
                failed += chunk.len();
                continue;
            }
        };

        if embeddings.len() != chunk.len() {
            eprintln!(
                "[reembed_products] 경고: 요청 {}건 / 응답 {}건 불일치",
                chunk.len(),
                embeddings.len()
            );
        }

        // 4. 각 product 임베딩 갱신
        for ((pid, _name), emb) in chunk.iter().zip(embeddings.into_iter()) {
            match db::update_product_embedding(conn, *pid, emb) {
                Ok(_) => done += 1,
                Err(e) => {
                    eprintln!("[reembed_products] product {} 갱신 실패: {}", pid, e);
                    failed += 1;
                }
            }
        }

        println!(
            "[reembed_products] 진행 {}/{} (배치 {}/{}, 실패 {})",
            done, total, chunk_index + 1, chunk_count, failed
        );
    }

    println!(
        "[reembed_products] 완료. 성공 {}건, 실패 {}건 (실패분은 재실행 시 재시도됩니다)",
        done, failed
    );
}
