use crate::errors::CommonResponseError;
use actix_web::HttpMessage;
use actix_web::{Error, dev::ServiceRequest};
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::extractors::bearer::{BearerAuth, Config};
use alcoholic_jwt::{JWKS, Validation, token_kid, validate};
use log::{debug, error};
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub: String,
    exp: usize,
}

pub async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    match validate_token(credentials.token()) {
        Ok(claims) => {
            debug!("claims {:?}", claims);
            req.extensions_mut().insert(claims.sub);
            Ok(req)
        }
        Err(_) => {
            let config = req
                .app_data::<Config>()
                .map(|data| data.clone())
                .unwrap_or_else(Default::default);
            error!("validator err config {:?}", config);
            Err((AuthenticationError::from(config).into(), req))
        }
    }
}

fn validate_token(token: &str) -> Result<Claims, CommonResponseError> {
    let authority = std::env::var("AUTHORITY").expect("AUTHORITY must be set");
    let audience = std::env::var("AUDIENCE").expect("AUDIENCE must be set");
    let jwks = fetch_jwks(&format!(
        "{}{}",
        authority.as_str(),
        ".well-known/jwks.json"
    ))
    .expect("failed to fetch jwks");
    let validations = vec![
        Validation::Issuer(authority),
        Validation::Audience(audience),
    ];
    // token에서 kid를 빼오지 못함 알콜홀릭 라이브러리가 이상한듯
    let kid = token_kid(token)
        .map_err(|_| CommonResponseError::JWKSFetchError)?
        .ok_or(CommonResponseError::AuthValidationFail)?;
    let jwk = jwks
        .find(&kid)
        .ok_or(CommonResponseError::AuthValidationFail)?;
    let res = validate(token, jwk, validations);
    let claims: Claims = serde_json::from_value(res.unwrap().claims).unwrap();

    Ok(claims)
}

fn fetch_jwks(uri: &str) -> Result<JWKS, Box<dyn core::error::Error>> {
    let mut res = reqwest::get(uri)?;
    let val = res.json::<JWKS>()?;
    return Ok(val);
}
