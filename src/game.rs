use crate::auth::MinecraftProfile;
use mc_launcher_core::prelude::*;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    io::Read,
    time::Duration,
};

const GAME_VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const SODIUM_VERSIONS_API: &str = "https://api.modrinth.com/v2/project/sodium/version";
const NEOFORGE_METADATA: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

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
    hashes: ModrinthHashes,
}
#[derive(Deserialize)]
struct ModrinthHashes {
    sha512: String,
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
        .join("java.exe")
}

fn install_neoforge(client: &Client, root: &Path, minecraft_version: &str, report: &impl Fn(String)) -> std::result::Result<String, Box<dyn Error + Send + Sync>> {
    let metadata = client.get(NEOFORGE_METADATA).send()?.error_for_status()?.text()?;
    let prefix = format!("{minecraft_version}.");
    let mut versions: Vec<(Vec<u64>, String)> = metadata.split("<version>").skip(1)
        .filter_map(|part| part.split("</version>").next())
        .filter(|version| version.starts_with(&prefix))
        .filter_map(|version| {
            let components: Option<Vec<u64>> = version.split('.').map(|part| part.parse::<u64>().ok()).collect();
            components.map(|numbers| (numbers, version.to_string()))
        }).collect();
    versions.sort_by(|left, right| left.0.cmp(&right.0));
    let version = versions.last().ok_or("Aucune version stable NeoForge n'est publiée pour cette version de Minecraft")?.1.clone();
    let profile_id = format!("neoforge-{version}");
    let profile_json = root.join("versions").join(&profile_id).join(format!("{profile_id}.json"));
    if profile_json.is_file() { report(format!("NeoForge {version} est déjà installé")); return Ok(profile_id); }
    report(format!("Téléchargement de NeoForge {version}…"));
    let installer_dir = root.join("versions").join(".installers");
    fs::create_dir_all(&installer_dir)?;
    let installer = installer_dir.join(format!("neoforge-{version}-installer.jar"));
    let installer_valid = fs::File::open(&installer).ok()
        .and_then(|file| zip::ZipArchive::new(file).ok()).is_some();
    if !installer_valid {
        let _ = fs::remove_file(&installer);
        let url = format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar");
        let mut response = client.get(url).send()?.error_for_status()?;
        let mut output = fs::File::create(&installer)?;
        std::io::copy(&mut response, &mut output)?;
        output.sync_all()?;
        if fs::File::open(&installer).ok().and_then(|file| zip::ZipArchive::new(file).ok()).is_none() {
            let _ = fs::remove_file(&installer);
            return Err("L'installateur NeoForge téléchargé est invalide ou incomplet".into());
        }
    }
    report("Installation de NeoForge…".into());
    let java = private_java();
    if !java.is_file() { return Err("Java 25 privé est introuvable. Relancez Moky Launcher pour réparer Java.".into()); }
    let output = Command::new(java).arg("-jar").arg(&installer).arg("--installClient").arg("--gameDir").arg(root).output()?;
    if !output.status.success() {
        return Err(format!("L'installation NeoForge a échoué : {} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).into());
    }
    if !profile_json.is_file() { return Err("NeoForge a terminé sans créer le profil attendu".into()); }
    Ok(profile_id)
}

fn download_sodium(client: &Client, mods_dir: &Path, minecraft_version: &str, report: &impl Fn(String)) -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
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
        let mut existing = fs::File::open(&destination)?;
        let mut hasher = Sha512::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = existing.read(&mut buffer)?;
            if count == 0 { break; }
            hasher.update(&buffer[..count]);
        }
        if format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(&file.hashes.sha512) {
            report("Sodium est déjà installé".into());
            return Ok(());
        }
        fs::remove_file(&destination)?;
    }
    report("Téléchargement de Sodium…".into());
    let mut response = client.get(&file.url).send()?.error_for_status()?;
    let mut temporary = tempfile::Builder::new().prefix("moky-sodium-").suffix(".jar").tempfile_in(mods_dir)?;
    std::io::copy(&mut response, &mut temporary)?;
    temporary.as_file_mut().sync_all()?;
    let mut hasher = Sha512::new();
    let mut input = fs::File::open(temporary.path())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(&file.hashes.sha512) {
        return Err("La vérification SHA-512 de Sodium a échoué".into());
    }
    for entry in fs::read_dir(mods_dir)? {
        let path = entry?.path();
        if path != destination && path.file_name().and_then(|name| name.to_str())
            .map(|name| name.starts_with("sodium") && name.ends_with(".jar")).unwrap_or(false) {
            let _ = fs::remove_file(path);
        }
    }
    temporary.persist(&destination)?;
    Ok(())
}

pub fn launch_minecraft(
    profile: MinecraftProfile,
    report: impl Fn(String) + Send + 'static,
) -> std::result::Result<(), String> {
    let result = (|| -> Result<(), Box<dyn Error + Send + Sync>> {
        let fresh_profile = crate::auth::restore_session()
            .map_err(std::io::Error::other)?
            .ok_or("La session Microsoft n'est plus disponible. Reconnectez-vous.")?;
        if fresh_profile.id != profile.id {
            return Err("Le compte Microsoft actif a changé. Reconnectez-vous.".into());
        }
        let profile = fresh_profile;
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
        launcher.install(InstallRequest::vanilla(minecraft_version.clone()))?;
        let neoforge_profile = install_neoforge(&client, &root, &minecraft_version, &report)?;
        let mods_dir = root.join("mods");
        download_sodium(&client, &mods_dir, &minecraft_version, &report)?;
        report("Préparation du lancement…".into());
        let version = launcher.load_version(&neoforge_profile)?;
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
        let process = Command::new(&command.executable)
            .args(&command.args)
            .current_dir(&command.working_dir)
            .envs(command.env.iter().cloned())
            .spawn()?;
        // Do not block the UI while the game is running. Keep the child detached.
        drop(process);
        report("Minecraft est lancé".into());
        Ok(())
    })();
    result.map_err(|error| error.to_string())
}
