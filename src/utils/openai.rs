use async_openai::{Client, types::CreateEmbeddingRequestArgs};
use pgvector::Vector;

pub async fn get_embedding(text: &str) -> Result<Vector, Box<dyn std::error::Error>> {
    let client = Client::new(); // OPENAI_API_KEY from environment
    
    let request = CreateEmbeddingRequestArgs::default()
        .model("text-embedding-3-small")
        .input([text])
        .build()?;

    let response = client.embeddings().create(request).await?;
    
    // text-embedding-3-small returns 1536 dim f32 array
    let embedding_data: Vec<f32> = response.data.into_iter().flat_map(|d| d.embedding).collect();
    
    Ok(Vector::from(embedding_data))
}
