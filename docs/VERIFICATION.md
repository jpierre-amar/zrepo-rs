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

## Release v0.2.0

Publiée le 15 octobre 2026 depuis le tag `v0.2.0` (commit `6eb3aca`, CI verte :
[run 38740023944](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38740023944)).

| Contrôle | Résultat |
|---|---|
| [Run `release.yml` 38749253992](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38749253992) | `success` : verify, 5 builds, publication |
| Avertissement « Secret ZREPO_RS_GITHUB_CLIENT_ID absent » | aucun (annotations des 7 jobs vides sur ce point) |
| Statut de la release | ni brouillon ni pré-release, « Latest » |
| Assets | 18 : 5 archives, 5 `.sha256`, `SHA256SUMS`, `install.sh`, `install.ps1`, `zrepo-rs.desktop`, `.png`, `.svg`, `.ico`, `Info.plist.in` |
| `sha256sum -c SHA256SUMS` et les 5 `.sha256` | OK pour tous les fichiers |
| Client ID dans les binaires | présent dans les 5 archives |
| Binaire Windows publié (`x86_64-pc-windows-msvc`) | `--version` → `zrepo-rs 0.2.0`, code 0 |
| Installation isolée Linux et SHA-256 de l'ancien `zrepo` | **à faire** sur le poste Linux (issue #7, étapes 6 et 7) |

`install.ps1` n'est pas isolable comme `install.sh` : même avec `-Dir`, il
ajoute le dossier au `PATH` utilisateur (`HKCU\Environment`) et crée un
raccourci dans le menu Démarrer. Le binaire Windows a donc été testé directement
depuis l'archive ; l'installeur est couvert par le job CI
`installeurs (windows-latest)`.

## Release v0.3.0

Publiée le 16 octobre 2026 depuis le commit de fusion `79cd86e` de la PR #10.
La [CI sur `main`](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38880965760)
et le [workflow de publication](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38882106605)
ont réussi sur leurs plateformes respectives.

| Contrôle | Résultat |
|---|---|
| Release `v0.3.0` | publiée, stable et « Latest » |
| Artefacts | 20 fichiers : cinq archives, cinq sommes individuelles, setup Windows et sa somme, `SHA256SUMS`, installeurs et fichiers d'intégration |
| Sommes SHA-256 | les 19 fichiers listés dans `SHA256SUMS` ont été téléchargés et vérifiés indépendamment |
| Binaire Windows de l'archive | `--version` affiche `zrepo-rs 0.3.0` |
| Setup Windows | métadonnée `ProductVersion = 0.3.0` ; fabrication et test silencieux couverts par la CI |

Le test manuel du Device Flow et de l'ouverture dans l'éditeur sur Linux reste suivi dans l'issue #8.

## Release v0.3.1

Correctif du parcours des réglages : depuis le lanceur, Ctrl+, ouvre directement
« Cibles » et l'étape « Serveurs » permet de poursuivre vers « Cibles ».
Le [workflow de publication](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38883926175)
a réussi sur les cinq cibles. La release `v0.3.1` est stable et « Latest ».
Les 19 fichiers listés dans `SHA256SUMS` ont été téléchargés et vérifiés ;
le binaire Windows publié affiche `zrepo-rs 0.3.1` et a été installé localement
avec une empreinte SHA-256 identique à celle de l'archive.

## Release v0.3.2

Publiée le 16 octobre 2026 depuis le tag `v0.3.2` (commit `6b4ace3`).
La [CI du commit](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38912916226)
et le [workflow de publication](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38913192208)
ont réussi, y compris les builds et les contrôles d'installeurs sur Linux,
macOS et Windows. La [release](https://github.com/jpierre-amar/zrepo-rs/releases/tag/v0.3.2)
est stable et marquée « Latest ».

| Contrôle | Résultat |
|---|---|
| Qualité locale | `cargo check --workspace`, formatage, Clippy avec avertissements interdits, tests du workspace et `git diff --check` : OK |
| Artefacts | 20 fichiers publiés : cinq archives, cinq sommes individuelles, setup Windows et sa somme, `SHA256SUMS`, installeurs et fichiers d'intégration |
| Intégrité | les 19 fichiers de `SHA256SUMS` ont été téléchargés séparément ; `sha256sum -c SHA256SUMS` : OK pour chacun |
| Installation Linux isolée | `install.sh --yes --no-setup --version v0.3.2` sous Ubuntu avec HOME/XDG temporaires : archive vérifiée, `zrepo-rs --version` affiche `0.3.2`, `zrepo-rs update` confirme la version courante |
| Ancien `zrepo` | le fichier sentinelle placé à `~/.local/bin/zrepo` dans le HOME temporaire est resté intact |

Le lancement interactif du binaire Windows publié et les parcours graphiques
sur les trois plateformes restent à valider manuellement. L'archive et le setup
Windows ont passé le contrôle d'intégrité ci-dessus ; leur fabrication et les
tests d'installeur ont réussi en CI.

## Release v0.3.3

Publiée le 16 octobre 2026 depuis le tag `v0.3.3` (commit `61bb056`).
Cette version remplace le logo de l'application dans les ressources SVG, PNG et
ICO utilisées par les plateformes et l'installateur Windows. La
[CI sur `main`](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38916912196)
et le [workflow de publication](https://github.com/jpierre-amar/zrepo-rs/actions/runs/38917175493)
ont réussi. La [release](https://github.com/jpierre-amar/zrepo-rs/releases/tag/v0.3.3)
est stable et marquée « Latest ».

| Contrôle | Résultat |
|---|---|
| Qualité locale | `cargo check --workspace`, formatage, Clippy sans avertissement et 54 tests : OK |
| Logo | SVG valide, PNG RGBA 256 × 256 avec transparence, ICO contenant sept tailles de 16 à 256 px |
| Artefacts | 20 fichiers publiés, dont le setup Windows `zrepo-rs-setup-x86_64.exe` |
| Intégrité | les 19 fichiers de `SHA256SUMS` ont été téléchargés séparément ; `sha256sum -c SHA256SUMS` : OK pour chacun |

L'aspect du nouveau logo dans les raccourcis et les fenêtres des trois systèmes
reste à contrôler manuellement après installation.

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
