use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OpenFoodFactsResponse {
    pub count: i64,
    pub page: i64,
    pub page_count: i64,
    pub products: Vec<OffProduct>,
}

#[derive(Debug, Deserialize)]
pub struct OffProduct {
    pub code: Option<String>,
    pub product_name: Option<String>,
    pub brands: Option<String>,
    pub image_url: Option<String>,
    pub nutriments: Option<Nutriments>,
    pub categories_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Nutriments {
    pub alcohol: Option<f32>,
}
