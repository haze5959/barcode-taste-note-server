use actix_web::{Error, HttpMessage, HttpRequest};

pub fn get_sub(req: HttpRequest) -> Result<String, Error> {
    let user_sub = req
        .extensions()
        .get::<String>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No sub in request"))?;

    return Ok(user_sub);
}