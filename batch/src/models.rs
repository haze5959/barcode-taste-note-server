use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::NaiveDateTime;

#[derive(Queryable, Selectable, Insertable, Serialize, Deserialize, Debug)]
#[diesel(table_name = crate::schema::product_images)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ProductImage {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub registered: NaiveDateTime,
}
