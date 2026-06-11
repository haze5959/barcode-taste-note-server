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

    /// 여러 객체를 일괄 삭제합니다. (S3 DeleteObjects — 한 번에 최대 1,000개씩 나눠 처리)
    pub async fn delete_keys(&self, keys: &[String]) -> Result<(), CommonResponseError> {
        use aws_sdk_s3::types::{Delete, ObjectIdentifier};

        for chunk in keys.chunks(1000) {
            let objects: Vec<ObjectIdentifier> = chunk
                .iter()
                .map(|key| {
                    ObjectIdentifier::builder().key(key).build().map_err(|e| {
                        error!("[R2 Batch Delete Error] key {}: {:?}", key, e);
                        CommonResponseError::InternalServerError
                    })
                })
                .collect::<Result<_, _>>()?;

            let delete = Delete::builder().set_objects(Some(objects)).build().map_err(|e| {
                error!("[R2 Batch Delete Error] Delete build: {:?}", e);
                CommonResponseError::InternalServerError
            })?;

            self.client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(delete)
                .send()
                .await
                .map_err(|e| {
                    error!("[R2 Batch Delete Error] {:?}", e);
                    CommonResponseError::InternalServerError
                })?;
        }

        Ok(())
    }

    /// 지정한 prefix 하위의 모든 객체 key 목록을 반환합니다.
    /// (한 번에 최대 1,000개씩 내려오므로 continuation token으로 전체 페이지를 순회한다)
    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, CommonResponseError> {
        let mut keys = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self.client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(token) = &continuation_token {
                request = request.continuation_token(token);
            }

            let output = request.send().await.map_err(|e| {
                error!("[R2 List Error] {}: {:?}", prefix, e);
                CommonResponseError::InternalServerError
            })?;

            if let Some(contents) = output.contents {
                keys.extend(contents.into_iter().filter_map(|obj| obj.key));
            }

            continuation_token = output.next_continuation_token;
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(keys)
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
