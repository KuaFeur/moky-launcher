use serde::Deserialize;
use std::{env, error::Error, io::Write, time::Duration};

const REPOSITORY: &str = "KuaFeur/moky-launcher";
const ASSET_NAME: &str = "moky-launcher-windows-x86_64.exe";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

fn check_for_updates() -> Result<bool, Box<dyn Error>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("MokyLauncher-Updater")
        .timeout(Duration::from_secs(30))
        .build()?;

    let release: Release = client
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
        .send()?
        .error_for_status()?
        .json()?;

    let latest = semver::Version::parse(release.tag_name.trim_start_matches('v'))?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;

    if latest <= current {
        println!("Moky Launcher est à jour ({current}).");
        return Ok(false);
    }

    println!("Mise à jour disponible : {current} -> {latest}");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == ASSET_NAME)
        .ok_or("Le fichier Windows attendu est absent de la dernière release GitHub")?;

    let mut response = client
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?;

    let mut temporary_file = tempfile::Builder::new()
        .prefix("moky-launcher-update-")
        .suffix(".exe")
        .tempfile()?;

    response.copy_to(&mut temporary_file)?;
    temporary_file.flush()?;

    // self-replace gère le remplacement de l'exécutable en cours, notamment sous Windows.
    let replacement_path = temporary_file.path().to_path_buf();
    self_replace::self_replace(&replacement_path)?;

    println!("Moky Launcher a été mis à jour vers {latest}.");
    Ok(true)
}

fn main() {
    if env::args().any(|arg| arg == "--version") {
        println!("Moky Launcher {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    println!("Moky Launcher — Moky Studios");

    match check_for_updates() {
        Ok(true) => {
            // Sous Windows, self-replace termine le remplacement à la fermeture du processus.
            println!("Mise à jour installée. La nouvelle version sera utilisée au prochain démarrage.");
            return;
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("Impossible de vérifier les mises à jour : {error}");
            eprintln!("Le launcher continue avec la version installée.");
        }
    }

    println!("Le launcher est prêt. Les fonctionnalités Minecraft arriveront ensuite.");
}
