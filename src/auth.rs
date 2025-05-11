use crate::errors::ServiceError;
use actix_web::HttpMessage;
use actix_web::{Error, dev::ServiceRequest};
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::extractors::bearer::{BearerAuth, Config};
use alcoholic_jwt::{token_kid, validate, Validation, JWKS};
use serde_json;
use serde::{Deserialize, Serialize};
use log::{debug, error};

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
        },
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

fn validate_token(token: &str) -> Result<Claims, ServiceError> {
    let authority = std::env::var("AUTHORITY").expect("AUTHORITY must be set");
    let jwks = fetch_jwks(&format!("{}{}", authority.as_str(), ".well-known/jwks.json"))
        .expect("failed to fetch jwks");
    let validations = vec![Validation::Issuer(authority), Validation::SubjectPresent];
    let kid = match token_kid(&token) {
        Ok(res) => res.expect("failed to decode kid"),
        Err(_) => return Err(ServiceError::JWKSFetchError),
    };
    let jwk = jwks.find(&kid).expect("Specified key not found in set");
    let res = validate(token, jwk, validations);
    let claims: Claims = serde_json::from_value(res.unwrap().claims).unwrap();
    
    Ok(claims)
}

fn fetch_jwks(uri: &str) -> Result<JWKS, Box<dyn core::error::Error>> {
    let mut res = reqwest::get(uri)?;
    let val = res.json::<JWKS>()?;
    return Ok(val);
}