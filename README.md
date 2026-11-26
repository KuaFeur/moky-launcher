# Moky Launcher

Launcher Minecraft officiel des différents serveurs de **Moky Studios**, développé en Rust.

## Installation Windows

Téléchargez **MokyLauncher-Setup.exe** depuis la dernière [GitHub Release](https://github.com/KuaFeur/moky-launcher/releases).

L'installateur et l'application utilisent le même exécutable :
- première ouverture : installation graphique dans `%LOCALAPPDATA%\\Programs\\Moky Studios\\Moky Launcher` ;
- création des raccourcis dans le menu Démarrer et sur le Bureau ;
- ouvertures suivantes : une fenêtre de démarrage vérifie les mises à jour avant d'ouvrir l'interface.

## Mise à jour automatique

Chaque release doit contenir deux fichiers :
- `MokyLauncher-Setup.exe` : installation initiale ou réinstallation ;
- `MokyLauncher.exe` : binaire autonome utilisé par le mécanisme de mise à jour.

À chaque ouverture, le launcher compare sa version avec la dernière release GitHub. Si une version plus récente existe, il télécharge `MokyLauncher.exe`, vérifie que le fichier est complet et qu'il s'agit d'un exécutable Windows, remplace sa propre version puis redémarre. Si aucune mise à jour n'est disponible, ou si GitHub est inaccessible, l'interface principale s'ouvre normalement.

## CI et releases

- Chaque pull request compile le projet sur Windows x64.
- Chaque push sur `main` compile et publie l'artefact de build.
- Chaque tag au format `vX.Y.Z` publie les deux exécutables dans une GitHub Release.

Pour publier une version, mettre à jour le champ `version` de `Cargo.toml`, puis créer un tag identique préfixé par `v` (par exemple `v0.1.0-alpha.2`).

## Développement

```bash
cargo run
cargo build --release
```
