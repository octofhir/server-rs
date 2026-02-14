mod auth;
mod cli;
mod client;
mod commands;
mod config;
mod output;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;

use cli::{Cli, Commands};
use client::FhirClient;
use output::print_error;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        print_error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let profile = &cli.profile;
    let format = cli.format.unwrap_or_default();

    match &cli.command {
        Commands::Login(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            commands::auth::login(&server, args, profile).await?;
        }
        Commands::Logout => {
            commands::auth::logout(profile)?;
        }
        Commands::Whoami => {
            commands::auth::whoami(profile)?;
        }
        Commands::Config(args) => match &args.command {
            cli::ConfigCommands::Show => {
                let cfg = config::load_profile(profile)?;
                println!("{}: {}", "Profile".cyan(), profile);
                println!(
                    "{}: {}",
                    "Server".cyan(),
                    cfg.server.as_deref().unwrap_or("(not set)")
                );
                println!(
                    "{}: {}",
                    "Format".cyan(),
                    cfg.format.as_deref().unwrap_or("json")
                );
            }
            cli::ConfigCommands::Set(set_args) => {
                let mut cfg = config::load_profile(profile)?;
                match set_args.key.as_str() {
                    "server" => cfg.server = Some(set_args.value.clone()),
                    "format" => cfg.format = Some(set_args.value.clone()),
                    other => {
                        anyhow::bail!("Unknown config key: {other}. Valid keys: server, format")
                    }
                }
                config::save_profile(profile, &cfg)?;
                output::print_success(&format!("Set {} = {}", set_args.key, set_args.value));
            }
        },
        Commands::Status => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::server::status(&client, &server).await?;
        }
        Commands::Metadata => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::server::metadata(&client, format).await?;
        }
        Commands::Get(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::crud::get(&client, &args.reference, format).await?;
        }
        Commands::Create(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::crud::create(&client, &args.resource_type, &args.file, format).await?;
        }
        Commands::Update(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::crud::update(&client, &args.reference, &args.file, format).await?;
        }
        Commands::Delete(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::crud::delete(&client, &args.reference).await?;
        }
        Commands::History(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::crud::history(&client, &args.reference, format).await?;
        }
        Commands::Search(args) => {
            let server = config::resolve_server(&cli.server, profile)?;
            let client = make_client(&server, profile)?;
            commands::search::search(
                &client,
                &args.resource_type,
                &args.params,
                args.count,
                format,
            )
            .await?;
        }
    }

    Ok(())
}

fn make_client(server: &str, profile: &str) -> Result<FhirClient> {
    let auth_header = auth::load_credentials(profile)?.map(|c| auth::to_auth_header(&c));
    Ok(FhirClient::new(server, auth_header))
}
