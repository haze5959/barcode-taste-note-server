use serde::Serialize;
use uuid::Uuid;
use crate::schema::*;

// 공통
#[derive(Serialize)]
pub struct CommonResponse<DataT> {
    pub result: bool,
    pub data: DataT,
    pub error: Option<u8>
}

#[derive(Identifiable, Queryable, Serialize)]
#[diesel(table_name = users)]
pub struct User {
    pub id: Uuid,
    pub nick_name: String,
    pub sub: String
}

#[derive(Insertable, Debug)]
#[diesel(table_name = users)]
pub struct NewUser<'a> {
    pub id: Uuid,
    pub nick_name: &'a str,
    pub sub: &'a str
}