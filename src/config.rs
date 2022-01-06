// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use std::net::SocketAddr;
use serenity::model::id::GuildId;

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub token: String,
    pub client_id: u64,
    pub client_secret: String,
    pub database_url: String,
    pub host_addr: SocketAddr,
    pub api_url: String,
    pub frontend_url: String,
    pub owner_id: u64,
    pub manager_guild: u64,
    #[serde(default)]
    pub commands_guild: Option<GuildId>,
}
