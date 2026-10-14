# Rapport de vérification avant publication

Date : 14 octobre 2026

Version : 0.2.0

Produit : `zrepo-rs`

Ce document conserve les preuves essentielles de la préparation du premier
push public. Il complète les procédures reproductibles de
[`DEVELOPMENT.md`](DEVELOPMENT.md).

## Critères fonctionnels

| Critère | Preuve | Résultat |
|---|---|---|
| L'ancien exécutable reste intact | SHA-256 avant/après de `~/.local/bin/zrepo` | `6bd55e72870f02d9d5387b9686f22a10134b747e209793fa03de217a21addc16`, inchangé |
| Installation côte à côte | archive release locale, `HOME` et chemins XDG temporaires, ancien binaire sentinelle | OK |
| Espaces de noms séparés | test des chemins binaire/config/cache/desktop/icône/keyring | OK |
| Raccourcis existants préservés | tests Lua et `hyprland.conf`, y compris toutes les touches candidates occupées | OK |
| CLI et logique | tests de parsing, ouverture, sources, cache et configuration | OK |
| Build optimisé | `cargo build --release --locked -p zrepo-rs` | OK, binaire Linux produit |

## Qualité et portabilité

| Contrôle | Résultat local |
|---|---|
| `cargo fmt --all -- --check` | OK |
| `cargo test --workspace --all-targets` | 37 tests réussis |
| `cargo clippy --workspace --all-targets -- -D warnings` | OK |
| `sh -n` et `dash -n` sur `install.sh` | OK |
| ShellCheck | OK |
| Analyse syntaxique PowerShell | OK |
| `desktop-file-validate` | OK |
| `actionlint` | OK |
| Contrat des cinq archives et sommes SHA-256 | OK |

Les exécutions natives Windows et macOS restent confiées à GitHub Actions ; la
publication d'un tag ne doit avoir lieu qu'après une CI verte sur les trois OS.

## Sécurité

- Le jeton GitHub est stocké sous le service de trousseau `zrepo-rs`.
- Le jeton de clonage n'apparaît ni dans les arguments, ni dans l'URL, ni dans
  `.git/config`.
- La pagination authentifiée refuse toute URL extérieure à
  `https://api.github.com/`.
- Sous Unix, les répertoires applicatifs écrits sont en `0700` et les fichiers
  atomiques de configuration/cache en `0600`.
- Toutes les Actions du dépôt sont épinglées par SHA complet.
- Le workflow de release refuse un tag hors de `main`, un écart tag/version,
  un échec qualité, une archive inattendue ou une somme incorrecte.
- Les sommes cohébergées détectent la corruption mais ne constituent pas une
  signature indépendante. Une attestation de provenance reste un durcissement
  possible pour une version ultérieure.

## Incidents de test et nettoyage

Deux simulations ont hérité d'un `XDG_DATA_HOME` absolu et créé uniquement les
nouveaux assets `zrepo-rs.desktop` et `zrepo-rs.png` dans le profil réel. Ils
ont été immédiatement déplacés hors du profil (récupérables), leur absence a
été vérifiée, et la CI fixe désormais explicitement `XDG_DATA_HOME`. Aucun
fichier de l'ancien `zrepo` n'a été modifié.
