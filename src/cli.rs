// Copyright 2021 Squili
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::Result;
use clap::Clap;
use dialoguer::Input;
use crate::config::Config;
use std::fs;
use std::net::SocketAddr;
use std::str::FromStr;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use serenity::http::Http;
use serenity::model::id::{GuildId, UserId};
use crate::macros::invite_url;

#[derive(Clap)]
pub struct Opts {
    #[clap(subcommand)]
    pub subcommand: Subcommand,
}

#[derive(Clap)]
pub enum Subcommand {
    Run,
    Init,
    Invite(InviteOpts),
}

#[derive(Clap)]
pub struct InviteOpts {
    #[clap(short = 'i')]
    id: Option<u64>,
}

pub fn init() -> Result<()> {
    let token = Input::new()
        .with_prompt("Bot token")
        .interact_text()?;

    let database_url = Input::new()
        .with_prompt("Database url")
        .interact_text()?;

    let api_addr = SocketAddr::from_str(&Input::<String>::new()
        .with_prompt("Api address")
        .interact_text()?)?;

    let owner_id = u64::from_str(&Input::<String>::new()
        .with_prompt("Owner id")
        .interact_text()?)?;

    let manager_guild = u64::from_str(&Input::<String>::new()
        .with_prompt("Manager guild id")
        .interact_text()?)?;

    let config = Config {
        token,
        database_url,
        api_addr: api_addr.to_string(),
        owner_id: UserId(owner_id),
        manager_guild: GuildId(manager_guild),
        commands_guild: None,
    };

    fs::write("config.ron", ron::ser::to_string_pretty(&config,
        PrettyConfig::new().with_extensions(Extensions::UNWRAP_NEWTYPES | Extensions::IMPLICIT_SOME))?)?;

    // this keypair is not currently being used, but it will be used in the future for signing api tokens
    println!("Generating keypair");

    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())?;
    fs::write("key.der", document.as_ref())?;

    Ok(())
}

pub async fn invite(opts: InviteOpts) -> Result<()> {
    match opts.id {
        Some(id) => invite_inner(id),
        None => {
            let config: Config = ron::from_str(&*fs::read_to_string("config.ron")?)?;
            invite_inner(Http::new_with_token(&config.token).get_current_user().await?.id.0)
        }
    }
}

pub fn invite_inner(id: u64) -> Result<()> {
    println!("Invite link: {}", invite_url!(id));

    Ok(())
}
