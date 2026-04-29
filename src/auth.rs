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
    match validate_token(credentials.token()).await {
        Ok(claims) => {
            debug!("claims {:?}", claims);
            req.extensions_mut().insert(claims.sub);
            req.extensions_mut().insert(crate::utils::auth::RawToken(credentials.token().to_string()));
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

async fn validate_token(token: &str) -> Result<Claims, CommonResponseError> {
    let authority = std::env::var("AUTHORITY").expect("AUTHORITY must be set");
    let audience = std::env::var("AUDIENCE").expect("AUDIENCE must be set");
    
    let jwks = fetch_jwks(&format!(
        "{}{}",
        authority.as_str(),
        ".well-known/jwks.json"
    ))
    .await
    .expect("failed to fetch jwks");
    
    let validations = vec![
        Validation::Issuer(authority.clone()),
        Validation::Audience(audience),
    ];
    
    let kid = match token_kid(token) {
        Ok(Some(k)) => {
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
    
    let claims: Claims = serde_json::from_value(res.claims)
        .map_err(|e| {
            error!("✗ Failed to parse claims: {:?}", e);
            CommonResponseError::AuthValidationFail
        })?;

    Ok(claims)
}

async fn fetch_jwks(uri: &str) -> Result<JWKS, Box<dyn core::error::Error>> {
    let res = reqwest::get(uri).await?;
    let val = res.json::<JWKS>().await?;
    return Ok(val);
}
