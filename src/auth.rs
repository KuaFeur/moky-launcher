use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use url::Url;

const CLIENT_ID: &str = "d53c2b5d-6674-4c80-9f3a-a9e5bd4fb0f7";
const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";
const CREDENTIAL_SERVICE: &str = "MokyLauncher";
const CREDENTIAL_USER: &str = "minecraft-refresh-token";

#[derive(Clone, Debug)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    pub skin_url: Option<String>,
}

#[derive(Deserialize)]
struct OAuthToken {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxUserProperties {
    auth_method: &'static str,
    site_name: &'static str,
    rps_ticket: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxUserRequest {
    properties: XboxUserProperties,
    relying_party: &'static str,
    token_type: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxUserResponse {
    token: String,
    display_claims: XboxDisplayClaims,
}

#[derive(Deserialize)]
struct XboxDisplayClaims {
    xui: Vec<XboxUserHash>,
}

#[derive(Deserialize)]
struct XboxUserHash {
    uhs: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsProperties {
    sandbox_id: &'static str,
    user_tokens: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsRequest {
    properties: XstsProperties,
    relying_party: &'static str,
    token_type: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XstsResponse {
    token: String,
    display_claims: XboxDisplayClaims,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MinecraftLoginRequest {
    identity_token: String,
}

#[derive(Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct EntitlementsResponse {
    items: Vec<Entitlement>,
}

#[derive(Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Deserialize)]
struct MinecraftProfileResponse {
    id: String,
    name: String,
    skins: Option<Vec<MinecraftSkin>>,
}

#[derive(Deserialize)]
struct MinecraftSkin {
    url: String,
    state: Option<String>,
}

fn http_client() -> Result<Client, Box<dyn Error + Send + Sync>> {
    Ok(Client::builder()
        .user_agent("MokyLauncher/0.1")
        .timeout(Duration::from_secs(30))
        .build()?)
}

fn random_url_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn open_browser(url: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

fn receive_authorization_code(listener: TcpListener, expected_state: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(75);
    loop {
        if Instant::now() > deadline {
            return Err("La connexion Microsoft a expiré. Réessayez.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0u8; 8192];
                let count = stream.read(&mut buffer)?;
                let request = String::from_utf8_lossy(&buffer[..count]);
                let first_line = request.lines().next().unwrap_or_default();
                let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                let callback = Url::parse(&format!("http://localhost{path}"))?;
                let params: std::collections::HashMap<String, String> =
                    callback.query_pairs().into_owned().collect();

                let body;
                let result = if let Some(error) = params.get("error") {
                    body = format!("<h2>Connexion impossible</h2><p>{}</p><p>Vous pouvez fermer cette fenêtre.</p>", error);
                    Err(format!("Microsoft a refusé la connexion : {error}"))
                } else if params.get("state").map(String::as_str) != Some(expected_state) {
                    body = "<h2>Connexion non vérifiée</h2><p>La réponse de sécurité ne correspond pas.</p>".to_string();
                    Err("La vérification de sécurité OAuth a échoué.".to_string())
                } else if let Some(code) = params.get("code") {
                    body = "<h2>Connexion réussie</h2><p>Vous pouvez fermer cette fenêtre et revenir dans Moky Launcher.</p>".to_string();
                    Ok(code.clone())
                } else {
                    body = "<h2>Réponse invalide</h2><p>Vous pouvez fermer cette fenêtre.</p>".to_string();
                    Err("Microsoft n'a renvoyé aucun code d'autorisation.".to_string())
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                return result.map_err(|e| -> Box<dyn Error + Send + Sync> { e.into() });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(150));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn microsoft_access_token(client: &Client) -> Result<(String, String), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}");
    let state = random_url_token();
    let verifier = random_url_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut authorize = Url::parse("https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize")?;
    authorize.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_mode", "query")
        .append_pair("scope", OAUTH_SCOPE)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    open_browser(authorize.as_str())?;
    let code = receive_authorization_code(listener, &state)?;

    let token: OAuthToken = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", &verifier),
            ("scope", OAUTH_SCOPE),
        ])
        .send()?
        .error_for_status()?
        .json()?;

    let refresh = token.refresh_token.ok_or("Microsoft n'a pas fourni de jeton de renouvellement.")?;
    Ok((token.access_token, refresh))
}

fn refresh_microsoft_token(client: &Client, refresh: &str) -> Result<(String, String), Box<dyn Error + Send + Sync>> {
    let token: OAuthToken = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    Ok((token.access_token, token.refresh_token.unwrap_or_else(|| refresh.to_string())))
}

fn minecraft_profile(client: &Client, microsoft_token: &str) -> Result<MinecraftProfile, Box<dyn Error + Send + Sync>> {
    let xbox: XboxUserResponse = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("x-xbl-contract-version", "1")
        .json(&XboxUserRequest {
            properties: XboxUserProperties {
                auth_method: "RPS",
                site_name: "user.auth.xboxlive.com",
                rps_ticket: format!("d={microsoft_token}"),
            },
            relying_party: "http://auth.xboxlive.com",
            token_type: "JWT",
        })
        .send()?
        .error_for_status()?
        .json()?;

    let xsts: XstsResponse = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("x-xbl-contract-version", "1")
        .json(&XstsRequest {
            properties: XstsProperties {
                sandbox_id: "RETAIL",
                user_tokens: vec![xbox.token],
            },
            relying_party: "rp://api.minecraftservices.com/",
            token_type: "JWT",
        })
        .send()?
        .error_for_status()?
        .json()?;

    let user_hash = xsts.display_claims.xui.first()
        .ok_or("Xbox n'a fourni aucun identifiant utilisateur.")?
        .uhs.clone();

    let minecraft: MinecraftLoginResponse = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&MinecraftLoginRequest {
            identity_token: format!("XBL3.0 x={user_hash};{}", xsts.token),
        })
        .send()?
        .error_for_status()?
        .json()?;

    let entitlements: EntitlementsResponse = client
        .get("https://api.minecraftservices.com/entitlements/mcstore")
        .bearer_auth(&minecraft.access_token)
        .send()?
        .error_for_status()?
        .json()?;

    if !entitlements.items.iter().any(|item| {
        item.name == "game_minecraft" || item.name == "product_minecraft"
    }) {
        return Err("Ce compte ne possède pas Minecraft: Java Edition (ou l'accès Game Pass correspondant).".into());
    }

    let profile: MinecraftProfileResponse = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&minecraft.access_token)
        .send()?
        .error_for_status()?
        .json()?;

    let skin_url = profile.skins.and_then(|skins| {
        skins.iter()
            .find(|skin| skin.state.as_deref() == Some("ACTIVE"))
            .or_else(|| skins.first())
            .map(|skin| skin.url.clone())
    });

    Ok(MinecraftProfile {
        id: profile.id,
        name: profile.name,
        skin_url,
    })
}

fn store_refresh_token(token: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)?;
    entry.set_password(token)?;
    Ok(())
}

pub fn sign_in() -> Result<MinecraftProfile, String> {
    let result = (|| -> Result<MinecraftProfile, Box<dyn Error + Send + Sync>> {
        let client = http_client()?;
        let (microsoft_token, refresh_token) = microsoft_access_token(&client)?;
        let profile = minecraft_profile(&client, &microsoft_token)?;
        store_refresh_token(&refresh_token)?;
        Ok(profile)
    })();
    result.map_err(|error| {
        let message = error.to_string();
        if message.contains("403") || message.to_lowercase().contains("invalid app registration") {
            format!("{message}\n\nMicrosoft doit autoriser le Client ID de Moky Launcher à utiliser Minecraft Services. L'inscription Entra seule ne suffit pas.")
        } else {
            message
        }
    })
}

pub fn restore_session() -> Result<Option<MinecraftProfile>, String> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|e| e.to_string())?;
    let old_refresh = match entry.get_password() {
        Ok(token) => token,
        Err(_) => return Ok(None),
    };

    let result = (|| -> Result<MinecraftProfile, Box<dyn Error + Send + Sync>> {
        let client = http_client()?;
        let (microsoft_token, new_refresh) = refresh_microsoft_token(&client, &old_refresh)?;
        let profile = minecraft_profile(&client, &microsoft_token)?;
        store_refresh_token(&new_refresh)?;
        Ok(profile)
    })();
    result.map(Some).map_err(|error| error.to_string())
}

pub fn sign_out() -> Result<(), String> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
