use actix_web::{Error, HttpMessage, HttpRequest};

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub sub: String,
    pub token: Option<String>,
}

pub fn get_auth_info(req: HttpRequest) -> Result<AuthInfo, Error> {
    let sub = req
        .extensions()
        .get::<String>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No sub in request"))?;

    let token = req.extensions().get::<RawToken>().map(|t| t.0.clone());

    Ok(AuthInfo { sub, token })
}

#[derive(Clone)]
pub struct RawToken(pub String);