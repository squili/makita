use std::ffi::CString;
use std::fs;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use axum::extract::Extension;
use axum::handler::{get, Handler};
use axum::response::IntoResponse;
use axum::{AddExtensionLayer, Router};
use jsonwebtokens::{Algorithm, AlgorithmID, Verifier};
use crate::api::utils::{ApiContext, ApiError, serialize_response};
use serde::Serialize;
use serde_json::to_value;
use serenity::model::id::UserId;
use crate::config::Config;
use crate::modules::updates::{GIT_META, GitMeta};
use crate::macros::{s, invite_url};
use axum_debug::{debug_handler, debug_router};
use tower::layer::layer_fn;

pub mod utils;
mod auth;

pub async fn start(ctx: Arc<ApiContext>) -> Result<(), anyhow::Error> {
    let addr = ctx.config.host_addr;
    let app = Router::new()
        .route("/", get(index))
        .route("/auth/redirect", get(auth::redirect))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/session", get(auth::session))
        .layer(AddExtensionLayer::new(ctx))
        .check_infallible();

    debug_router!(app);

    tokio::spawn(async move {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    Ok(())
}

#[derive(Serialize)]
struct IndexResponse {
    docs: String,
    invite: String,
    #[serde(skip_serializing_if="Option::is_none")]
    tag: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    commit: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    package: Option<String>,
}

#[debug_handler]
async fn index(Extension(ctx): Extension<Arc<ApiContext>>) -> impl IntoResponse {
    let mut response = IndexResponse {
        docs: s!("https://squili.github.io/makita-docs/"),
        invite: invite_url!(ctx.http.application_id),
        tag: None,
        commit: None,
        repo: None,
        package: None,
    };
    match &GIT_META {
        Some(meta) => {
            response.tag = Some(s!(meta.tag));
            response.commit = Some(s!(meta.commit[0..7]));
            response.repo = Some(s!(meta.repo));
        },
        None => response.package = Some(s!(env!("CARGO_PKG_VERSION")))
    }

    serialize_response(response)
}
