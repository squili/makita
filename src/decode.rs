// Copyright 2021 Mia Stoaks
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

// pretty much a clone of serenity-slash-decode, but using our git version of serenity. there's really no better way of doing this

use serenity::model::channel::PartialChannel;
use serenity::model::guild::{PartialMember, Role};
use serenity::model::interactions::application_command::{
    ApplicationCommandInteractionData, ApplicationCommandInteractionDataOptionValue,
    ApplicationCommandOptionType,
};
use serenity::model::misc::{Mention, Mentionable as SerenityMentionable};
use serenity::model::user::User;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use serenity::model::id::UserId;

#[derive(Debug)]
pub enum Error {
    WrongType {
        expected: String,
        found: String,
        name: String,
    },
    MissingValue {
        name: String,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::WrongType {
                expected,
                found,
                name,
            } => f.write_str(&*format!(
                "Wrong type in field `{}` (expected `{}`, got `{}`)",
                name, expected, found
            )),
            Error::MissingValue { name } => {
                f.write_str(&*format!("Missing value in field `{}`", name))
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Contains the values of the slash command
#[derive(Debug)]
pub struct SlashValue {
    /// The actual value
    inner: Option<ApplicationCommandInteractionDataOptionValue>,
    /// The name of the parameter; Included for error messages
    name: String,
}

/// Optionally contains a `PartialMember` so you don't need to do a cache lookup
pub enum UserOrMember {
    User(User),
    Member(User, PartialMember),
}

impl UserOrMember {
    fn from_pair(user: User, member: Option<PartialMember>) -> Self {
        match member {
            Some(m) => Self::Member(user, m),
            None => Self::User(user),
        }
    }

    /// Gets the inner user
    pub fn get_user(&self) -> &User {
        match self {
            UserOrMember::User(s) => s,
            UserOrMember::Member(u, _) => u,
        }
    }

    /// Gets the inner member, if it exists
    #[allow(unused)]
    pub fn get_member(&self) -> Option<&PartialMember> {
        match self {
            UserOrMember::User(_) => None,
            UserOrMember::Member(_, m) => Some(m),
        }
    }
    
    pub fn id(&self) -> &UserId {
        match self {
            UserOrMember::User(u) => &u.id,
            UserOrMember::Member(u, _) => &u.id,
        }
    }
}

/// Mentionables
pub enum Mentionable {
    #[allow(unused)]
    UserOrMember(UserOrMember),
    #[allow(unused)]
    Role(Role),
}

impl SerenityMentionable for Mentionable {
    fn mention(&self) -> Mention {
        match self {
            Mentionable::UserOrMember(u) => u.get_user().mention(),
            Mentionable::Role(r) => r.mention(),
        }
    }
}

impl SlashValue {
    fn get_type_name(&self) -> String {
        match self.inner.as_ref().unwrap() {
            ApplicationCommandInteractionDataOptionValue::String(_) => "String".to_string(),
            ApplicationCommandInteractionDataOptionValue::Integer(_) => "Integer".to_string(),
            ApplicationCommandInteractionDataOptionValue::Boolean(_) => "Boolean".to_string(),
            ApplicationCommandInteractionDataOptionValue::User(_, _) => "User".to_string(),
            ApplicationCommandInteractionDataOptionValue::Channel(_) => "Channel".to_string(),
            ApplicationCommandInteractionDataOptionValue::Role(_) => "Role".to_string(),
            _ => "Unknown".to_string(),
        }
    }

    /// Returns the inner value if it is `Some`
    pub fn expect_some(&self) -> Result<ApplicationCommandInteractionDataOptionValue> {
        match self.inner.clone() {
            Some(s) => Ok(s),
            None => Err(Error::MissingValue {
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `String`
    pub fn get_string(&self) -> Result<String> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::String(s) => Ok(s),
            _ => Err(Error::WrongType {
                expected: "String".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is an `Integer`
    pub fn get_integer(&self) -> Result<i64> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::Integer(s) => Ok(s),
            _ => Err(Error::WrongType {
                expected: "Integer".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `Boolean`
    #[allow(unused)]
    pub fn get_boolean(&self) -> Result<bool> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::Boolean(s) => Ok(s),
            _ => Err(Error::WrongType {
                expected: "Boolean".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `UserOrMember`
    pub fn get_user(&self) -> Result<UserOrMember> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::User(u, m) => {
                Ok(UserOrMember::from_pair(u, m))
            }
            _ => Err(Error::WrongType {
                expected: "User".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `PartialChannel`
    pub fn get_channel(&self) -> Result<PartialChannel> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::Channel(s) => Ok(s),
            _ => Err(Error::WrongType {
                expected: "Channel".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `Role`
    pub fn get_role(&self) -> Result<Role> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::Role(s) => Ok(s),
            _ => Err(Error::WrongType {
                expected: "Role".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }

    /// Returns the inner value if it is a `Mentionable`
    #[allow(unused)]
    pub fn get_mentionable(&self) -> Result<Mentionable> {
        match self.expect_some()? {
            ApplicationCommandInteractionDataOptionValue::User(u, m) => {
                Ok(Mentionable::UserOrMember(UserOrMember::from_pair(u, m)))
            }
            ApplicationCommandInteractionDataOptionValue::Role(r) => Ok(Mentionable::Role(r)),
            _ => Err(Error::WrongType {
                expected: "Mentionable".to_string(),
                found: self.get_type_name(),
                name: self.name.clone(),
            }),
        }
    }
}

/// Wrapper around `HashMap<String, SlashValue>`
pub struct SlashMap(HashMap<String, SlashValue>);

impl SlashMap {
    fn new() -> Self {
        Self(HashMap::new())
    }

    /// If `SlashMap` has value, call `SlashValue::get_string()` on it
    pub fn get_string(&self, name: &str) -> Result<String> {
        match self.0.get(name) {
            Some(s) => s.get_string(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_integer()` on it
    pub fn get_integer(&self, name: &str) -> Result<i64> {
        match self.0.get(name) {
            Some(s) => s.get_integer(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_boolean()` on it
    #[allow(unused)]
    pub fn get_boolean(&self, name: &str) -> Result<bool> {
        match self.0.get(name) {
            Some(s) => s.get_boolean(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_user()` on it
    pub fn get_user(&self, name: &str) -> Result<UserOrMember> {
        match self.0.get(name) {
            Some(s) => s.get_user(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_channel()` on it
    pub fn get_channel(&self, name: &str) -> Result<PartialChannel> {
        match self.0.get(name) {
            Some(s) => s.get_channel(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_role()` on it
    pub fn get_role(&self, name: &str) -> Result<Role> {
        match self.0.get(name) {
            Some(s) => s.get_role(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }

    /// If `SlashMap` has value, call `SlashValue::get_mentionable()` on it
    #[allow(unused)]
    pub fn get_mentionable(&self, name: &str) -> Result<Mentionable> {
        match self.0.get(name) {
            Some(s) => s.get_mentionable(),
            None => Err(Error::MissingValue {
                name: name.to_string(),
            }),
        }
    }
}

/// For derive macros
pub trait FromSlashMap {
    fn from_slash_map(_: SlashMap) -> Result<Self>
        where
            Self: Sized;
}

/// Processes a `ApplicationCommandInteractionData` and returns the path and arguments
pub fn process(interaction: &ApplicationCommandInteractionData) -> (String, SlashMap) {
    // traverse
    let mut options = &interaction.options;
    let mut path = vec![interaction.name.clone()];

    loop {
        match options.get(0) {
            None => break,
            Some(option) => {
                if matches!(
                    option.kind,
                    ApplicationCommandOptionType::SubCommand
                        | ApplicationCommandOptionType::SubCommandGroup
                ) {
                    path.push(option.name.clone());
                    options = &option.options;
                } else {
                    break;
                }
            }
        }
    }

    // map data
    let mut map = SlashMap::new();
    for option in options {
        map.0.insert(
            option.name.clone(),
            SlashValue {
                inner: option.resolved.clone(),
                name: option.name.clone(),
            },
        );
    }

    (path.join(" "), map)
}
