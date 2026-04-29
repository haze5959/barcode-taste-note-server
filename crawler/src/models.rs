use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OpenFoodFactsResponse {
    pub products: Vec<OffProduct>,
}

#[derive(Debug, Deserialize)]
pub struct OffProduct {
    pub code: Option<String>,
    pub product_name: Option<String>,
    pub brands: Option<String>,
    pub image_url: Option<String>,
    pub categories_tags: Option<Vec<String>>,
}