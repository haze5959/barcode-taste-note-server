use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::schema::*;

// 공통
#[derive(Serialize, Deserialize, Debug)]
pub struct CommonResponse<DataT> {
    pub result: bool,
    pub data: DataT,
    pub error: Option<u8>
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = users)]
pub struct User {
    pub id: Uuid,
    pub nick_name: String,
    pub intro: Option<String>,
    pub image_id: Option<Uuid>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = users)]
pub struct NewUser<'a> {
    pub id: Uuid,
    pub nick_name: &'a str,
    pub sub: &'a str
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = products)]
pub struct Product {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: i16,
    pub desc: Option<String>,
    pub rating: Option<f32>,
    pub flavors: Option<serde_json::Value>,
    pub registered: DateTime<Utc>,
    pub note_count: i32,
}

#[derive(Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = products)]
pub struct ProductLite {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: i16,
    pub rating: Option<f32>,
    pub registered: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = products)]
pub struct NewProduct<'a> {
    pub id: Uuid,
    pub name: &'a str,
    #[diesel(column_name = type_)]
    pub type_: i16,
    pub desc: Option<&'a str>,
    pub registered: DateTime<Utc>,
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = barcodes)]
pub struct Barcode {
    pub id: Uuid,
    pub barcode_id: String,
    pub product_id: Uuid
}

#[derive(Insertable, Debug)]
#[diesel(table_name = barcodes)]
pub struct NewBarcode<'a> {
    pub id: Uuid,
    pub barcode_id: &'a str,
    pub product_id: Uuid
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = product_images)]
pub struct ProductImage {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub registered: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = product_images)]
pub struct NewProductImage {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub registered: DateTime<Utc>,
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = notes)]
pub struct Note {
    pub id: Uuid,
    pub user_id: Uuid,
    pub product_id: Uuid,
    pub body: Option<String>,
    pub registered: DateTime<Utc>,
    pub rating: i16,
    pub public_scope: i16
}

#[derive(Insertable, Debug)]
#[diesel(table_name = notes)]
pub struct NewNote {
    pub id: Uuid,
    pub user_id: Uuid,
    pub product_id: Uuid,
    pub body: Option<String>,
    pub registered: DateTime<Utc>,
    pub rating: i16,
    pub public_scope: i16
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = favorites)]
pub struct Favorite {
    pub id: Uuid,
    pub product_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = favorites)]
pub struct NewFavorite {
    pub id: Uuid,
    pub product_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = flavor_tags)]
pub struct FlavorTag {
    pub id: Uuid,
    pub flavor: i16,
    pub product_id: Uuid,
    pub note_id: Uuid,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = flavor_tags)]
pub struct NewFlavorTag {
    pub id: Uuid,
    pub flavor: i16,
    pub product_id: Uuid,
    pub note_id: Uuid,
}