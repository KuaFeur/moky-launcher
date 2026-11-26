# Moky Launcher

Launcher Minecraft officiel des différents serveurs de **Moky Studios**.

## État du projet

Le projet est écrit en **Rust**. Pour le moment, le dépôt fournit la base de compilation Windows x64 et le mécanisme de mise à jour automatique. Les fonctionnalités de lancement Minecraft seront ajoutées ensuite.

## Télécharger

- Les compilations de chaque commit sont disponibles dans l'onglet **Actions**, sous forme d'artefacts CI.
- Les versions distribuables sont publiées dans **Releases**.

## Mise à jour automatique

Au démarrage, le launcher consulte la dernière release publiée sur GitHub. Si une version plus récente existe, il télécharge `moky-launcher-windows-x86_64.exe` et remplace son propre exécutable.

Pour publier une version distribuable, commence par mettre à jour `version` dans `Cargo.toml`, puis crée et pousse le tag correspondant. Les deux versions doivent être identiques :

```toml
version = "0.1.1"
```

```bash
git add Cargo.toml
git commit -m "chore: bump version to 0.1.1"
git tag v0.1.1
git push origin main --tags
```

La CI compile alors le launcher et attache le fichier Windows à la release GitHub. Le launcher compare cette version avec celle de son exécutable et remplace le binaire installé. Sous Windows, le remplacement se finalise à la fermeture du processus ; le prochain démarrage utilise donc la nouvelle version.

## Développement

```bash
cargo run
cargo build --release
```
