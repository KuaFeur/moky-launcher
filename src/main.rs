#![windows_subsystem = "windows"]

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke, Vec2};
use serde::Deserialize;
use std::{
    env,
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

const REPOSITORY: &str = "KuaFeur/moky-launcher";
const ASSET_NAME: &str = "MokyLauncher-Setup.exe";
const APP_NAME: &str = "Moky Launcher";
const BG: Color32 = Color32::BLACK;
const PANEL: Color32 = Color32::from_rgb(10, 10, 10);
const PANEL_ALT: Color32 = Color32::from_rgb(24, 24, 24);
const TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = Color32::from_rgb(165, 165, 165);
const PURPLE: Color32 = Color32::WHITE;
const PURPLE_DARK: Color32 = Color32::WHITE;
const GREEN: Color32 = Color32::WHITE;
const ORANGE: Color32 = Color32::from_rgb(190, 190, 190);

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

#[derive(Clone)]
enum Event {
    Checking,
    Downloading(String),
    UpToDate(String),
    Updated(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Installer,
    Installing,
    Launcher,
}

struct MokyApp {
    screen: Screen,
    status: String,
    version: String,
    receiver: Receiver<Event>,
    sender: Sender<Event>,
    install_error: Option<String>,
    install_path: PathBuf,
}

impl MokyApp {
    fn new() -> Self {
        let install_path = installation_path();
        let installed = env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            == Some(install_path.join("MokyLauncher.exe").canonicalize().unwrap_or_default());

        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            screen: if installed { Screen::Launcher } else { Screen::Installer },
            status: if installed { "Recherche des mises à jour…".into() } else { "Prêt à installer".into() },
            version: env!("CARGO_PKG_VERSION").into(),
            receiver,
            sender,
            install_error: None,
            install_path,
        };

        if installed {
            app.start_update_check();
        }
        app
    }

    fn install(&mut self, ctx: &egui::Context) {
        self.screen = Screen::Installing;
        self.status = "Préparation de l’installation…".into();
        self.install_error = None;
        let destination = self.install_path.join("MokyLauncher.exe");
        let marker = self.install_path.join("MokyLauncher.installed");
        let _ = fs::remove_file(&marker);
        let ctx = ctx.clone();

        thread::spawn(move || {
            let result = install_current_executable(&destination)
                .and_then(|_| fs::write(&marker, "installed").map_err(Into::into));
            ctx.request_repaint();
            if let Err(error) = result {
                let _ = fs::write(
                    env::temp_dir().join("moky-launcher-install-error.txt"),
                    error.to_string(),
                );
            }
        });
    }

    fn start_update_check(&mut self) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let _ = sender.send(Event::Checking);
            match check_for_updates(&sender) {
                Ok(Some(version)) => { let _ = sender.send(Event::Updated(version)); }
                Ok(None) => {
                    let _ = sender.send(Event::UpToDate(env!("CARGO_PKG_VERSION").into()));
                }
                Err(error) => { let _ = sender.send(Event::Failed(error.to_string())); }
            }
        });
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Checking => self.status = "Recherche des mises à jour…".into(),
                Event::Downloading(version) => self.status = format!("Téléchargement de la version {version}…"),
                Event::UpToDate(version) => {
                    self.version = version;
                    self.status = "Votre launcher est à jour".into();
                }
                Event::Updated(version) => {
                    self.version = version.clone();
                    self.status = format!("Mise à jour {version} prête. Redémarrez le launcher pour l’appliquer.");
                }
                Event::Failed(error) => self.status = format!("Vérification indisponible : {error}"),
            }
            ctx.request_repaint();
        }

        if self.screen == Screen::Installing {
            let error_file = env::temp_dir().join("moky-launcher-install-error.txt");
            if error_file.exists() {
                self.install_error = fs::read_to_string(&error_file).ok();
                let _ = fs::remove_file(error_file);
                self.screen = Screen::Installer;
            } else if self.install_path.join("MokyLauncher.installed").exists() {
                self.status = "Installation terminée !".into();
                self.screen = Screen::Launcher;
                if let Ok(exe) = env::current_exe() {
                    let target = self.install_path.join("MokyLauncher.exe");
                    if exe != target {
                        let _ = Command::new(target).spawn();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
    }
}

impl eframe::App for MokyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events(ctx);
        ctx.request_repaint_after(Duration::from_millis(180));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(30)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui_material_icons::icons::ICON_APPS);
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("MOKY").font(FontId::proportional(17.0)).strong().color(TEXT));
                            ui.label(RichText::new("STUDIOS").font(FontId::proportional(10.0)).strong().color(TEXT));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            pill(ui, "OFFICIAL", Color32::WHITE);
                        });
                    });

                    ui.add_space(34.0);
                    match self.screen {
                        Screen::Installer => self.installer_ui(ui, ctx),
                        Screen::Installing => self.installing_ui(ui),
                        Screen::Launcher => self.launcher_ui(ui),
                    }
                    ui.add_space(20.0);
                    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                        ui.label(RichText::new(format!("MOKY STUDIOS  •  VERSION {}", self.version))
                            .size(10.0).strong().color(MUTED));
                    });
                });
            });
    }
}

impl MokyApp {
    fn installer_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("Votre aventure").size(15.0).color(MUTED));
        ui.label(RichText::new("commence ici.").size(34.0).strong().color(TEXT));
        ui.add_space(10.0);
        ui.label(RichText::new("Installez Moky Launcher et retrouvez tous vos serveurs Moky Studios au même endroit.")
            .size(14.0).color(MUTED));
        ui.add_space(26.0);

        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui_material_icons::icons::ICON_DOWNLOAD);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Installation rapide").strong().color(TEXT));
                    ui.label(RichText::new("Un seul clic, aucune configuration compliquée.").size(12.0).color(MUTED));
                });
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(egui_material_icons::icons::ICON_SYSTEM_UPDATE_ALT);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Toujours à jour").strong().color(TEXT));
                    ui.label(RichText::new("Le launcher vérifie automatiquement les nouvelles versions.").size(12.0).color(MUTED));
                });
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(egui_material_icons::icons::ICON_APPS);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Pensé pour Moky").strong().color(TEXT));
                    ui.label(RichText::new("Tous les univers Moky Studios, réunis dans une seule application.").size(12.0).color(MUTED));
                });
            });
        });

        ui.add_space(20.0);
        ui.label(RichText::new(format!("Emplacement : {}", self.install_path.display())).size(11.0).color(MUTED));
        ui.add_space(10.0);
        if primary_button(ui, "Installer Moky Launcher") {
            self.install(ctx);
        }
        if let Some(error) = &self.install_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Échec de l’installation : {error}")).color(Color32::LIGHT_RED).size(12.0));
        }
    }

    fn installing_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.add_space(22.0);
            ui.label(RichText::new("Installation en cours…").size(26.0).strong().color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new("Moky Launcher prépare votre espace de jeu.").color(MUTED));
            ui.add_space(24.0);
            ui.add(egui::ProgressBar::new(0.65).desired_width(360.0).fill(PURPLE).corner_radius(egui::CornerRadius::same(8)));
            ui.add_space(12.0);
            ui.label(RichText::new(&self.status).size(12.0).color(MUTED));
        });
    }

    fn launcher_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("BIENVENUE SUR").size(12.0).strong().color(PURPLE));
        ui.label(RichText::new("Moky Launcher").size(34.0).strong().color(TEXT));
        ui.add_space(8.0);
        ui.label(RichText::new("Tous vos serveurs. Une seule destination.").size(14.0).color(MUTED));
        ui.add_space(24.0);

        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ÉTAT DU LAUNCHER").size(10.0).strong().color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, "EN LIGNE", GREEN);
                });
            });
            ui.add_space(12.0);
            ui.label(RichText::new(&self.status).size(14.0).color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Version installée : {}", self.version)).size(11.0).color(MUTED));
        });

        ui.add_space(18.0);
        panel(ui, |ui| {
            ui.label(RichText::new("VOS UNIVERS").size(11.0).strong().color(PURPLE));
            ui.add_space(8.0);
            ui.label(RichText::new("Les serveurs Moky Studios arrivent bientôt.").size(16.0).strong().color(TEXT));
            ui.label(RichText::new("Nous préparons votre espace de jeu.").size(12.0).color(MUTED));
        });
        ui.add_space(18.0);
        if primary_button(ui, "Lancer Minecraft") {
            self.status = "La sélection des serveurs sera disponible prochainement.".into();
        }
    }
}

fn installation_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("Programs").join("Moky Studios").join("Moky Launcher")
}

fn install_current_executable(destination: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    let source = env::current_exe()?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, destination)?;

    let target = destination.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$shell = New-Object -ComObject WScript.Shell; \
         $start = [Environment]::GetFolderPath('Programs'); \
         $desktop = [Environment]::GetFolderPath('Desktop'); \
         foreach ($folder in @($start, $desktop)) {{ \
           $shortcut = $shell.CreateShortcut((Join-Path $folder 'Moky Launcher.lnk')); \
           $shortcut.TargetPath = '{target}'; \
           $shortcut.WorkingDirectory = Split-Path '{target}'; \
           $shortcut.Description = 'Moky Launcher - Moky Studios'; \
           $shortcut.Save(); \
         }}"
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .status()?;
    if !status.success() {
        return Err("Impossible de créer les raccourcis Windows".into());
    }
    Ok(())
}

fn check_for_updates(sender: &Sender<Event>) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("MokyLauncher-Updater")
        .timeout(Duration::from_secs(60))
        .build()?;
    let release: Release = client
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
        .send()?
        .error_for_status()?
        .json()?;

    let latest = semver::Version::parse(release.tag_name.trim_start_matches('v'))?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    if latest <= current {
        return Ok(None);
    }

    let _ = sender.send(Event::Downloading(latest.to_string()));
    let asset = release.assets.iter().find(|asset| asset.name == ASSET_NAME)
        .ok_or("L’installateur Windows est absent de la dernière release GitHub")?;
    let mut response = client.get(&asset.browser_download_url).send()?.error_for_status()?;
    let mut temporary_file = tempfile::Builder::new()
        .prefix("moky-launcher-update-").suffix(".exe").tempfile()?;
    response.copy_to(&mut temporary_file)?;
    temporary_file.flush()?;
    self_replace::self_replace(temporary_file.path())?;
    Ok(Some(latest.to_string()))
}

fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(13), PURPLE_DARK);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "M",
        FontId::proportional(25.0), Color32::WHITE);
}

fn feature_icon(ui: &mut egui::Ui, symbol: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(11), Color32::from_rgb(48, 39, 72));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, symbol,
        FontId::proportional(18.0), PURPLE);
}

fn panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, Color32::from_rgb(48, 48, 48)))
        .corner_radius(egui::CornerRadius::same(16))
        .inner_margin(egui::Margin::same(18))
        .show(ui, add_contents);
}

fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgb(35, 35, 35))
        .corner_radius(egui::CornerRadius::same(20))
        .inner_margin(egui::Margin { left: 10, right: 10, top: 5, bottom: 5 })
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(9.0).strong().color(Color32::WHITE));
        });
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> bool {
    let button = egui::Button::new(RichText::new(text).size(14.0).strong().color(Color32::BLACK))
        .fill(Color32::WHITE)
        .stroke(Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(12))
        .min_size(Vec2::new(ui.available_width(), 48.0));
    ui.add(button).clicked()
}


fn main() -> eframe::Result<()> {
    if env::args().any(|arg| arg == "--version") {
        println!("Moky Launcher {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 650.0])
            .with_min_inner_size([560.0, 600.0])
            .with_resizable(false)
            .with_title("Moky Launcher — Moky Studios"),
        ..Default::default()
    };

    eframe::run_native(
        "Moky Launcher",
        options,
        Box::new(|cc| { egui_material_icons::initialize(&cc.egui_ctx); Ok(Box::new(MokyApp::new())) }),
    )
}
