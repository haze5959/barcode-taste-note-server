use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use pgvector::Vector;
use crate::schema::*;

// 공통
#[derive(Serialize, Deserialize, Debug)]
pub struct CommonResponse<DataT> {
    pub result: bool,
    pub data: DataT,
    pub error: Option<u8>
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = users)]
pub struct User {
    pub id: Uuid,
    pub nick_name: String,
    pub intro: Option<String>,
    pub image_id: Option<Uuid>,
    pub registered: Option<DateTime<Utc>>,
    pub premium_expire_at: Option<DateTime<Utc>>,
}

pub type UserColumns = (
    crate::schema::users::id,
    crate::schema::users::nick_name,
    crate::schema::users::intro,
    crate::schema::users::image_id,
    crate::schema::users::registered,
    crate::schema::users::premium_expire_at,
);

pub const USER_COLUMNS: UserColumns = (
    crate::schema::users::id,
    crate::schema::users::nick_name,
    crate::schema::users::intro,
    crate::schema::users::image_id,
    crate::schema::users::registered,
    crate::schema::users::premium_expire_at,
);

#[derive(Insertable, Debug)]
#[diesel(table_name = users)]
pub struct NewUser<'a> {
    pub id: Uuid,
    pub nick_name: &'a str,
    pub sub: &'a str,
    pub image_id: Option<Uuid>,
    pub registered: Option<DateTime<Utc>>,
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
    pub flavor_infos: Option<serde_json::Value>,
    pub registered: DateTime<Utc>,
    pub note_count: i32,
}

pub type ProductColumns = (
    crate::schema::products::id,
    crate::schema::products::name,
    crate::schema::products::type_,
    crate::schema::products::desc,
    crate::schema::products::rating,
    crate::schema::products::flavor_infos,
    crate::schema::products::registered,
    crate::schema::products::note_count,
);

pub const PRODUCT_COLUMNS: ProductColumns = (
    crate::schema::products::id,
    crate::schema::products::name,
    crate::schema::products::type_,
    crate::schema::products::desc,
    crate::schema::products::rating,
    crate::schema::products::flavor_infos,
    crate::schema::products::registered,
    crate::schema::products::note_count,
);

#[derive(Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = products)]
pub struct ProductLite {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: i16,
    pub rating: Option<f32>,
    pub registered: DateTime<Utc>,
    pub note_count: i32,
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
    pub embedding: Option<Vector>,
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
    pub public_scope: i16,
    pub details: Option<serde_json::Value>,
}

pub type NoteColumns = (
    crate::schema::notes::id,
    crate::schema::notes::user_id,
    crate::schema::notes::product_id,
    crate::schema::notes::body,
    crate::schema::notes::registered,
    crate::schema::notes::rating,
    crate::schema::notes::public_scope,
    crate::schema::notes::details,
);

pub const NOTE_COLUMNS: NoteColumns = (
    crate::schema::notes::id,
    crate::schema::notes::user_id,
    crate::schema::notes::product_id,
    crate::schema::notes::body,
    crate::schema::notes::registered,
    crate::schema::notes::rating,
    crate::schema::notes::public_scope,
    crate::schema::notes::details,
);

pub type NoteSimpleColumns = (
    crate::schema::notes::id,
    crate::schema::notes::user_id,
    crate::schema::notes::product_id,
    crate::schema::notes::body,
    crate::schema::notes::registered,
    crate::schema::notes::rating,
    crate::schema::notes::public_scope,
);

pub const NOTE_SIMPLE_COLUMNS: NoteSimpleColumns = (
    crate::schema::notes::id,
    crate::schema::notes::user_id,
    crate::schema::notes::product_id,
    crate::schema::notes::body,
    crate::schema::notes::registered,
    crate::schema::notes::rating,
    crate::schema::notes::public_scope,
);

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = notes)]
pub struct NoteSimple {
    pub id: Uuid,
    pub user_id: Uuid,
    pub product_id: Uuid,
    pub body: Option<String>,
    pub registered: DateTime<Utc>,
    pub rating: i16,
    pub public_scope: i16,
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
    pub public_scope: i16,
    pub details: Option<serde_json::Value>,
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

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug)]
#[diesel(table_name = reports)]
pub struct Report {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub user_id: Uuid,
    pub body: Option<String>,
    pub state: Option<i16>,
    pub reply: Option<String>,
    pub registered: Option<DateTime<Utc>>,
    #[serde(rename = "type")]
    pub type_: i16,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = reports)]
pub struct NewReport {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub user_id: Uuid,
    pub body: Option<String>,
    pub state: Option<i16>,
    pub reply: Option<String>,
    pub registered: Option<DateTime<Utc>>,
    #[diesel(column_name = type_)]
    pub type_: i16,
}

#[derive(Identifiable, Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = follows, primary_key(id))]
pub struct Follow {
    pub user_id: Uuid,
    pub following_user_id: Uuid,
    pub id: Uuid,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = follows)]
pub struct NewFollow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub following_user_id: Uuid,
}

#[derive(Queryable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = fcm_tokens)]
pub struct FcmToken {
    pub token: String,
    pub user_id: Uuid,
    pub is_active: i16,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = fcm_tokens)]
pub struct NewFcmToken<'a> {
    pub token: &'a str,
    pub user_id: Uuid,
    pub is_active: i16,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteListQuery {
    pub page: Option<i64>,
    pub per: Option<i64>,
    pub product_id: Option<Uuid>,
    pub order_by: Option<String>,
    pub ids: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoteListResponse {
    pub note: NoteSimple,
    pub product: Option<ProductLite>,
    pub user: Option<User>,
    pub image_ids: Option<Vec<Uuid>>,
    pub product_image_id: Option<Uuid>,
    pub flavors: Option<Vec<i16>>,
}