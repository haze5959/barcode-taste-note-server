use crate::db::{
    get_product_id_and_details_by_barcode, insert_barcode, insert_product, insert_product_image,
    product_exists_by_name, update_product_info, NewBarcode, NewProduct, NewProductImage,
};
use crate::r2::R2Client;
use chrono::Utc;
use diesel::pg::PgConnection;
use serde::Deserialize;
use uuid::Uuid;

/// new_product.json에서 읽어올 아이템 구조
#[derive(Debug, Deserialize)]
pub struct ProductJsonItem {
    pub barcode: String,
    pub product_name: String,
    pub desc: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub image_url: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// new_product.json 경로 (batch 폴더 내 data 디렉토리)
const JSON_FILE_PATH: &str = "data/new_product.json";

pub async fn run(conn: &mut PgConnection, r2: &R2Client) {
    println!("[add_product_with_json] 시작");

    // JSON 파일 읽기
    let json_str = match std::fs::read_to_string(JSON_FILE_PATH) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[add_product_with_json] JSON 파일 읽기 실패: {} (경로: {})", e, JSON_FILE_PATH);
            return;
        }
    };

    let items: Vec<ProductJsonItem> = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[add_product_with_json] JSON 파싱 실패: {}", e);
            return;
        }
    };

    println!("[add_product_with_json] 총 {}개 항목 처리 시작", items.len());

    let client = reqwest::Client::new();

    for item in &items {
        let cleaned_name = item.product_name.trim().to_string();
        if cleaned_name.is_empty() {
            println!("추가 실패: 이름이 비어 있음");
            continue;
        }

        // 바코드 중복 확인
        if let Some((pid, details)) = get_product_id_and_details_by_barcode(conn, &item.barcode) {
            if details.is_none() {
                // 정보 갱신
                let type_ = parse_category(&item.type_);
                let desc = if item.desc.is_empty() { None } else { Some(item.desc.as_str()) };
                
                if let Err(e) = update_product_info(conn, pid, desc, type_, item.details.clone()) {
                    println!("{} 정보 갱신 실패: {}", cleaned_name, e);
                } else {
                    println!("{} 정보 갱신", cleaned_name);
                }

                // 이미지 처리
                if let Some(ref url) = item.image_url {
                    let image_id = Uuid::new_v4();
                    match download_and_upload_image(&client, r2, url, image_id).await {
                        Ok(_) => {
                            let new_image = NewProductImage {
                                id: image_id,
                                product_id: Some(pid),
                                registered: Utc::now().naive_utc(),
                            };
                            if let Err(e) = insert_product_image(conn, &new_image) {
                                println!("{} 이미지 DB 저장 실패: {}", cleaned_name, e);
                            }
                        }
                        Err(e) => println!("{} 이미지 업로드 실패: {}", cleaned_name, e),
                    }
                }
            } else {
                println!("{} 이미 존재 (바코드: {})", cleaned_name, item.barcode);
            }
            continue;
        }

        // product_id 결정 (동일 이름 기존 제품 또는 신규 생성)
        let product_id = if let Some(existing_pid) = product_exists_by_name(conn, &cleaned_name) {
            // 이미 동일한 제품명 존재 → 바코드만 연결
            println!("{} 이미 존재 (동일 제품명, 바코드만 추가)", cleaned_name);
            existing_pid
        } else {
            // 신규 제품 생성
            let type_ = parse_category(&item.type_);
            let desc = if item.desc.is_empty() { None } else { Some(item.desc.as_str()) };

            let pid = Uuid::new_v4();
            let new_product = NewProduct {
                id: pid,
                name: &cleaned_name,
                desc,
                type_,
                registered: Utc::now(),
                embedding: None, // 배치에서는 임베딩 생략
                details: item.details.clone(),
                is_verified: true,
            };

            if let Err(e) = insert_product(conn, &new_product) {
                println!("{} 추가 실패: products 테이블 insert 오류 - {}", cleaned_name, e);
                continue;
            }

            // 이미지 다운로드 및 업로드
            if let Some(ref url) = item.image_url {
                let image_id = Uuid::new_v4();
                match download_and_upload_image(&client, r2, url, image_id).await {
                    Ok(_) => {
                        let new_image = NewProductImage {
                            id: image_id,
                            product_id: Some(pid),
                            registered: Utc::now().naive_utc(),
                        };
                        if let Err(e) = insert_product_image(conn, &new_image) {
                            println!("{} 이미지 DB 저장 실패: {}", cleaned_name, e);
                        }
                    }
                    Err(e) => {
                        println!("{} 이미지 업로드 실패: {}", cleaned_name, e);
                    }
                }
            }

            pid
        };

        // 바코드 추가
        let new_barcode = NewBarcode {
            id: Uuid::new_v4(),
            barcode_id: &item.barcode,
            product_id,
        };

        match insert_barcode(conn, &new_barcode) {
            Ok(_) => println!("{} 추가 성공 (바코드: {})", cleaned_name, item.barcode),
            Err(e) => println!("{} 추가 실패: 바코드 insert 오류 - {}", cleaned_name, e),
        }
    }

    println!("[add_product_with_json] 완료");
}

/// URL에서 이미지를 다운로드하여 R2에 업로드
async fn download_and_upload_image(
    client: &reqwest::Client,
    r2: &R2Client,
    url: &str,
    image_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("이미지 다운로드 HTTP 오류: {}", resp.status()).into());
    }
    let bytes = resp.bytes().await?;
    let key = format!("images/{}", image_id);
    r2.upload_image(&key, bytes.to_vec(), "image/jpeg").await?;
    Ok(())
}

/// type 문자열로 카테고리 int 반환
fn parse_category(type_str: &str) -> i16 {
    let lower = type_str.to_lowercase();
    if lower.contains("wine") { return 0; }
    if lower.contains("whisky") || lower.contains("whiskey") || lower.contains("whiskies") { return 1; }
    if lower.contains("beer") { return 2; }
    if lower.contains("soju") || lower.contains("sake") { return 3; }
    if lower.contains("liqueur") || lower.contains("liquor") || lower.contains("spirit") { return 4; }
    if lower.contains("beverage") { return 7; }
    8
}