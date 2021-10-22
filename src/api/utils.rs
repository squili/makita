use std::sync::Arc;
use anyhow::Error;
use axum::body::{Bytes, Full};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use serde::Serialize;
use serde_json::{Map, to_string, to_value, Value};
use crate::macros::s;
use axum::response::IntoResponse;
use serenity::cache::Cache;
use serenity::http::{CacheHttp, Http};
use serenity::model::id::UserId;
use sqlx::PgPool;
use crate::config::Config;
use crate::modules::{AuthModule, PermissionsModule, PreviewsModule, UpdatesModule};
use crate::utils::BotContext;

pub enum ApiError {
    Internal(Error),
    RateLimited(u64),
    BadRequest(&'static str),
    InvalidSession,
    MissingPermission,
    CacheMissing,
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        Self::Internal(err)
    }
}

impl IntoResponse for ApiError {
    type Body = Full<Bytes>;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        let mut map = Map::new();

        let (status, kind, data) = match self {
            ApiError::Internal(err) => (StatusCode::INTERNAL_SERVER_ERROR, s!("INTERNAL"), vec![(s!("info"), Value::String(s!(err)))]),
            ApiError::RateLimited(limit) => (StatusCode::TOO_MANY_REQUESTS, s!("RATELIMITED"), vec![(s!("wait"), Value::Number(limit.into()))]),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, s!("BAD_REQUEST"), vec![(s!("msg"), Value::String(s!(msg)))]),
            ApiError::InvalidSession => (StatusCode::UNAUTHORIZED, s!("INVALID_SESSION"), vec![]),
            ApiError::MissingPermission => (StatusCode::FORBIDDEN, s!("MISSING_PERMISSION"), vec![]),
            ApiError::CacheMissing => (StatusCode::NOT_FOUND, s!("CACHE_MISSING"), vec![]),
        };

        let mut error_data = Map::new();
        error_data.insert(s!("kind"), Value::String(kind));
        for (key, value) in data {
            error_data.insert(key, value);
        }
        map.insert(s!("error"), Value::Object(error_data));

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        (status, headers, to_string(&Value::Object(map)).unwrap()).into_response()
    }
}

#[inline(always)]
pub fn serialize_response<S: Serialize>(from: S) -> (StatusCode, HeaderMap, String) {
    serialize_response_status(from, StatusCode::OK)
}

pub fn serialize_response_status<S: Serialize>(from: S, status: StatusCode) -> (StatusCode, HeaderMap, String) {
    // note that we unwrap all errors here - this should not be an issue, as we
    // use both the default Serialize implementation and the official Map structure
    let mut map = Map::new();
    map.insert(s!("data"), to_value(from).unwrap());
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    (status, headers, to_string(&Value::Object(map)).unwrap())
}

#[derive(Clone)]
pub struct ApiContext {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub updates: Arc<UpdatesModule>,
    pub permissions: Arc<PermissionsModule>,
    pub previews: Arc<PreviewsModule>,
    pub auth: Arc<AuthModule>,
}

impl CacheHttp for ApiContext {
    fn http(&self) -> &Http {
        &self.http
    }

    fn cache(&self) -> Option<&Arc<Cache>> {
        Some(&self.cache)
    }
}

impl AsRef<Cache> for ApiContext {
    fn as_ref(&self) -> &Cache {
        &self.cache
    }
}

impl AsRef<Http> for ApiContext {
    fn as_ref(&self) -> &Http {
        &self.http
    }
}

pub macro api_map {
    () => {
        |err| ApiError::from(anyhow::Error::from(err))
    }
}
