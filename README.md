# Moky Launcher

Launcher Minecraft officiel des différents serveurs de **Moky Studios**.

## État du projet

Le projet est écrit en **Rust**. Pour le moment, le dépôt fournit la base de compilation Windows x64 et le mécanisme de mise à jour automatique. Les fonctionnalités de lancement Minecraft seront ajoutées ensuite.

## Télécharger

- Les compilations de chaque commit sont disponibles dans l'onglet **Actions**, sous forme d'artefacts CI.
- Les versions distribuables sont publiées dans **Releases**.

## Mise à jour automatique

Au démarrage, le launcher consulte la dernière release publiée sur GitHub. Si une version plus récente existe, il télécharge `moky-launcher-windows-x86_64.exe` et remplace son propre exécutable.

Pour publier une version distribuable, crée et pousse un tag SemVer, par exemple :

```bash
git tag v0.1.1
git push origin v0.1.1
```

La CI compile alors le launcher et attache le fichier Windows à la release GitHub.

## Développement

```bash
cargo run
cargo build --release
```
