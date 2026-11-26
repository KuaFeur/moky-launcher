#![windows_subsystem = "windows"]

mod auth;

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Stroke, Vec2};
use serde::Deserialize;
use std::{
    env,
    error::Error,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

const REPOSITORY: &str = "KuaFeur/moky-launcher";
const ASSET_NAME: &str = "MokyLauncher.exe";
const APP_NAME: &str = "Moky Launcher";
const BG: Color32 = Color32::BLACK;
const PANEL: Color32 = Color32::from_rgb(10, 10, 10);
const PANEL_ALT: Color32 = Color32::from_rgb(24, 24, 24);
const TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = Color32::from_rgb(165, 165, 165);

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
    SessionRestored(Option<auth::MinecraftProfile>),
    SessionRestoreFailed(String),
    Authenticated(auth::MinecraftProfile),
    AuthFailed(String),
    DownloadProgress(String, f32),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Installer,
    Installing,
    Startup,
    Launcher,
    Auth,
}

struct MokyApp {
    screen: Screen,
    status: String,
    version: String,
    receiver: Receiver<Event>,
    sender: Sender<Event>,
    install_error: Option<String>,
    install_path: PathBuf,
    profile: Option<auth::MinecraftProfile>,
    session_checked: bool,
    startup_finished: bool,
    auth_busy: bool,
    startup_progress: f32,
    settings_open: bool,
    servers_open: bool,
}

impl MokyApp {
    fn new() -> Self {
        let install_path = installation_path();
        let installed = is_installed();
        let skip_update = env::args().any(|arg| arg == "--launch");

        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            screen: if !installed { Screen::Installer } else { Screen::Startup },
            status: if installed { "Recherche des mises à jour…".into() } else { "Prêt à installer".into() },
            version: env!("CARGO_PKG_VERSION").into(),
            receiver,
            sender,
            install_error: None,
            install_path,
            profile: None,
            session_checked: false,
            startup_finished: false,
            auth_busy: false,
            startup_progress: 0.0,
            settings_open: false,
            servers_open: false,
        };

        if installed {
            let sender = app.sender.clone();
            thread::spawn(move || {
                match auth::restore_session() {
                    Ok(profile) => { let _ = sender.send(Event::SessionRestored(profile)); }
                    Err(error) => { let _ = sender.send(Event::SessionRestoreFailed(error)); }
                }
            });
            app.start_update_check();
        }
        app
    }

    fn open_session_screen(&mut self, ctx: &egui::Context) {
        self.screen = if self.profile.is_some() { Screen::Launcher } else { Screen::Auth };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(1080.0, 720.0)));
        center_window(ctx, Vec2::new(1080.0, 720.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Moky Launcher — Moky Studios".into()));
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
                    self.startup_finished = true;
                    if self.session_checked { self.open_session_screen(ctx); }
                }
                Event::DownloadProgress(version, progress) => {
                    self.status = format!("Téléchargement de la version {version}…");
                    self.startup_progress = progress;
                }
                Event::Updated(_version) => {
                    if let Ok(exe) = env::current_exe() {
                        let _ = Command::new(exe).arg("--launch").spawn();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Event::Failed(_error) => {
                    self.startup_finished = true;
                    if self.session_checked { self.open_session_screen(ctx); }
                },
                Event::SessionRestored(profile) => {
                    self.profile = profile;
                    self.session_checked = true;
                    if self.startup_finished { self.open_session_screen(ctx); }
                },
                Event::SessionRestoreFailed(error) => {
                    self.profile = None;
                    self.session_checked = true;
                    self.status = format!("Impossible de restaurer la session : {error}");
                    if self.startup_finished { self.open_session_screen(ctx); }
                },
                Event::Authenticated(profile) => {
                    self.profile = Some(profile);
                    self.session_checked = true;
                    self.auth_busy = false;
                    self.status.clear();
                    self.screen = Screen::Launcher;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(1080.0, 720.0)));
                    center_window(ctx, Vec2::new(1080.0, 720.0));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title("Moky Launcher — Moky Studios".into()));
                },
                Event::AuthFailed(error) => { self.auth_busy = false; self.status = error; self.screen = Screen::Auth; },
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
                self.screen = Screen::Auth;
                self.session_checked = true;
                self.profile = None;
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
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(0)))
            .show(ctx, |ui| {
                self.custom_titlebar(ui, ctx);
                match self.screen {
                    Screen::Startup => self.startup_ui(ui),
                    Screen::Auth => self.auth_ui(ui),
                    Screen::Installer => self.installer_ui(ui, ctx),
                    Screen::Installing => self.installing_ui(ui),
                    Screen::Launcher => self.launcher_ui(ui, ctx),
                }
            });
    }
}

impl MokyApp {
    fn custom_titlebar(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let bar = egui::Frame::new()
            .fill(Color32::from_rgb(8, 8, 8))
            .inner_margin(egui::Margin { left: 18, right: 10, top: 5, bottom: 5 });
        bar.show(ui, |ui| {
            ui.set_min_height(28.0);
            ui.horizontal(|ui| {
                let title = match self.screen {
                    Screen::Startup => "MOKY  /  INITIALISATION",
                    Screen::Auth => "MOKY  /  COMPTE MICROSOFT",
                    Screen::Installer | Screen::Installing => "MOKY  /  INSTALLATION",
                    Screen::Launcher if self.settings_open => "MOKY  /  PARAMÈTRES",
                    Screen::Launcher => "MOKY STUDIOS",
                };
                let drag = ui.allocate_response(
                    Vec2::new((ui.available_width() - 92.0).max(80.0), 26.0),
                    egui::Sense::click_and_drag(),
                );
                ui.painter().text(
                    drag.rect.left_center() + Vec2::new(0.0, 1.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(9.0),
                    MUTED,
                );
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close = ui.add_sized(
                        Vec2::new(30.0, 24.0),
                        egui::Button::new(egui_material_icons::icons::ICON_CLOSE)
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    );
                    if close.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    let minimize = ui.add_sized(
                        Vec2::new(30.0, 24.0),
                        egui::Button::new(egui_material_icons::icons::ICON_REMOVE)
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    );
                    if minimize.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });
        });
        ui.painter().line_segment(
            [ui.min_rect().left_bottom(), ui.min_rect().right_bottom()],
            Stroke::new(1.0, Color32::from_rgb(35, 35, 35)),
        );
    }

    fn startup_ui(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        draw_voxel_backdrop(ui, rect, 0.22);
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new("M O K Y   S T U D I O S").size(9.0).strong().color(MUTED));
            ui.add_space(8.0);
            ui.label(RichText::new("Moky Launcher").size(22.0).strong().color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new(
                if self.startup_progress > 0.0 { "Téléchargement de la mise à jour" } else { "Vérification des mises à jour" }
            ).size(11.0).color(MUTED));
            ui.add_space(14.0);
            ui.add(egui::ProgressBar::new(self.startup_progress)
                .desired_width(300.0).animate(true).fill(Color32::WHITE));
            ui.add_space(7.0);
            ui.label(RichText::new("Le launcher démarre automatiquement").size(9.0).color(Color32::from_gray(105)));
        });
    }

    fn auth_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        draw_voxel_backdrop(ui, rect, 0.40);
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.add_space(42.0);
            ui.vertical(|ui| {
                ui.add_space(84.0);
                ui.label(RichText::new("M O K Y   S T U D I O S").size(10.0).strong().color(MUTED));
                ui.add_space(24.0);
                ui.label(RichText::new("UN SEUL ACCÈS.\nTOUS VOS MONDES.").size(31.0).strong().color(TEXT));
                ui.add_space(14.0);
                ui.label(RichText::new("Connectez votre compte Minecraft pour\naccéder à vos serveurs Moky Studios.")
                    .size(12.0).color(Color32::from_gray(190)));
                ui.add_space(34.0);
                ui.label(RichText::new("MICROSOFT ACCOUNT").size(9.0).strong().color(MUTED));
                ui.label(RichText::new("Connexion sécurisée · Aucun mot de passe stocké")
                    .size(10.0).color(Color32::from_gray(145)));
            });
            ui.add_space(28.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(74.0);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(8, 8, 8, 232))
                    .stroke(Stroke::new(1.0, Color32::from_gray(55)))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(28))
                    .show(ui, |ui| {
                        ui.set_min_width(310.0);
                        ui.label(RichText::new("BIENVENUE").size(9.0).strong().color(MUTED));
                        ui.add_space(12.0);
                        ui.label(RichText::new("Connectez-vous").size(25.0).strong().color(TEXT));
                        ui.add_space(7.0);
                        ui.label(RichText::new("Utilisez le compte Microsoft qui possède\nMinecraft: Java Edition.")
                            .size(11.0).color(MUTED));
                        ui.add_space(24.0);
                        let button = egui::Button::new(
                            RichText::new(if self.auth_busy { "Connexion en cours…" } else { "Continuer avec Microsoft  →" })
                                .size(12.0).strong().color(Color32::BLACK)
                        ).fill(Color32::WHITE).stroke(Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(7))
                            .min_size(Vec2::new(ui.available_width(), 46.0));
                        clicked = ui.add_enabled(!self.auth_busy, button).clicked();
                        if self.auth_busy {
                            ui.add_space(12.0);
                            ui.spinner();
                        }
                        if !self.status.is_empty() {
                            ui.add_space(12.0);
                            ui.label(RichText::new(&self.status).size(10.0).color(Color32::from_rgb(235, 135, 135)));
                        }
                        ui.add_space(17.0);
                        ui.label(RichText::new("AUTHENTIFICATION OFFICIELLE MICROSOFT")
                            .size(8.0).color(Color32::from_gray(105)));
                    });
            });
        });
        if clicked {
            self.auth_busy = true;
            self.status.clear();
            let tx = self.sender.clone();
            thread::spawn(move || match auth::sign_in() {
                Ok(profile) => { let _ = tx.send(Event::Authenticated(profile)); }
                Err(error) => { let _ = tx.send(Event::AuthFailed(error)); }
            });
        }
    }

    fn installer_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        draw_voxel_backdrop(ui, ui.max_rect(), 0.35);
        ui.vertical_centered(|ui| {
            ui.add_space(64.0);
            ui.label(RichText::new("M O K Y   S T U D I O S").size(10.0).strong().color(MUTED));
            ui.add_space(20.0);
            ui.label(RichText::new("Votre prochain monde\ncommence ici.").size(34.0).strong().color(TEXT));
            ui.add_space(12.0);
            ui.label(RichText::new("Installez Moky Launcher pour rejoindre vos serveurs.")
                .size(12.0).color(MUTED));
            ui.add_space(34.0);
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(8, 8, 8, 225))
                .stroke(Stroke::new(1.0, Color32::from_gray(55)))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.set_min_width(430.0);
                    ui.label(RichText::new("EMPLACEMENT D’INSTALLATION").size(8.0).strong().color(MUTED));
                    ui.add_space(7.0);
                    ui.label(RichText::new(self.install_path.display().to_string()).size(11.0).color(TEXT));
                });
            ui.add_space(20.0);
            if primary_button(ui, "INSTALLER MOKY LAUNCHER") { self.install(ctx); }
            if let Some(error) = &self.install_error {
                ui.label(RichText::new(error).size(10.0).color(Color32::LIGHT_RED));
            }
        });
    }

    fn installing_ui(&mut self, ui: &mut egui::Ui) {
        draw_voxel_backdrop(ui, ui.max_rect(), 0.28);
        ui.vertical_centered(|ui| {
            ui.add_space(110.0);
            ui.spinner();
            ui.add_space(20.0);
            ui.label(RichText::new("Installation en cours").size(25.0).strong().color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new("Préparation de votre espace de jeu…").size(11.0).color(MUTED));
            ui.add_space(22.0);
            ui.add(egui::ProgressBar::new(0.65).desired_width(340.0).fill(Color32::WHITE));
        });
    }

    fn launcher_ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        if self.profile.is_none() {
            self.screen = Screen::Auth;
            return;
        }

        let profile = self.profile.clone();
        let mut logout = false;
        let mut open_settings = false;
        let mut close_settings = false;
        let rect = ui.max_rect();
        draw_voxel_backdrop(ui, rect, 0.30);

        // Paladium-inspired composition: narrow icon rail, full-bleed game scene,
        // strong central title/action and a compact information strip.
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 224))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin { left: 9, right: 9, top: 18, bottom: 14 })
                .show(ui, |ui| {
                    ui.set_width(58.0);
                    ui.set_min_height(ui.available_height());
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("M").size(27.0).strong().color(TEXT));
                        ui.add_space(42.0);
                        if rail_item(ui, egui_material_icons::icons::ICON_HOME, !self.settings_open && !self.servers_open) {
                            self.settings_open = false;
                            self.servers_open = false;
                        }
                        ui.add_space(10.0);
                        if rail_item(ui, egui_material_icons::icons::ICON_APPS, self.servers_open) {
                            self.servers_open = true;
                            self.settings_open = false;
                        }
                        ui.add_space(10.0);
                        if rail_item(ui, egui_material_icons::icons::ICON_SETTINGS, self.settings_open) {
                            open_settings = true;
                        }
                        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                            if let Some(p) = profile.as_ref() {
                                ui.label(RichText::new(p.name.chars().take(1).collect::<String>().to_uppercase())
                                    .size(15.0).strong().color(TEXT));
                            }
                            ui.add_space(12.0);
                            ui.label(RichText::new(format!("v{}", self.version)).size(8.0).color(Color32::from_gray(115)));
                        });
                    });
                });

            ui.add_space(22.0);
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.add_space(18.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new(if self.settings_open { "PARAMÈTRES" } else if self.servers_open { "VOS SERVEURS" } else { "MOKY STUDIOS" })
                        .size(10.0).strong().color(Color32::from_gray(225)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(p) = profile.as_ref() {
                            ui.label(RichText::new(&p.name).size(10.0).strong().color(TEXT));
                            ui.label(egui_material_icons::icons::ICON_ACCOUNT_CIRCLE);
                        }
                    });
                });

                if self.settings_open {
                    ui.add_space(42.0);
                    ui.label(RichText::new("Paramètres").size(34.0).strong().color(TEXT));
                    ui.add_space(22.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 222))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(22))
                        .show(ui, |ui| {
                            ui.set_min_width(500.0);
                            ui.label(RichText::new("COMPTE MINECRAFT").size(9.0).strong().color(MUTED));
                            ui.add_space(12.0);
                            if let Some(p) = profile.as_ref() {
                                ui.label(RichText::new(&p.name).size(20.0).strong().color(TEXT));
                                ui.label(RichText::new(&p.id).size(10.0).color(MUTED));
                                ui.add_space(18.0);
                                if primary_button(ui, "SE DÉCONNECTER") { logout = true; }
                            }
                        });
                    ui.add_space(12.0);
                    close_settings = ui.button("←  Retour à l’accueil").clicked();
                } else if self.servers_open {
                    ui.add_space(42.0);
                    ui.label(RichText::new("Vos serveurs").size(36.0).strong().color(TEXT));
                    ui.add_space(12.0);
                    ui.label(RichText::new("Tous les univers Moky Studios, réunis au même endroit.")
                        .size(13.0).color(Color32::from_gray(210)));
                    ui.add_space(34.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 218))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(24))
                        .show(ui, |ui| {
                            ui.set_min_width(540.0);
                            ui.label(RichText::new("BIENTÔT DISPONIBLE").size(9.0).strong().color(MUTED));
                            ui.add_space(10.0);
                            ui.label(RichText::new("Les serveurs Moky Studios seront affichés ici.")
                                .size(17.0).strong().color(TEXT));
                            ui.add_space(6.0);
                            ui.label(RichText::new("Aucune configuration de serveur n’est encore intégrée.")
                                .size(11.0).color(MUTED));
                        });
                } else {
                    ui.add_space(62.0);
                    ui.label(RichText::new("VOTRE PROCHAINE AVENTURE").size(10.0).strong().color(Color32::from_gray(220)));
                    ui.add_space(12.0);
                    ui.label(RichText::new("MOKY STUDIOS").size(54.0).strong().color(TEXT));
                    ui.add_space(7.0);
                    ui.label(RichText::new("Un launcher. Plusieurs mondes. À vous de jouer.")
                        .size(15.0).color(Color32::from_gray(225)));
                    ui.add_space(32.0);

                    ui.horizontal(|ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 220))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin { left: 16, right: 18, top: 12, bottom: 12 })
                            .show(ui, |ui| {
                                ui.set_min_width(310.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui_material_icons::icons::ICON_APPS);
                                    ui.add_space(8.0);
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("SERVEUR SÉLECTIONNÉ").size(8.0).strong().color(MUTED));
                                        ui.label(RichText::new("Aucun serveur disponible").size(13.0).strong().color(TEXT));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(egui_material_icons::icons::ICON_EXPAND_MORE);
                                    });
                                });
                            });
                        ui.add_space(10.0);
                        let play = egui::Button::new(RichText::new("▶   JOUER").size(14.0).strong().color(Color32::BLACK))
                            .fill(Color32::WHITE)
                            .stroke(Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(7))
                            .min_size(Vec2::new(190.0, 58.0));
                        ui.add_enabled(false, play);
                    });

                    ui.add_space(68.0);
                    ui.horizontal(|ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 218))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(16))
                            .show(ui, |ui| {
                                ui.set_min_width(350.0);
                                ui.label(RichText::new("DERNIÈRES ACTUALITÉS").size(9.0).strong().color(MUTED));
                                ui.add_space(12.0);
                                ui.label(RichText::new("Les actualités Moky Studios arriveront ici.")
                                    .size(12.0).color(TEXT));
                            });
                        ui.add_space(12.0);
                        egui::Frame::new()
                            .fill(Color32::from_rgba_unmultiplied(5, 5, 5, 218))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(16))
                            .show(ui, |ui| {
                                ui.set_min_width(260.0);
                                ui.label(RichText::new("MOKY STUDIOS").size(9.0).strong().color(MUTED));
                                ui.add_space(12.0);
                                ui.label(RichText::new("Un seul endroit pour retrouver tous nos serveurs.")
                                    .size(12.0).color(TEXT));
                            });
                    });
                }
            });
        });

        if open_settings { self.settings_open = true; }
        if close_settings { self.settings_open = false; }
        if logout {
            let _ = auth::sign_out();
            self.profile = None;
            self.session_checked = true;
            self.screen = Screen::Auth;
            self.settings_open = false;
            self.servers_open = false;
        }
    }
}

fn rail_item(ui: &mut egui::Ui, icon: &str, selected: bool) -> bool {
    let button = egui::Button::new(RichText::new(icon).size(21.0)
        .color(if selected { TEXT } else { MUTED }))
        .fill(if selected { Color32::from_gray(48) } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(9))
        .min_size(Vec2::new(42.0, 42.0));
    ui.add(button).clicked()
}

fn nav_item(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let button = egui::Button::new(RichText::new(label).size(11.0).strong()
        .color(if selected { TEXT } else { MUTED }))
        .fill(if selected { Color32::from_gray(38) } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(6))
        .min_size(Vec2::new(ui.available_width(), 36.0));
    ui.add(button).clicked()
}

fn draw_voxel_backdrop(ui: &mut egui::Ui, rect: egui::Rect, darkness: f32) {
    let painter = ui.painter();
    let width = rect.width();
    let height = rect.height();
    let block = (width / 58.0).clamp(10.0, 22.0);
    let columns = (width / block).ceil() as usize + 1;
    let ground_y = rect.top() + height * 0.67;

    painter.rect_filled(rect, 0.0, Color32::from_rgb(63, 72, 77));
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + height * 0.64)),
        0.0, Color32::from_rgb(83, 94, 100),
    );

    // Block-shaped clouds in the distance.
    for (cx, cy, span) in [(0.12, 0.18, 5), (0.40, 0.12, 4), (0.76, 0.22, 6), (0.91, 0.10, 3)] {
        let x = rect.left() + width * cx;
        let y = rect.top() + height * cy;
        for (dx, dy, w) in [(0, 1, 2), (1, 0, span), (2, 1, span - 1), (span - 1, 2, 2)] {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x + dx as f32 * block * 0.65, y + dy as f32 * block * 0.55),
                    Vec2::new(w as f32 * block * 0.65, block * 0.48),
                ),
                0.0, Color32::from_rgb(126, 135, 137),
            );
        }
    }

    // Distant stepped mountain silhouettes.
    let ridge = [
        15,16,17,18,19,20,20,21,22,23,24,24,23,22,21,20,19,18,17,16,
        15,14,13,12,12,13,14,15,16,17,18,19,20,21,22,23,22,21,20,19,
        18,17,16,15,14,13,12,11,12,13,14,15,16,17,18,17,16,15,14
    ];
    for col in 0..columns {
        let h = ridge[col % ridge.len()] as f32 * block * 0.48;
        let x = rect.left() + col as f32 * block;
        for row in 0..((h / block).ceil() as usize) {
            let y = ground_y - (row as f32 + 1.0) * block;
            let shade = if row == (h / block).floor() as usize {
                Color32::from_rgb(78, 91, 84)
            } else if (col + row) % 4 == 0 {
                Color32::from_rgb(66, 77, 78)
            } else {
                Color32::from_rgb(58, 69, 72)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), Vec2::splat(block + 0.5)),
                0.0, shade,
            );
        }
    }

    // Mid-ground forest: square canopies and straight trunks.
    for (col, height_blocks, canopy) in [(5, 5, 3), (12, 7, 4), (22, 5, 3), (39, 6, 4), (48, 7, 4), (54, 5, 3)] {
        let x = rect.left() + col as f32 * block;
        let trunk_top = ground_y - height_blocks as f32 * block;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x + block * 0.35, trunk_top), Vec2::new(block * 0.42, block * height_blocks as f32)),
            0.0, Color32::from_rgb(49, 43, 35),
        );
        let crown_y = trunk_top - canopy as f32 * block * 0.62;
        for (dx, dy, w) in [(-1, 1, 3), (0, 0, 3), (1, 1, 3), (0, 2, 4)] {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x + dx as f32 * block * 0.58, crown_y + dy as f32 * block * 0.52),
                    Vec2::new(w as f32 * block * 0.58, block * 0.72),
                ),
                0.0, Color32::from_rgb(47, 66, 52),
            );
        }
    }

    // Foreground terrain: grass surface over layered dirt blocks.
    let front_y = ground_y + block * 2.0;
    for col in 0..columns {
        let x = rect.left() + col as f32 * block;
        let bump = ((col * 7 + 3) % 4) as f32 * block * 0.22;
        let surface = front_y - bump;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, surface), Vec2::new(block + 0.5, block * 0.42)),
            0.0, Color32::from_rgb(68, 84, 59),
        );
        for row in 0..8 {
            let y = surface + block * 0.42 + row as f32 * block;
            if y > rect.bottom() { break; }
            let color = if row < 2 {
                Color32::from_rgb(67, 58, 45)
            } else {
                Color32::from_rgb(45, 48, 43)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), Vec2::splat(block + 0.5)),
                0.0, color,
            );
        }
    }

    // Darken the scene without flattening its block geometry.
    let alpha = (darkness.clamp(0.0, 1.0) * 205.0) as u8;
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, alpha));
    for i in 0..14 {
        let t = i as f32 / 14.0;
        let y = rect.top() + t * height * 0.7;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(rect.left(), y), Vec2::new(width, height * 0.7 / 14.0 + 1.0)),
            0.0, Color32::from_rgba_unmultiplied(0, 0, 0, ((1.0 - t) * 30.0) as u8),
        );
    }
}

fn center_window(ctx: &egui::Context, size: Vec2) {
    let position = ctx.input(|input| input.viewport().monitor_size.map(|monitor| {
        egui::pos2(((monitor.x - size.x) * 0.5).max(0.0), ((monitor.y - size.y) * 0.5).max(0.0))
    }));
    if let Some(position) = position {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
    }
}

fn is_installed() -> bool {
    let path = installation_path().join("MokyLauncher.exe");
    env::current_exe().ok().and_then(|p| p.canonicalize().ok())
        == path.canonicalize().ok()
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
    use std::os::windows::process::CommandExt;
    let status = Command::new("powershell")
        .creation_flags(0x08000000)
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
    let releases: Vec<Release> = client
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=1"))
        .send()?
        .error_for_status()?
        .json()?;
    let release = releases.into_iter().next()
        .ok_or("Aucune release n’est encore disponible sur GitHub")?;

    let latest = semver::Version::parse(release.tag_name.trim_start_matches('v'))?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    if latest <= current {
        return Ok(None);
    }

    let _ = sender.send(Event::Downloading(latest.to_string()));
    let asset = release.assets.iter().find(|asset| asset.name == ASSET_NAME)
        .ok_or("L’installateur Windows est absent de la dernière release GitHub")?;
    let mut response = client.get(&asset.browser_download_url).send()?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let mut temporary_file = tempfile::Builder::new()
        .prefix("moky-launcher-update-").suffix(".exe").tempfile()?;
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 { break; }
        temporary_file.write_all(&buffer[..count])?;
        downloaded += count as u64;
        let progress = if total > 0 { downloaded as f32 / total as f32 } else { 0.0 };
        let _ = sender.send(Event::DownloadProgress(latest.to_string(), progress.clamp(0.0, 1.0)));
    }
    temporary_file.flush()?;
    if downloaded == 0 || (total > 0 && downloaded != total) {
        return Err("Le téléchargement du programme est incomplet.".into());
    }
    let mut signature = [0u8; 2];
    use std::io::{Seek, SeekFrom};
    temporary_file.as_file_mut().seek(SeekFrom::Start(0))?;
    temporary_file.as_file_mut().read_exact(&mut signature)?;
    if &signature != b"MZ" {
        return Err("Le fichier téléchargé n’est pas un exécutable Windows valide.".into());
    }
    self_replace::self_replace(temporary_file.path())?;
    Ok(Some(latest.to_string()))
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

    let startup_window = is_installed() && !env::args().any(|arg| arg == "--launch");
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size(if startup_window { [420.0, 190.0] } else { [1080.0, 720.0] })
            .with_min_inner_size(if startup_window { [420.0, 190.0] } else { [900.0, 650.0] })
            .with_resizable(false)
            .with_title(if startup_window { "Moky Launcher — Mise à jour" } else { "Moky Launcher — Moky Studios" }),
        ..Default::default()
    };

    eframe::run_native(
        "Moky Launcher",
        options,
        Box::new(|cc| { egui_material_icons::initialize(&cc.egui_ctx); Ok(Box::new(MokyApp::new())) }),
    )
}
