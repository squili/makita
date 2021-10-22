// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ApplicationId, GuildId, UserId};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub token: String,
    pub client_id: ApplicationId,
    pub client_secret: String,
    pub database_url: String,
    pub host_addr: SocketAddr,
    pub api_url: String,
    pub frontend_url: String,
    pub owner_id: UserId,
    pub manager_guild: GuildId,
    #[serde(default)]
    pub commands_guild: Option<GuildId>,
}
