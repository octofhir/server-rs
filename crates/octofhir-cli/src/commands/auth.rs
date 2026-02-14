use anyhow::{Context, Result};
use colored::Colorize;

use crate::auth::{self, StoredCredentials};
use crate::cli::{AuthFlow, LoginArgs};
use crate::output::{print_error, print_success};

pub async fn login(server: &str, args: &LoginArgs, profile: &str) -> Result<()> {
    match args.auth_flow {
        AuthFlow::Basic => login_basic(server, args, profile),
        AuthFlow::OAuth => login_oauth(server, args, profile).await,
    }
}

fn login_basic(server: &str, args: &LoginArgs, profile: &str) -> Result<()> {
    let username = args
        .username
        .as_deref()
        .context("--username is required")?;
    let password = args
        .password
        .as_deref()
        .context("--password is required")?;

    let creds = StoredCredentials::Basic {
        server: server.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    };
    auth::save_credentials(profile, &creds)?;
    print_success(&format!(
        "Saved Basic Auth credentials for {} (user: {})",
        server.cyan(),
        username.cyan()
    ));
    Ok(())
}

async fn login_oauth(server: &str, args: &LoginArgs, profile: &str) -> Result<()> {
    let token_resp = if let (Some(client_id), Some(client_secret)) =
        (&args.client_id, &args.client_secret)
    {
        // client_credentials grant
        println!("Logging in with client credentials...");
        auth::oauth_client_credentials(server, client_id, client_secret).await?
    } else {
        // password grant (requires username + password + client_id)
        let username = args
            .username
            .as_deref()
            .context("--username is required for OAuth password grant")?;
        let password = args
            .password
            .as_deref()
            .context("--password is required for OAuth password grant")?;
        let client_id = args
            .client_id
            .as_deref()
            .context("--client-id is required for OAuth password grant")?;
        println!("Logging in as {username} (OAuth)...");
        auth::oauth_password(server, username, password, client_id).await?
    };

    let creds = StoredCredentials::Bearer {
        server: server.to_string(),
        access_token: token_resp.access_token,
    };
    auth::save_credentials(profile, &creds)?;
    print_success(&format!("Logged in to {} (OAuth Bearer)", server.cyan()));
    Ok(())
}

pub fn logout(profile: &str) -> Result<()> {
    if auth::remove_credentials(profile)? {
        print_success("Logged out (credentials removed)");
    } else {
        println!("No credentials found for profile \"{profile}\"");
    }
    Ok(())
}

pub fn whoami(profile: &str) -> Result<()> {
    match auth::load_credentials(profile)? {
        Some(creds) => {
            println!("{}: {}", "Profile".cyan(), profile);
            println!("{}: {}", "Server".cyan(), creds.server().cyan());
            match &creds {
                StoredCredentials::Basic { username, .. } => {
                    println!("{}: Basic (user: {})", "Auth".cyan(), username);
                }
                StoredCredentials::Bearer { access_token, .. } => {
                    let preview = if access_token.len() > 20 {
                        format!(
                            "{}...{}",
                            &access_token[..8],
                            &access_token[access_token.len() - 8..]
                        )
                    } else {
                        access_token.clone()
                    };
                    println!("{}: Bearer (token: {})", "Auth".cyan(), preview);
                }
            }
        }
        None => {
            print_error(&format!("Not logged in (profile: \"{profile}\")"));
        }
    }
    Ok(())
}
