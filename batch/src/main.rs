use diesel::prelude::*;
use std::env;
use chrono::{Utc, Duration};
use std::fs;
use std::path::Path;

mod db;
mod models;
mod schema;

use schema::product_images::dsl::*;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: cargo run <command>");
        eprintln!("Available commands: clean_image");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "clean_image" => {
            println!("Starting 'clean_image' batch job...");
            clean_image_job().await;
            println!("Batch job 'clean_image' completed.");
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}

async fn clean_image_job() {
    let mut conn = db::establish_connection();

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

    // Trash 디렉토리 확인 및 생성 (서버 루트의 static/trash 기준)
    let static_dir = env::current_dir().unwrap().join("static");
    // Workspace 기준(batch 내부인지 바깥인지) 보정
    let static_dir = if static_dir.exists() {
        static_dir
    } else {
        env::current_dir().unwrap().join("..").join("static")
    };
    
    let trash_dir = static_dir.join("trash");
    let images_dir = static_dir.join("images");

    if !trash_dir.exists() {
        fs::create_dir_all(&trash_dir).expect("Failed to create trash directory");
    }

    let mut moved_count = 0;
    
    // 2. 파일 시스템 이동 처리
    for img_id in &target_images {
        let file_name = img_id.to_string();
        let source_path = images_dir.join(&file_name);
        
        if source_path.exists() {
            let target_file_name = format!("{}.jpeg", file_name);
            let target_path = trash_dir.join(&target_file_name);
            
            match fs::rename(&source_path, &target_path) {
                Ok(_) => {
                    println!("Moved {} to trash", target_file_name);
                    moved_count += 1;
                }
                Err(e) => eprintln!("Failed to move file {}: {}", file_name, e),
            }
        } else {
            println!("File {} not found in images directory, but exists in DB. Will still delete DB record.", file_name);
        }
    }

    // 3. DB 레코드 삭제
    let deleted_rows = diesel::delete(product_images.filter(id.eq_any(&target_images)))
        .execute(&mut conn)
        .expect("Error deleting records from database");

    println!("Cleanup Summary: DB records deleted: {}, Files moved to trash: {}", deleted_rows, moved_count);
}
