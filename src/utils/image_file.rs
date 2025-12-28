use crate::constants::{IMAGE_DIR, DELETED_DIR};
use actix_web::Error;
use uuid::Uuid;

pub fn move_image_to_deleted(image_id: Uuid) -> Result<(), Error> {
    // deleted 폴더 생성 (없으면)
    std::fs::create_dir_all(DELETED_DIR).map_err(|e| {
        eprintln!("Failed to create deleted directory: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to create deleted directory")
    })?;

    let source_path = format!("{}/{}.jpeg", IMAGE_DIR, image_id);
    let dest_path = format!("{}/{}.jpeg", DELETED_DIR, image_id);

    // 파일이 존재하는 경우에만 이동
    if std::path::Path::new(&source_path).exists() {
        std::fs::rename(&source_path, &dest_path).map_err(|e| {
            eprintln!("Failed to move image to deleted folder: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to move image")
        })?;
    }

    Ok(())
}
