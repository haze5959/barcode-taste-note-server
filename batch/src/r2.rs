use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use std::env;

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
        let bucket = "barnote".to_string();

        Self { client, bucket }
    }

    pub async fn upload_image(&self, key: &str, body: Vec<u8>, content_type: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body))
            .content_type(content_type)
            .send()
            .await?;

        Ok(())
    }

    pub async fn delete_image(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;

        Ok(())
    }

    /// 지정한 prefix 하위의 모든 객체 key 목록을 반환합니다.
    /// (한 번에 최대 1,000개씩 내려오므로 continuation token으로 전체 페이지를 순회한다)
    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
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

            let output = request.send().await?;

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

    /// R2에서 객체를 다운로드합니다.
    pub async fn get_image(&self, key: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let output = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;

        let data = output.body.collect().await?;
        Ok(data.into_bytes().to_vec())
    }

    pub async fn move_to_deleted(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        let destination_key = format!("deleted/{}", key);

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(format!("{}/{}", self.bucket, key))
            .key(&destination_key)
            .send()
            .await?;

        self.delete_image(key).await?;

        Ok(())
    }
}
