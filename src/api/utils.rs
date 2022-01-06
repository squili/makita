// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use anyhow::Error;
use axum::body::BoxBody;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use serde_json::to_string;
use serenity::cache::Cache;
use serenity::http::{CacheHttp, Http};
use sqlx::PgPool;
use crate::config::Config;
use crate::modules::{AuthModule, PermissionsModule, PreviewsModule, UpdatesModule};

pub enum ApiError {
    Internal(Error),
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

#[derive(Serialize)]
struct ApiErrorResponse {
    kind: String,
    msg: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response<BoxBody> {
        let (status, kind, msg) = match self {
            ApiError::Internal(err) => (StatusCode::INTERNAL_SERVER_ERROR, s!("INTERNAL"), format!("Internal error: {}", err)),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, s!("BAD_REQUEST"), s!(msg)),
            ApiError::InvalidSession => (StatusCode::UNAUTHORIZED, s!("INVALID_SESSION"), s!("Invalid session")),
            ApiError::MissingPermission => (StatusCode::FORBIDDEN, s!("MISSING_PERMISSION"), s!("Missing permission")),
            ApiError::CacheMissing => (StatusCode::NOT_FOUND, s!("CACHE_MISSING"), s!("Object missing from cache, please try again later")),
        };

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        (status, headers, to_string(&ApiErrorResponse { kind, msg }).unwrap()).into_response()
    }
}

#[inline(always)]
pub fn serialize_response<S: Serialize>(from: S) -> (StatusCode, HeaderMap, String) {
    serialize_response_status(from, StatusCode::OK)
}

pub fn serialize_response_status<S: Serialize>(from: S, status: StatusCode) -> (StatusCode, HeaderMap, String) {
    #[derive(Serialize)]
    struct Wrapper<T: Serialize> {
        data: T
    }

    // note that we unwrap all errors here - this should not be an issue, as we use the default Serialize implementation
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    (status, headers, to_string(&Wrapper { data: from }).unwrap())
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
