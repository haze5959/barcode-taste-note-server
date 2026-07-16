use actix_web::{Error, HttpMessage, HttpRequest, http::header};

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub sub: String,
    pub token: Option<String>,
    pub locale: String,
}

pub fn get_auth_info(req: HttpRequest) -> Result<AuthInfo, Error> {
    let sub = req
        .extensions()
        .get::<String>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No sub in request"))?;

    let token = req.extensions().get::<RawToken>().map(|t| t.0.clone());

    let locale = req
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|val| val.to_str().ok())
        .map(|s| {
            let s = s.to_lowercase();
            if s.starts_with("ko") { "ko" }
            else if s.starts_with("ja") { "ja" }
            else if s.starts_with("zh-tw") || s.starts_with("zh-hk") || s.starts_with("zh-hant") { "zh-Hant" }
            else if s.starts_with("zh") { "zh" }
            else if s.starts_with("fr") { "fr" }
            else if s.starts_with("de") { "de" }
            else if s.starts_with("es") { "es" }
            else if s.starts_with("pt") { "pt" }
            else if s.starts_with("it") { "it" }
            else if s.starts_with("ru") { "ru" }
            else { "en" }
        })
        .unwrap_or("en")
        .to_string();

    Ok(AuthInfo { sub, token, locale })
}

#[derive(Clone)]
pub struct RawToken(pub String);