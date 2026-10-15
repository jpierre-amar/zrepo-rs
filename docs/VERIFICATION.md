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

La [CI du commit `0eefb05`](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38625130898)
est verte : formatage, Clippy, tests et build natifs sur Linux, macOS et
Windows, ainsi que les installeurs sur les trois OS. La publication d'un tag
doit attendre une CI verte sur le commit à publier.

## Reprise de la préparation de release

Le 14 octobre 2026, le dépôt ne comporte encore aucune release et aucun
secret Actions `ZREPO_RS_GITHUB_CLIENT_ID`. L'accès navigateur n'est pas
disponible dans l'environnement de travail pour enregistrer l'application OAuth.

Un cas propre à GitHub Actions a été corrigé : un secret absent y est injecté
comme une chaîne vide. Cette valeur désactive désormais la connexion par le
navigateur, comme une variable absente. Un test couvre ces deux cas et un
Client ID non vide. La suite compte désormais 38 tests sous Unix.

Avant la première release :

1. ~~Enregistrer l'application OAuth `zrepo-rs` avec Device Flow activé, puis
   configurer son Client ID selon le README (aucun client secret nécessaire).~~
   Fait le 15 octobre 2026 (#6) : le secret Actions `ZREPO_RS_GITHUB_CLIENT_ID`
   est posé ; `POST https://github.com/login/device/code` avec ce Client ID et le
   scope du code (`repo read:org`) renvoie un `device_code` ; un build release
   local sous Windows embarque le Client ID et l'assistant (étape 3 « GitHub »)
   propose « Copier le code et ouvrir github.com ». Le Client ID n'apparaît dans
   aucun fichier versionné.
2. Vérifier la CI du commit à publier, puis pousser uniquement le tag `v0.2.0`.
3. Vérifier les cinq archives publiées et leurs sommes, puis tester l'installation
   réelle avec des chemins HOME/XDG temporaires.
4. Valider manuellement le Device Flow et l'ouverture d'un projet dans l'éditeur.

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
