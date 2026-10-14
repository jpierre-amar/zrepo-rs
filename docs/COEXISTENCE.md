# Coexistence avec l'ancien `zrepo`

`zrepo-rs` est volontairement une application indépendante. Une installation
ou une désinstallation de `zrepo-rs` ne doit jamais modifier l'ancien `zrepo`.

## Espaces de noms réservés

| Élément | Nouvelle application |
|---|---|
| Exécutable | `zrepo-rs` (`zrepo-rs.exe` sous Windows) |
| Configuration | dossier système de l'application `zrepo-rs` |
| Cache | dossier système de l'application `zrepo-rs` |
| Trousseau | service `zrepo-rs`, compte `github` |
| Variables | préfixe `ZREPO_RS_` |
| Linux | `zrepo-rs.desktop`, icône `zrepo-rs` |
| macOS | `~/Applications/zrepo-rs.app`, bundle `io.github.jpierre-amar.zrepo-rs` |
| Windows | `%LOCALAPPDATA%\Programs\zrepo-rs`, raccourci `zrepo-rs.lnk` |
| Fenêtre | classe/app-id et titre `zrepo-rs` |

Les variables génériques de GitHub (`GITHUB_TOKEN`) restent prises en charge,
car elles décrivent l'accès GitHub et non l'identité de l'application.

## Raccourcis Hyprland

L'assistant ne reconnaît comme sien que l'exécutable `zrepo-rs`. Une commande
qui lance `zrepo`, même sur un chemin absolu, appartient à l'ancienne
application et n'est jamais remplacée. Si `SUPER + R` est déjà occupé,
l'assistant propose une combinaison alternative.

Toute modification reste précédée d'un aperçu, d'une validation explicite et
d'une sauvegarde horodatée. Les liens symboliques et les permissions sont
préservés.

## Vérification avant publication

Le test d'installation doit utiliser un `HOME` temporaire contenant un faux
`bin/zrepo`. Il doit aussi redéfinir `XDG_DATA_HOME`, `XDG_CONFIG_HOME` et
`XDG_CACHE_HOME` : ces variables peuvent pointer hors du `HOME` de test dans
une session graphique. Installer ensuite `zrepo-rs`, puis vérifier :

1. que le contenu et les métadonnées du faux `zrepo` n'ont pas changé ;
2. que `zrepo-rs` et uniquement `zrepo-rs` a été ajouté ;
3. qu'aucun fichier `zrepo.desktop`, `zrepo.app` ou `zrepo.lnk` n'a été créé ;
4. que les chemins de configuration et de cache restent distincts ;
5. que le test unitaire `hypr::tests::mot_zrepo` refuse explicitement `zrepo`.
