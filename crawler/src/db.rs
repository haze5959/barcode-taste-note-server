use crate::schema::{barcodes, products, product_images};
use diesel::prelude::*;
use diesel::pg::PgConnection;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Insertable)]
#[diesel(table_name = products)]
pub struct NewProduct<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub desc: Option<&'a str>,
    pub type_: i16,
    pub registered: DateTime<Utc>,
    pub embedding: Option<pgvector::Vector>,
    pub details: Option<serde_json::Value>,
}

#[derive(Insertable)]
#[diesel(table_name = barcodes)]
pub struct NewBarcode<'a> {
    pub id: Uuid,
    pub barcode_id: &'a str,
    pub product_id: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = product_images)]
pub struct NewProductImage {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub registered: DateTime<Utc>,
}

pub fn establish_connection() -> PgConnection {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url)
        .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
}

pub fn barcode_exists(conn: &mut PgConnection, code: &str) -> bool {
    use diesel::dsl::{select, exists};

    select(exists(barcodes::dsl::barcodes.filter(barcodes::dsl::barcode_id.eq(code))))
        .get_result(conn)
        .unwrap_or(false)
}

pub fn product_exists_by_name(conn: &mut PgConnection, product_name: &str) -> Option<Uuid> {
    products::dsl::products
        .filter(products::dsl::name.eq(product_name))
        .select(products::dsl::id)
        .first::<Uuid>(conn)
        .ok()
}

pub fn insert_product(conn: &mut PgConnection, new_product: &NewProduct) -> QueryResult<Uuid> {
    diesel::insert_into(products::table)
        .values(new_product)
        .returning(products::dsl::id)
        .get_result(conn)
}

pub fn insert_barcode(conn: &mut PgConnection, new_barcode: &NewBarcode) -> QueryResult<usize> {
    diesel::insert_into(barcodes::table)
        .values(new_barcode)
        .execute(conn)
}

pub fn insert_product_image(conn: &mut PgConnection, new_image: &NewProductImage) -> QueryResult<usize> {
    diesel::insert_into(product_images::table)
        .values(new_image)
        .execute(conn)
}
