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
use rcgen::{PKCS_ECDSA_P256_SHA256, PKCS_ECDSA_P384_SHA384, PKCS_ED25519};
use ring::rand::SystemRandom;
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use serenity::http::Http;
use serenity::model::id::{ApplicationId, GuildId, UserId};
use crate::macros::{invite_url, s};

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

    let client_id = ApplicationId(u64::from_str(&Input::<String>::new()
        .with_prompt("Client ID")
        .interact_text()?)?);

    let client_secret = Input::new()
        .with_prompt("Client secret")
        .interact_text()?;

    let database_url = Input::new()
        .with_prompt("Database url")
        .interact_text()?;

    let host_addr = SocketAddr::from_str(&Input::<String>::new()
        .with_prompt("Host address")
        .interact_text()?)?;

    let api_url = Input::<String>::new()
        .with_prompt("Api url")
        .interact_text()?;

    let frontend_url = Input::<String>::new()
        .with_prompt("Frontend url")
        .interact_text()?;

    let owner_id = u64::from_str(&Input::<String>::new()
        .with_prompt("Owner id")
        .interact_text()?)?;

    let manager_guild = u64::from_str(&Input::<String>::new()
        .with_prompt("Manager guild id")
        .interact_text()?)?;

    let config = Config {
        token,
        client_id,
        client_secret,
        database_url,
        host_addr,
        api_url,
        frontend_url,
        owner_id: UserId(owner_id),
        manager_guild: GuildId(manager_guild),
        commands_guild: None,
    };

    fs::write("config.ron", ron::ser::to_string_pretty(&config,
        PrettyConfig::new().with_extensions(Extensions::UNWRAP_NEWTYPES | Extensions::IMPLICIT_SOME))?)?;

    println!("Generating keypair");

    let keypair = rcgen::KeyPair::generate(&PKCS_ECDSA_P256_SHA256)?;
    fs::write("public.pem", keypair.serialize_pem())?;
    fs::write("private.pem", keypair.public_key_pem())?;

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
