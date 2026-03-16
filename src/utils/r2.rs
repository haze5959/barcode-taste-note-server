use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use crate::errors::CommonResponseError;
use std::env;
use log::error;

pub struct R2Client {
    client: Client,
    bucket: String,
}

impl R2Client {
    pub async fn new() -> Self {
        let endpoint = env::var("R2_S3_CLIENT").expect("R2_S3_CLIENT must be set");
        let access_key_id = env::var("R2_S3_ACCESS_KEY_ID").expect("R2_S3_ACCESS_KEY_ID must be set");
        let secret_access_key = env::var("R2_S3_ACCESS_KEY_SECRET").expect("R2_S3_ACCESS_KEY_SECRET must be set");
        let region = Region::new("auto");
        
        let config = aws_config::from_env()
            .region(region)
            .endpoint_url(endpoint)
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                access_key_id,
                secret_access_key,
                None,
                None,
                "static",
            ))
            .load()
            .await;

        let client = Client::new(&config);
        let bucket = "barnote".to_string(); // 요청에 따라 'images' 버킷 사용

        Self { client, bucket }
    }

    /// 이미지를 R2에 업로드합니다.
    pub async fn upload_image(&self, key: &str, body: Vec<u8>, content_type: &str) -> Result<(), CommonResponseError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body))
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| {
                error!("[R2 Upload Error] {}: {:?}", key, e);
                CommonResponseError::InternalServerError
            })?;

        Ok(())
    }

    /// R2에서 이미지를 삭제합니다.
    pub async fn delete_image(&self, key: &str) -> Result<(), CommonResponseError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!("[R2 Delete Error] {}: {:?}", key, e);
                CommonResponseError::InternalServerError
            })?;

        Ok(())
    }

    /// 이미지를 deleted/ 경로로 이동시킵니다. (Soft Delete)
    pub async fn move_to_deleted(&self, key: &str) -> Result<(), CommonResponseError> {
        let destination_key = format!("deleted/{}", key);

        // 1. 복사
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(format!("{}/{}", self.bucket, key))
            .key(&destination_key)
            .send()
            .await
            .map_err(|e| {
                error!("[R2 Copy Error] {} to {}: {:?}", key, destination_key, e);
                CommonResponseError::InternalServerError
            })?;

        // 2. 원본 삭제
        self.delete_image(key).await?;

        Ok(())
    }

    /// R2에서 이미지를 가져옵니다.
    pub async fn get_image(&self, key: &str) -> Result<Vec<u8>, CommonResponseError> {
        let output = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                eprintln!("[R2 Get Error] {}: {:?}", key, e);
                CommonResponseError::InternalServerError
            })?;

        let data = output.body.collect().await.map_err(|e| {
            eprintln!("[R2 Body Collect Error] {}: {:?}", key, e);
            CommonResponseError::InternalServerError
        })?;

        Ok(data.into_bytes().to_vec())
    }
}
