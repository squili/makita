// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use serenity::builder::Timestamp;
use anyhow::Result;
use serenity::http::Http;
use serenity::model::interactions::application_command::ApplicationCommandInteraction;
use serenity::model::interactions::message_component::MessageComponentInteraction;

pub fn remove_indexes<T>(vector: &mut Vec<T>, indexes: &Vec<usize>) -> Vec<T> {
    let mut offset = 0;
    let mut entries = Vec::new();
    for entry in indexes {
        entries.push(vector.remove(entry - offset));
        offset += 1;
    }
    entries
}

pub struct SqlId<T>(pub T) where T: From<u64> + Into<u64>;

pub fn default_arg<T, U: Default>(_: T) -> U { U::default() }

#[derive(Default)]
pub struct FollowupBuilder {
    title: Option<String>,
    description: Option<String>,
}

macro_rules! builder_entry {
    ($ty: ty, $name: ident) => {
        #[allow(unused)]
        pub fn $name<T: Into<$ty>>(mut self, $name: T) -> Self {
            self.$name = Some($name.into());
            self
        }
    };
}

macro_rules! build_entry {
    ($self: expr, $builder: expr, $name: ident) => {
        match $self.$name {
            Some(s) => { $builder.$name(s); }
            None => {}
        }
    };
}

impl FollowupBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn build_command(self, http: &Http, interaction: &ApplicationCommandInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    #[allow(unused)]
    pub async fn build_component(self, http: &Http, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction.create_followup_message(&http, |m|
            m.create_embed(|e| {
                build_entry!(self, e, title);
                build_entry!(self, e, description);
                e
            })
        ).await?;

        Ok(())
    }

    builder_entry!(String, title);
    builder_entry!(String, description);
}

pub macro invite_url {
    ($id: expr) => {
        format!("https://discord.com/oauth2/authorize?client_id={}&permissions=8&scope=applications.commands+bot", $id)
    }
}
