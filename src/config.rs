// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use serde::{Deserialize, Serialize};
use serenity::model::id::{GuildId, UserId};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub token: String,
    pub database_url: String,
    pub api_addr: String,
    pub owner_id: UserId,
    pub manager_guild: GuildId,
    #[serde(default)]
    pub commands_guild: Option<GuildId>,
}
