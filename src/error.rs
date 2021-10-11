// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use std::fmt::{Display, Formatter, Debug};
use std::error::Error as StdError;
use serenity::model::id::UserId;
use serenity::model::misc::Mentionable;
use crate::modules::PermissionType;

pub enum BotError {
    Generic(String),
    Internal(u64), // used: 0-12
    GuildOnly,
    Permissions(PermissionType),
    CacheMissing,
    InvalidRequest(String),
    WrongGuild,
    NotFound(String),
    OwnerOnly(UserId),
}

impl Display for BotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BotError::Generic(s) => f.write_str(s),
            BotError::Internal(s) => write!(f, "Internal error code `{}`", s),
            BotError::GuildOnly => f.write_str("Command must be run in a server"),
            BotError::Permissions(ty) => write!(f, "Missing permission `{}`", ty.as_value()),
            BotError::CacheMissing => f.write_str("Cache failure, please try again later"),
            BotError::InvalidRequest(msg) => write!(f, "Invalid request: `{}`", msg),
            BotError::WrongGuild => f.write_str("Can't refer to data from another server"),
            BotError::NotFound(s) => write!(f, "{} not found", s),
            BotError::OwnerOnly(s) => write!(f, "{} is not in the sudoers file. This incident will be reported.", s.mention())
        }
    }
}

impl Debug for BotError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        <Self as Display>::fmt(self, f)
    }
}

impl StdError for BotError {}