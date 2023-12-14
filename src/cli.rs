// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use crate::config::Config;
use crate::invite_url;
use anyhow::Result;
use clap::Parser;
use dialoguer::Input;
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use serenity::http::Http;
use std::fs;
use std::str::FromStr;

#[derive(Parser)]
pub struct Opts {
    #[clap(subcommand)]
    pub subcommand: Subcommand,
}

#[derive(Parser)]
pub enum Subcommand {
    Run,
    Init,
    Invite(InviteOpts),
}

#[derive(Parser)]
pub struct InviteOpts {
    #[clap(short = 'i')]
    id: Option<u64>,
}

pub fn init() -> Result<()> {
    let token = Input::new().with_prompt("Bot token").interact_text()?;

    let client_id = u64::from_str(
        &Input::<String>::new()
            .with_prompt("Client ID")
            .interact_text()?,
    )?;

    let client_secret = Input::new().with_prompt("Client secret").interact_text()?;

    let database_url = Input::new().with_prompt("Database url").interact_text()?;

    let owner_id = u64::from_str(
        &Input::<String>::new()
            .with_prompt("Owner id")
            .interact_text()?,
    )?;

    let config = Config {
        token,
        client_id,
        client_secret,
        database_url,
        host_addr: None,
        owner_id,
        commands_guild: None,
        github_webhook_secret: None,
    };

    fs::write(
        "config.ron",
        ron::ser::to_string_pretty(
            &config,
            PrettyConfig::new().extensions(Extensions::UNWRAP_NEWTYPES | Extensions::IMPLICIT_SOME),
        )?,
    )?;

    Ok(())
}

pub async fn invite(opts: InviteOpts) -> Result<()> {
    match opts.id {
        Some(id) => invite_inner(id),
        None => {
            let config: Config = ron::from_str(&fs::read_to_string("config.ron")?)?;
            invite_inner(Http::new(&config.token).get_current_user().await?.id.0)
        }
    }
}

pub fn invite_inner(id: u64) -> Result<()> {
    println!("Invite link: {}", invite_url!(id));

    Ok(())
}
