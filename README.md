# Moky Launcher

Launcher Minecraft officiel des différents serveurs de **Moky Studios**, développé en Rust.

## Installateur Windows

La CI GitHub Actions compile un exécutable Windows x64 nommé **MokyLauncher-Setup.exe**.

Cet exécutable est à la fois l’installateur et l’application :
- première ouverture : interface d’installation graphique ;
- installation dans `%LOCALAPPDATA%\\Programs\\Moky Studios\\Moky Launcher` ;
- création de raccourcis dans le menu Démarrer et sur le Bureau ;
- lancement de l’application installée ;
- ouvertures suivantes : interface Moky Launcher et vérification des mises à jour GitHub.

L’interface utilise un thème sombre avec accents violets. Les fonctionnalités de sélection et de lancement des serveurs Minecraft seront ajoutées ensuite.

## CI et releases

- Chaque push sur `main` compile l’installateur.
- L’artefact CI est conservé pendant 30 jours.
- Chaque tag de version au format `vX.Y.Z` publie une GitHub Release contenant `MokyLauncher-Setup.exe`.

Pour publier une version, mettre à jour le champ `version` de `Cargo.toml`, puis créer un tag correspondant, par exemple `v0.1.1`.

## Développement

```bash
cargo run
cargo build --release
```
