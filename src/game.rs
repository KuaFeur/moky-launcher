use crate::auth::MinecraftProfile;
use mc_launcher_core::prelude::*;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const GAME_VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const SODIUM_VERSIONS_API: &str = "https://api.modrinth.com/v2/project/sodium/version";

#[derive(Deserialize)]
struct VersionManifest {
    latest: LatestVersions,
}
#[derive(Deserialize)]
struct LatestVersions {
    release: String,
}

#[derive(Deserialize)]
struct ModrinthVersion {
    game_versions: Vec<String>,
    loaders: Vec<String>,
    version_type: String,
    files: Vec<ModrinthFile>,
}
#[derive(Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
    primary: bool,
}

fn game_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Moky Studios")
        .join("Minecraft")
}

fn private_java() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Moky Studios")
        .join("runtime")
        .join("java-25")
        .join("bin")
        .join("javaw.exe")
}

fn download_sodium(client: &Client, mods_dir: &Path, minecraft_version: &str, report: &impl Fn(String)) -> Result<(), Box<dyn Error + Send + Sync>> {
    fs::create_dir_all(mods_dir)?;
    let versions: Vec<ModrinthVersion> = client
        .get(SODIUM_VERSIONS_API)
        .query(&[("game_versions", format!("[\"{minecraft_version}\"]")), ("loaders", "[\"neoforge\"]".to_string())])
        .send()?
        .error_for_status()?
        .json()?;
    let version = versions.into_iter()
        .find(|version| version.version_type == "release"
            && version.game_versions.iter().any(|v| v == minecraft_version)
            && version.loaders.iter().any(|loader| loader == "neoforge"))
        .ok_or("Aucune version stable de Sodium compatible avec NeoForge n'a été trouvée")?;
    let file = version.files.iter().find(|file| file.primary)
        .or_else(|| version.files.first())
        .ok_or("Le paquet Sodium ne contient aucun fichier")?;
    let destination = mods_dir.join(&file.filename);
    if destination.is_file() {
        report("Sodium est déjà installé".into());
        return Ok(());
    }
    report("Téléchargement de Sodium…".into());
    let mut response = client.get(&file.url).send()?.error_for_status()?;
    let mut output = fs::File::create(&destination)?;
    std::io::copy(&mut response, &mut output)?;
    if !destination.is_file() || fs::metadata(&destination)?.len() == 0 {
        return Err("Le téléchargement de Sodium est incomplet".into());
    }
    Ok(())
}

pub fn launch_minecraft(
    profile: MinecraftProfile,
    report: impl Fn(String) + Send + 'static,
) -> Result<(), String> {
    let result = (|| -> Result<(), Box<dyn Error + Send + Sync>> {
        let client = Client::builder()
            .user_agent("MokyLauncher/0.1")
            .timeout(Duration::from_secs(90))
            .build()?;
        let root = game_directory();
        fs::create_dir_all(&root)?;
        let manifest: VersionManifest = client.get(GAME_VERSION_MANIFEST)
            .send()?.error_for_status()?.json()?;
        let minecraft_version = manifest.latest.release;
        report(format!("Installation de Minecraft {minecraft_version}…"));
        let launcher = Launcher::new(&root);
        let installed = launcher.install(InstallRequest {
            minecraft_version: minecraft_version.clone(),
            loader: Some(LoaderSpec::NeoForge { version: LoaderVersion::LatestStable }),
            java: JavaInstallPolicy::Never,
        })?;
        let mods_dir = root.join("mods");
        download_sodium(&client, &mods_dir, &minecraft_version, &report)?;
        report("Préparation du lancement…".into());
        let version = launcher.load_version(&installed.version_id)?;
        let java = private_java();
        if !java.is_file() {
            return Err("Java 25 privé est introuvable. Fermez puis relancez Moky Launcher pour réparer son installation.".into());
        }
        let options = LaunchOptions {
            account: Account::Microsoft {
                username: profile.name,
                uuid: profile.id,
                access_token: profile.access_token,
            },
            java_executable: Some(java),
            game_directory: Some(root.clone()),
            ..Default::default()
        };
        let command = launcher.build_launch_command_from_version(&version, options)?;
        let mut process = Command::new(&command.executable)
            .args(&command.args)
            .current_dir(&command.working_dir)
            .envs(&command.env)
            .spawn()?;
        // Do not block the UI while the game is running. Keep the child detached.
        drop(process);
        report("Minecraft est lancé".into());
        Ok(())
    })();
    result.map_err(|error| error.to_string())
}
