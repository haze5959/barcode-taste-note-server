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
    debug!("===== Token Validation Started =====");
    let authority = std::env::var("AUTHORITY").expect("AUTHORITY must be set");
    let audience = std::env::var("AUDIENCE").expect("AUDIENCE must be set");
    
    let jwks = fetch_jwks(&format!(
        "{}{}",
        authority.as_str(),
        ".well-known/jwks.json"
    ))
    .expect("failed to fetch jwks");
    
    let validations = vec![
        Validation::Issuer(authority.clone()),
        Validation::Audience(audience),
    ];
    
    let kid = match token_kid(token) {
        Ok(Some(k)) => {
            debug!("✓ Kid extracted successfully: {}", k);
            k
        },
        Ok(None) => {
            error!("✗ Token does not contain 'kid' field in header");
            error!("This usually means the JWT header is missing the 'kid' field");
            return Err(CommonResponseError::AuthValidationFail);
        },
        Err(e) => {
            error!("✗ Failed to parse token header: {:?}", e);
            error!("This could mean: 1) Invalid JWT format, 2) Invalid Base64 encoding, 3) Malformed header");
            return Err(CommonResponseError::JWKSFetchError);
        }
    };
    
    let jwk = jwks
        .find(&kid)
        .ok_or_else(|| {
            error!("✗ JWK not found for kid: {}", kid);
            CommonResponseError::AuthValidationFail
        })?;
    
    let res = validate(token, jwk, validations)
        .map_err(|e| {
            error!("✗ Token validation failed: {:?}", e);
            CommonResponseError::AuthValidationFail
        })?;
    
    debug!("✓ Token validated successfully");
    
    let claims: Claims = serde_json::from_value(res.claims)
        .map_err(|e| {
            error!("✗ Failed to parse claims: {:?}", e);
            CommonResponseError::AuthValidationFail
        })?;

    debug!("✓ Claims parsed successfully");
    debug!("===== Token Validation Ended =====");
    Ok(claims)
}

fn fetch_jwks(uri: &str) -> Result<JWKS, Box<dyn core::error::Error>> {
    let mut res = reqwest::get(uri)?;
    let val = res.json::<JWKS>()?;
    return Ok(val);
}
