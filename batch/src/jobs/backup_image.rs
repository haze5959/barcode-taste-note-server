use crate::r2::R2Client;
use std::path::Path;

/// R2 의 images/ 경로 prefix
const IMAGES_PREFIX: &str = "images/";

/// R2 images/ 경로의 모든 파일을 로컬 backup/ 폴더에 백업한다.
/// - 실행할 때마다 같은 backup/ 폴더가 갱신된다
///   (이미 받아둔 파일은 스킵하고 새로 추가된 파일만 다운로드)
/// - 중단 후 재실행해도 받은 데까지 건너뛰고 이어서 백업한다(멱등)
/// - 반환값: 전부 성공하면 true, 하나라도 실패하면 false (스케줄러의 종료코드/알림용)
pub async fn run() -> bool {
    let r2 = R2Client::new().await;

    let backup_dir = "backup";

    println!("=== Image Backup Job 시작 ===");
    println!("백업 위치: {}", backup_dir);

    if let Err(e) = std::fs::create_dir_all(backup_dir) {
        eprintln!("백업 폴더 생성 실패 ({}): {}", backup_dir, e);
        return false;
    }

    // 1. R2 images/ 경로 전체 key 목록 조회
    let keys = match r2.list_keys(IMAGES_PREFIX).await {
        Ok(k) => k,
        Err(e) => {
            eprintln!("R2 목록 조회 실패: {}", e);
            return false;
        }
    };

    let total = keys.len();
    if total == 0 {
        println!("백업할 이미지가 없습니다. 종료.");
        return true;
    }
    println!("총 {}개 파일 백업 시작", total);

    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    // 2. 파일별 다운로드 → 로컬 저장
    for key in keys.iter() {
        // key 에서 images/ prefix 를 떼어 상대 경로로 사용 (images/profile/{uuid} 같은 하위 경로 포함)
        // 끝이 '/' 인 key 는 폴더 마커이므로 건너뛴다
        let file_name = match key.strip_prefix(IMAGES_PREFIX) {
            Some(n) if !n.is_empty() && !n.ends_with('/') => n,
            _ => continue,
        };
        let local_path = format!("{}/{}", backup_dir, file_name);

        // 이미 백업된 파일은 스킵 (재실행 대비)
        if Path::new(&local_path).exists() {
            skipped += 1;
        } else {
            // 하위 폴더 구조(profile/ 등)는 로컬에도 동일하게 생성
            if let Some(parent) = Path::new(&local_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match r2.get_image(key).await {
                Ok(bytes) => match std::fs::write(&local_path, bytes) {
                    Ok(_) => downloaded += 1,
                    Err(e) => {
                        eprintln!("로컬 저장 실패 ({}): {}", local_path, e);
                        failed += 1;
                    }
                },
                Err(e) => {
                    eprintln!("다운로드 실패 ({}): {}", key, e);
                    failed += 1;
                }
            }
        }
    }

    println!("=== Image Backup Job 완료 ===");
    println!("백업 위치: {} (다운로드 {}, 스킵 {}, 실패 {})", backup_dir, downloaded, skipped, failed);

    failed == 0
}
