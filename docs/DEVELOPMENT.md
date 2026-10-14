# Développement et vérification

## Organisation

- `crates/zrepo-core` contient la logique sans interface graphique.
- `crates/zrepo-rs` contient le CLI, le lanceur egui et l'assistant.
- `install/` contient les installeurs sans privilèges administrateur.
- `packaging/` contient les intégrations de bureau.
- `.github/workflows/` contient la CI et la publication des releases.

L'architecture détaillée et le contrat public sont dans
[`ARCHITECTURE.md`](ARCHITECTURE.md). Les garanties d'isolation avec l'ancienne
application sont dans [`COEXISTENCE.md`](COEXISTENCE.md).

## Contrôles locaux

À exécuter avant chaque push :

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked -p zrepo-rs
sh -n install/install.sh
bash --posix -n install/install.sh
```

Si Docker et l'image PowerShell sont disponibles, l'analyse syntaxique de
l'installeur Windows peut être reproduite ainsi :

```sh
docker run --rm -v "$PWD:/work:ro" mcr.microsoft.com/powershell \
  pwsh -NoLogo -NoProfile -Command \
  '$e=$null; [System.Management.Automation.Language.Parser]::ParseFile("/work/install/install.ps1",[ref]$null,[ref]$e)>$null; if($e.Count){$e;exit 1}'
```

## Publication

1. Vérifier que la version de `Cargo.toml` correspond au tag prévu.
2. Configurer le secret Actions `ZREPO_RS_GITHUB_CLIENT_ID` si le Device Flow
   GitHub doit être disponible.
3. Exécuter tous les contrôles locaux et le test de coexistence.
4. Pousser le commit et attendre la CI sur Linux, macOS et Windows.
5. Créer et pousser le tag `vX.Y.Z` seulement après une CI verte.
6. Tester les deux installeurs depuis les artefacts de la release.

Pour tout test d'installeur Linux, isoler ensemble `HOME`, `XDG_DATA_HOME`,
`XDG_CONFIG_HOME` et `XDG_CACHE_HOME`. Isoler uniquement `HOME` ne suffit pas
si la session graphique exporte déjà un chemin XDG absolu.

Le workflow refuse un tag dont la version ne correspond pas au workspace et
publie les sommes SHA-256 des archives et des fichiers d'intégration.

## Retour arrière

Une release défectueuse doit être retirée ou remplacée par une version corrigée.
Les installeurs utilisent `releases/latest`, donc marquer la dernière version
saine comme « Latest » rétablit le canal par défaut. Cette opération n'affecte
jamais l'ancien `zrepo`, qui utilise un autre exécutable et d'autres chemins.
