// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

pub use crate::prelude::*;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::{AddExtensionLayer, Router};
use axum::routing::{get, delete};
use reqwest::header::AUTHORIZATION;
use reqwest::Method;
use tower_http::cors::{any, CorsLayer};
use crate::api::utils::{ApiContext, serialize_response};
use crate::modules::updates::GIT_META;
use crate::macros::invite_url;
use crate::updates::GitMeta;

pub mod utils;
mod auth;
mod previews;

pub async fn start(ctx: Arc<ApiContext>) -> Result<(), anyhow::Error> {
    let app = Router::new()
        .route("/", get(index))
        .route("/auth/redirect", get(auth::redirect))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/session", get(auth::session))
        .route("/auth/sessions", delete(auth::clear_sessions))
        // .route("/auth/:id/viewer", get(auth::check_viewer))
        // .route("/auth/:id/editor", get(auth::check_editor))
        .route("/guilds/:id/previews", get(previews::get_previews).patch(previews::patch_previews))
        .layer(AddExtensionLayer::new(ctx.clone()))
        .layer(CorsLayer::new()
            .allow_origin(any())
            .allow_headers(vec![AUTHORIZATION])
            .allow_methods(vec![Method::GET, Method::POST])
        );

    tokio::spawn(async move {
        axum::Server::bind(&ctx.config.host_addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BuildInfo<'a> {
    Git(&'a GitMeta),
    Local {
        package: &'static str,
    }
}

#[derive(Serialize)]
struct IndexResponse<'a> {
    docs: String,
    invite: String,
    build: BuildInfo<'a>,
}

async fn index(Extension(ctx): Extension<Arc<ApiContext>>) -> impl IntoResponse {
    let response = IndexResponse {
        docs: s!("https://squili.github.io/makita-docs/"),
        invite: invite_url!(ctx.http.application_id),
        build: match &GIT_META {
            Some(s) => BuildInfo::Git(s),
            None => BuildInfo::Local {
                package: env!("CARGO_PKG_VERSION"),
            }
        },
    };

    serialize_response(response)
}
