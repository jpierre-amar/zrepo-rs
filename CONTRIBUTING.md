# Contribuer à zrepo-rs / Contributing to zrepo-rs

[Français](#français) · [English](#english)

## Français

Merci de contribuer à zrepo-rs. Les signalements de bugs, propositions et
correctifs sont bienvenus.

### Avant de commencer

- Cherchez dans les [issues existantes](https://github.com/jpierre-amar/zrepo-rs/issues)
  pour éviter les doublons. Pour un changement important, ouvrez d'abord une issue
  afin de discuter du besoin et de l'approche.
- Pour un bug, indiquez la version de zrepo-rs, le système d'exploitation, les
  étapes de reproduction, le résultat attendu et le résultat observé. Masquez les
  jetons, noms d'hôtes et chemins personnels dans les journaux et captures.
- Pour une vulnérabilité, suivez [SECURITY.md](SECURITY.md) et utilisez le
  signalement privé ; ne publiez pas les détails dans une issue.

### Développer et proposer un correctif

1. Créez une branche depuis `main`. Consultez [le guide de développement](docs/DEVELOPMENT.md)
   pour l'organisation du code et les dépendances propres à chaque plateforme.
2. Gardez la modification ciblée. Préservez la coexistence avec l'ancien `zrepo` :
   son exécutable, ses fichiers et ses raccourcis ne doivent pas être modifiés.
3. Ajoutez ou adaptez les tests lorsqu'un comportement change. Avant une pull
   request de code, lancez au minimum :

   ```sh
   cargo fmt --all -- --check
   cargo check --workspace --all-targets
   cargo test --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```

4. Dans la pull request, décrivez le problème, la solution, les plateformes
   vérifiées et les contrôles exécutés. Liez l'issue correspondante, s'il y en a
   une. Les modifications de documentation seules n'exigent pas la suite Rust.

La CI vérifie le projet sur Linux, macOS et Windows. Les contributions intégrées
au dépôt sont distribuées sous sa [licence MIT](LICENSE) ; ne soumettez que du
contenu que vous avez le droit de partager sous cette licence.

## English

Thanks for contributing to zrepo-rs. Bug reports, ideas, and fixes are welcome.

### Before you start

- Search the [existing issues](https://github.com/jpierre-amar/zrepo-rs/issues)
  to avoid duplicates. For a substantial change, open an issue first to discuss
  the need and the approach.
- For a bug, include the zrepo-rs version, operating system, reproduction steps,
  expected result, and actual result. Redact tokens, hostnames, and personal paths
  from logs and screenshots.
- For a vulnerability, follow [SECURITY.md](SECURITY.md) and use private
  reporting; do not publish details in an issue.

### Develop and submit a fix

1. Create a branch from `main`. See the [development guide](docs/DEVELOPMENT.md)
   for the code layout and platform-specific dependencies.
2. Keep the change focused. Preserve coexistence with the older `zrepo` app:
   do not modify its executable, files, or shortcuts.
3. Add or update tests when behavior changes. Before opening a code pull request,
   run at least:

   ```sh
   cargo fmt --all -- --check
   cargo check --workspace --all-targets
   cargo test --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   ```

4. In the pull request, describe the problem, the solution, the platforms you
   checked, and the checks you ran. Link the related issue when there is one.
   Documentation-only changes do not need the Rust checks.

CI checks the project on Linux, macOS, and Windows. Contributions merged into
the repository are distributed under its [MIT license](LICENSE); submit only
content you have the right to share under that license.
