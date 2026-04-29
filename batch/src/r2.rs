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
