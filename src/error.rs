// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::error::Error as StdError;
use std::fmt::{Debug, Display, Formatter};

pub enum BotError {
    Generic(String),
    Internal(u64), // used: 0-13
    GuildOnly,
    CacheMissing,
    InvalidRequest(String),
    WrongGuild,
    NotFound(String),
}

impl Display for BotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BotError::Generic(s) => f.write_str(s),
            BotError::Internal(s) => write!(f, "Internal error code `{}`", s),
            BotError::GuildOnly => f.write_str("Command must be run in a server"),
            BotError::CacheMissing => f.write_str("Cache failure, please try again later"),
            BotError::InvalidRequest(msg) => write!(f, "Invalid request: `{}`", msg),
            BotError::WrongGuild => f.write_str("Can't refer to data from another server"),
            BotError::NotFound(s) => write!(f, "{} not found", s),
        }
    }
}

impl Debug for BotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl StdError for BotError {}
