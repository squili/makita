// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::prelude::*;
use anyhow::{Error, Result};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

pub enum CustomIdType {
    ListPermissions,
}

impl Display for CustomIdType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ListPermissions => "ListPermissions",
        })
    }
}

impl CustomIdType {
    pub fn from_str(from: &str) -> Result<Self> {
        match from {
            "ListPermissions" => Ok(Self::ListPermissions),
            _ => Err(Error::new(BotError::InvalidRequest(format!(
                "Invalid CustomID type {}",
                from
            )))),
        }
    }
}

pub fn parse_custom_id(custom_id: &str) -> Result<(CustomIdType, HashMap<String, String>)> {
    let parts = custom_id.split(';').skip(1).collect::<Vec<&str>>();
    let mut map = HashMap::with_capacity(parts.len() - 1);
    for item in parts.iter().skip(1) {
        let parts = match item.split_once('=') {
            Some(parts) => parts,
            None => {
                return Err(Error::new(BotError::InvalidRequest(format!(
                    "Invalid CustomID argument {}",
                    item
                ))))
            }
        };
        let lhs = parts.0;
        let rhs = parts.1;
        map.insert(lhs.to_string(), rhs.to_string());
    }
    Ok((CustomIdType::from_str(parts.first().unwrap())?, map))
}

// make sure the custom id can never be over 100 characters!
pub fn build_custom_id(ty: &CustomIdType, map: &Option<HashMap<String, String>>) -> String {
    let mut buf = String::new();
    buf.push_str("MAK;");
    buf.extend(format!("{}", ty).chars());

    if let Some(map) = map {
        for (key, value) in map {
            buf.extend(format!(";{}={}", key, value).chars());
        }
    }

    buf
}
