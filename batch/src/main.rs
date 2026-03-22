use diesel::prelude::*;
use std::env;
use chrono::{Utc, Duration};

mod db;
mod models;
mod schema;
mod r2;
mod jobs;

use schema::product_images::dsl::*;
use r2::R2Client;
use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: cargo run <command>");
        eprintln!("Available commands: clean_image, add_product_with_json");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "clean_image" => {
            println!("Starting 'clean_image' batch job...");
            clean_image_job().await;
            println!("Batch job 'clean_image' completed.");
        }
        "add_product_with_json" => {
            println!("Starting 'add_product_with_json' batch job...");
            let mut conn = db::establish_connection();
            let r2 = R2Client::new().await;
            jobs::add_product_with_json::run(&mut conn, &r2).await;
            println!("Batch job 'add_product_with_json' completed.");
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}

async fn clean_image_job() {
    let mut conn = db::establish_connection();
    let r2 = R2Client::new().await;

    // 1시간 전 시간 계산
    let one_hour_ago = Utc::now().naive_utc() - Duration::hours(1);

    // 1. DB에서 조건에 맞는 이미지 ID 목록 추출
    // 조건: product_id IS NULL AND note_id IS NULL AND registered < 1시간 전
    let target_images = product_images
        .filter(product_id.is_null())
        .filter(note_id.is_null())
        .filter(registered.lt(one_hour_ago))
        .select(id)
        .load::<uuid::Uuid>(&mut conn)
        .expect("Error loading target images from database");

    if target_images.is_empty() {
        println!("No orphaned images found to clean.");
        return;
    }

    println!("Found {} orphaned image(s). Proceeding to clean up...", target_images.len());

    let mut moved_count = 0;
    
    // 2. R2 이동 처리
    for img_id in &target_images {
        let key = format!("images/{}", img_id);
        
        match r2.move_to_deleted(&key).await {
            Ok(_) => {
                println!("Moved {} to deleted/", key);
                moved_count += 1;
            }
            Err(e) => {
                // R2에 파일이 없을 수도 있으므로 (DB엔 있지만) 에러 로그만 남기고 진행
                eprintln!("Failed to move R2 object {}: {}", key, e);
            }
        }
    }

    // 3. DB 레코드 삭제
    let deleted_rows = diesel::delete(product_images.filter(id.eq_any(&target_images)))
        .execute(&mut conn)
        .expect("Error deleting records from database");

    println!("Cleanup Summary: DB records deleted: {}, Files moved to trash: {}", deleted_rows, moved_count);
}
