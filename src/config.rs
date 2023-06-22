// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use serenity::model::id::GuildId;
use std::net::SocketAddr;

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub token: String,
    pub client_id: u64,
    pub client_secret: String,
    pub database_url: String,
    #[serde(default)]
    pub host_addr: Option<SocketAddr>,
    pub owner_id: u64,
    #[serde(default)]
    pub commands_guild: Option<GuildId>,
    #[serde(default)]
    pub github_webhook_secret: Option<String>,
}
