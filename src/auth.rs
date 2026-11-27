use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    thread,
    time::Duration,
};

const CLIENT_ID: &str = "d53c2b5d-6674-4c80-9f3a-a9e5bd4fb0f7";
const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";
const CREDENTIAL_SERVICE: &str = "MokyLauncher";
const CREDENTIAL_USER: &str = "minecraft-refresh-token";

#[derive(Clone, Debug)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    pub skin_url: Option<String>,
    pub access_token: String,
}

#[derive(Deserialize)]
struct OAuthToken {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub message: String,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
    message: String,
}

#[derive(Deserialize)]
struct DeviceTokenError {
    error: Option<String>,
    error_description: Option<String>,
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
struct LauncherLoginRequest {
    xtoken: String,
    platform: &'static str,
}

#[derive(Deserialize)]
struct LauncherLoginResponse {
    access_token: String,
    expires_in: u64,
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

fn ensure_minecraft_response(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, Box<dyn Error + Send + Sync>> {
    if response.status().as_u16() == 429 {
        let retry_after = response.headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(|value| format!(" Indication du serveur : {value}."))
            .unwrap_or_else(|| " Attendez quelques minutes avant de réessayer.".to_string());
        return Err(format!(
            "Minecraft Services limite temporairement les connexions (HTTP 429).{retry_after}"
        ).into());
    }
    Ok(response.error_for_status()?)
}

fn begin_device_code(client: &Client) -> Result<(DeviceCode, String, u64), Box<dyn Error + Send + Sync>> {
    let response: DeviceCodeResponse = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[("client_id", CLIENT_ID), ("scope", OAUTH_SCOPE)])
        .send()?
        .error_for_status()?
        .json()?;
    let display = DeviceCode {
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        verification_uri_complete: response.verification_uri_complete,
        message: response.message,
    };
    Ok((display, response.device_code, response.expires_in.max(1)))
}

pub fn start_device_sign_in() -> Result<(DeviceCode, String, u64), String> {
    let client = http_client().map_err(|e| e.to_string())?;
    begin_device_code(&client).map_err(|e| e.to_string())
}

fn poll_device_code(
    client: &Client,
    device_code: &str,
    expires_in: u64,
) -> Result<(String, String), Box<dyn Error + Send + Sync>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in);
    let mut interval = Duration::from_secs(5);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("Le code de connexion Microsoft a expiré. Relancez la connexion.".into());
        }
        thread::sleep(interval);
        let response = client
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", CLIENT_ID),
                ("device_code", device_code),
            ])
            .send()?;
        if response.status().is_success() {
            let token: OAuthToken = response.json()?;
            let refresh = token.refresh_token.ok_or("Microsoft n'a pas fourni de jeton de renouvellement.")?;
            return Ok((token.access_token, refresh));
        }
        let status = response.status();
        let error: DeviceTokenError = response.json().unwrap_or(DeviceTokenError {
            error: None,
            error_description: None,
        });
        match error.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += Duration::from_secs(5);
                continue;
            }
            Some("authorization_declined") => return Err("La connexion Microsoft a été refusée.".into()),
            Some("expired_token") => return Err("Le code de connexion Microsoft a expiré. Relancez la connexion.".into()),
            Some("bad_verification_code") => return Err("Microsoft a rejeté le code de connexion.".into()),
            _ => return Err(format!(
                "Échec de la connexion Microsoft (HTTP {status}) : {}",
                error.error_description.unwrap_or_else(|| "réponse inconnue".into())
            ).into()),
        }
    }
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

    // Match Prism Launcher's PC launcher authentication endpoint and payload.
    let minecraft_response = client
        .post("https://api.minecraftservices.com/launcher/login")
        .json(&LauncherLoginRequest {
            xtoken: format!("XBL3.0 x={user_hash};{}", xsts.token),
            platform: "PC_LAUNCHER",
        })
        .send()?;
    let minecraft: LauncherLoginResponse =
        ensure_minecraft_response(minecraft_response).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Minecraft Launcher Login (/launcher/login) a échoué : {error}"),
            )
        })?.json()?;

    // Prism uses the license endpoint with a unique request ID.
    let request_id = format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (rand::random::<u32>()),
        (rand::random::<u16>()),
        (rand::random::<u16>() & 0x0fff) | 0x4000,
        (rand::random::<u16>() & 0x3fff) | 0x8000,
        (rand::random::<u64>() & 0xffffffffffff));
    let entitlements: EntitlementsResponse = client
        .get(format!("https://api.minecraftservices.com/entitlements/license?requestId={request_id}"))
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
        access_token: minecraft.access_token,
    })
}

fn store_refresh_token(token: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)?;
    entry.set_password(token)?;
    Ok(())
}

pub fn complete_device_sign_in(device_code: &str, expires_in: u64) -> Result<MinecraftProfile, String> {
    let result = (|| -> Result<MinecraftProfile, Box<dyn Error + Send + Sync>> {
        let client = http_client()?;
        let (microsoft_token, refresh_token) = poll_device_code(&client, device_code, expires_in)?;
        let profile = minecraft_profile(&client, &microsoft_token)?;
        store_refresh_token(&refresh_token)?;
        Ok(profile)
    })();
    result.map_err(|error| {
        let message = error.to_string();
        if message.contains("403") || message.to_lowercase().contains("invalid app registration") {
            format!("{message}\n\nLe parcours PC_LAUNCHER de Prism a également été refusé. Cela peut indiquer que le Client ID de Moky n'est pas autorisé par Minecraft Services.")
        } else { message }
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
