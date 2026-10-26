//! Raccourci clavier sous Hyprland : ce que l'assistant propose d'écrire.
//!
//! Rien n'est écrit sans `apply`, appelé seulement après un clic explicite.
//! Chaque fichier modifié est d'abord copié en `<fichier>.bak.<horodatage>`.
//!
//! Deux formats : la configuration Lua (Hyprland récent, Omarchy :
//! `o.bind(...)`, `hl.window_rule{...}`) et l'ancien `hyprland.conf`
//! (`bind = ...`, `windowrule = ...`).

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use zrepo_core::shortcut::Shortcut;

/// Une ligne de l'aperçu : retirée, ajoutée, ou repère.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Removed(String),
    Added(String),
    Note(String),
}

#[derive(Clone, Debug)]
pub struct Change {
    pub file: PathBuf,
    pub old_text: String,
    pub new_text: String,
    pub preview: Vec<Line>,
}

#[derive(Clone, Debug, Default)]
pub struct Proposal {
    pub changes: Vec<Change>,
    /// Ce qui est déjà en place, ou les limites (conflit de touches...).
    pub notes: Vec<String>,
}

/// Sous Hyprland seulement (Linux, HYPRLAND_INSTANCE_SIGNATURE définie).
pub fn running() -> bool {
    cfg!(target_os = "linux") && env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn hypr_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("hypr"))
}

/// Ce qu'il faudrait modifier pour lancer `exe` avec la combinaison `sc` et
/// faire flotter la fenêtre. Ne modifie rien.
pub fn propose(exe: &Path, sc: &Shortcut) -> Result<Proposal, String> {
    let dir = hypr_dir().ok_or("Dossier de configuration introuvable.")?;
    let exe = exe.to_string_lossy();
    let lua = dir.join("hyprland.lua");
    let conf = dir.join("hyprland.conf");
    if lua.is_file() {
        let bindings = Some(dir.join("bindings.lua"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| lua.clone());
        let bind_text = read(&bindings)?;
        let rules_text = if bindings == lua {
            bind_text.clone()
        } else {
            read(&lua)?
        };
        let omarchy = bind_text.contains("o.bind(") || rules_text.contains("default.hypr.omarchy");
        let mut p = Proposal::default();
        let bind = lua_binding(&bind_text, &exe, omarchy, sc);
        push(&mut p, &bindings, &bind_text, bind);
        // Même fichier : la règle s'ajoute au texte déjà modifié.
        let base = match p.changes.iter().find(|c| c.file == lua) {
            Some(c) => c.new_text.clone(),
            None => rules_text,
        };
        let rules = lua_rules(&base);
        merge(&mut p, &lua, &base, rules);
        Ok(p)
    } else if conf.is_file() {
        let bindings = Some(dir.join("bindings.conf"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| conf.clone());
        let bind_text = read(&bindings)?;
        let mut p = Proposal::default();
        push(
            &mut p,
            &bindings,
            &bind_text,
            conf_binding(&bind_text, &exe, sc),
        );
        let base = match p.changes.iter().find(|c| c.file == conf) {
            Some(c) => c.new_text.clone(),
            None => read(&conf)?,
        };
        let rules = conf_rules(&base, new_rule_syntax());
        merge(&mut p, &conf, &base, rules);
        Ok(p)
    } else {
        Err(format!(
            "Ni hyprland.lua ni hyprland.conf dans {}.",
            dir.display()
        ))
    }
}

fn read(p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("Lecture impossible de {} : {e}", p.display()))
}

/// Résultat d'une étape : nouveau texte + aperçu, ou rien à faire (note).
enum Edit {
    Change(String, Vec<Line>),
    Nothing(String),
}

fn push(p: &mut Proposal, file: &Path, old: &str, edit: Edit) {
    match edit {
        Edit::Nothing(note) => p.notes.push(note),
        Edit::Change(new_text, preview) => p.changes.push(Change {
            file: file.to_path_buf(),
            old_text: old.to_string(),
            new_text,
            preview,
        }),
    }
}

/// Comme `push`, mais fusionne avec une modification déjà prévue du même fichier.
fn merge(p: &mut Proposal, file: &Path, base: &str, edit: Edit) {
    match (edit, p.changes.iter_mut().find(|c| c.file == file)) {
        (Edit::Change(new_text, preview), Some(c)) => {
            c.new_text = new_text;
            c.preview.extend(preview);
        }
        (edit, _) => push(p, file, base, edit),
    }
}

fn is_comment(line: &str, marker: &str) -> bool {
    line.trim_start().starts_with(marker)
}

/// Un mot de la commande désigne-t-il le binaire zrepo-rs (`zrepo-rs`,
/// `/chemin/zrepo-rs`, `uwsm-app -- zrepo-rs`...) ? « Projets (zrepo-rs) » non.
fn launches_zrepo(cmd: &str) -> bool {
    cmd.split_whitespace().any(|w| {
        let w = w.trim_matches(['"', '\'']);
        let base = w.rsplit(['/', '\\']).next().unwrap_or(w);
        base == "zrepo-rs" || base == "zrepo-rs.exe"
    })
}

/// Chaînes Lua entre guillemets doubles d'une ligne : (début, fin, contenu brut).
fn lua_strings(line: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-' && b.get(i + 1) == Some(&b'-') {
            break; // commentaire de fin de ligne
        }
        if b[i] == b'"' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < b.len() {
                out.push((start, i + 1, &line[start + 1..i]));
            }
        }
        i += 1;
    }
    out
}

fn lua_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Combinaison comparable : modificateurs triés, majuscules, sans espaces.
/// « SUPER + ALT + R » et « ALT+SUPER+r » donnent la même chose.
fn norm_keys(mods: &[&str], key: &str) -> String {
    let mut m: Vec<String> = mods
        .iter()
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    m.sort();
    m.push(key.trim().to_ascii_uppercase());
    m.join("+")
}

/// « SUPER + ALT + R » (Lua).
fn norm_lua(s: &str) -> String {
    let mut parts: Vec<&str> = s.split('+').collect();
    let key = parts.pop().unwrap_or("");
    norm_keys(&parts, key)
}

fn taken(sc: &Shortcut, file: &str) -> Edit {
    Edit::Nothing(format!(
        "{} est déjà utilisé dans {file} : choisissez une autre combinaison.",
        sc.label()
    ))
}

fn lua_binding(text: &str, exe: &str, omarchy: bool, sc: &Shortcut) -> Edit {
    let lines: Vec<&str> = text.lines().collect();
    let want = norm_lua(&sc.hypr_lua());
    // Ligne `bind` (hors commentaire) dont la combinaison est `want`, sauf `skip`.
    let used = |skip: Option<usize>| {
        lines.iter().enumerate().any(|(n, l)| {
            Some(n) != skip
                && !is_comment(l, "--")
                && l.contains("bind")
                && lua_strings(l)
                    .first()
                    .is_some_and(|(_, _, k)| norm_lua(k) == want)
        })
    };
    for (n, line) in lines.iter().enumerate() {
        if is_comment(line, "--") || !line.contains("bind") {
            continue;
        }
        let strings = lua_strings(line);
        let Some(&(s, e, cmd)) = strings.iter().find(|(_, _, c)| launches_zrepo(c)) else {
            continue;
        };
        let same_cmd = cmd == lua_quote(exe).trim_matches('"');
        // La combinaison est la première chaîne de la ligne.
        let key = strings.first().filter(|&&(ks, _, _)| ks != s);
        let same_key = key.is_none_or(|(_, _, k)| norm_lua(k) == want);
        if same_cmd && same_key {
            return Edit::Nothing(format!(
                "{} lance déjà {exe} (ligne {}).",
                sc.label(),
                n + 1
            ));
        }
        if !same_key && used(Some(n)) {
            return taken(sc, "la configuration de Hyprland");
        }
        // De droite à gauche : la commande d'abord, les positions de la clé restent justes.
        let mut new_line = format!("{}{}{}", &line[..s], lua_quote(exe), &line[e..]);
        if let Some(&(ks, ke, _)) = key.filter(|_| !same_key) {
            new_line = format!(
                "{}{}{}",
                &new_line[..ks],
                lua_quote(&sc.hypr_lua()),
                &new_line[ke..]
            );
        }
        let what = match (same_key, same_cmd) {
            (false, false) => "combinaison et commande remplacées",
            (false, true) => "combinaison remplacée",
            _ => "commande remplacée",
        };
        let preview = vec![
            Line::Note(format!("ligne {} : {what}", n + 1)),
            Line::Removed(line.to_string()),
            Line::Added(new_line.clone()),
        ];
        return Edit::Change(replace_line(text, n, &new_line), preview);
    }
    if used(None) {
        return taken(sc, "la configuration de Hyprland");
    }
    let key = sc.hypr_lua();
    let bind = if omarchy {
        format!(
            "o.bind({}, \"Projets (zrepo-rs)\", {})",
            lua_quote(&key),
            lua_quote(exe)
        )
    } else {
        format!(
            "hl.bind({}, hl.dsp.exec_cmd({}), {{ description = \"Projets (zrepo-rs)\" }})",
            lua_quote(&key),
            lua_quote(exe)
        )
    };
    let block = [
        "",
        "-- zrepo-rs : lanceur de projets (ajouté par zrepo-rs setup).",
        &bind,
    ];
    let mut preview = vec![Line::Note("ajouté à la fin du fichier".into())];
    preview.extend(block[1..].iter().map(|l| Line::Added(l.to_string())));
    Edit::Change(append(text, &block), preview)
}

const LUA_RULES: [&str; 8] = [
    "",
    "-- zrepo-rs : la fenêtre flotte, centrée et opaque ; le lanceur (titre « zrepo-rs »)",
    "-- dessine lui-même ses coins arrondis et son ombre (ajouté par zrepo-rs setup).",
    "hl.window_rule({",
    "    match = { class = \"^zrepo-rs$\" },",
    "    float = true, center = true, tag = \"-default-opacity\", opacity = \"1 1\",",
    "})",
    "hl.window_rule({ match = { class = \"^zrepo-rs$\", title = \"^zrepo-rs$\" }, border_size = 0, rounding = 0, no_shadow = true, no_blur = true })",
];

fn has_zrepo_rule(text: &str, comment: &str, words: &[&str]) -> bool {
    text.lines().any(|l| {
        !is_comment(l, comment) && l.contains("zrepo-rs") && words.iter().any(|w| l.contains(w))
    })
}

fn lua_rules(text: &str) -> Edit {
    if has_zrepo_rule(text, "--", &["window_rule", "o.window("]) {
        return Edit::Nothing(
            "La règle de fenêtre flottante pour zrepo-rs est déjà présente.".into(),
        );
    }
    let mut preview = vec![Line::Note("ajouté à la fin du fichier".into())];
    preview.extend(LUA_RULES[1..].iter().map(|l| Line::Added(l.to_string())));
    Edit::Change(append(text, &LUA_RULES), preview)
}

/// `bindd = SUPER ALT, R, …` : la combinaison comparable et la position de la
/// fin du champ de la touche (après sa virgule).
fn conf_keys(line: &str) -> Option<(String, usize)> {
    let (head, rest) = line.split_once('=')?;
    if !head.trim_start().starts_with("bind") {
        return None;
    }
    let mut fields = rest.splitn(3, ',');
    let mods = fields.next()?;
    let key = fields.next()?;
    fields.next()?;
    let end = head.len() + 1 + mods.len() + 1 + key.len() + 1;
    let mods: Vec<&str> = mods.split_whitespace().collect();
    Some((norm_keys(&mods, key), end))
}

fn conf_binding(text: &str, exe: &str, sc: &Shortcut) -> Edit {
    let lines: Vec<&str> = text.lines().collect();
    let (mods, key) = sc.hypr_conf();
    let want = norm_keys(&mods.split_whitespace().collect::<Vec<_>>(), &key);
    let used = |skip: Option<usize>| {
        lines
            .iter()
            .enumerate()
            .any(|(n, l)| Some(n) != skip && conf_keys(l).is_some_and(|(k, _)| k == want))
    };
    for (n, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !t.starts_with("bind") || !launches_zrepo(t.split_once('=').map_or("", |x| x.1)) {
            continue;
        }
        // `bindd = SUPER, R, Projets (zrepo-rs), exec, zrepo-rs` : tout ce qui suit `exec,`.
        let Some(pos) = line.find("exec,") else {
            continue;
        };
        let cmd = line[pos + 5..].trim();
        let same_cmd = cmd == exe;
        let current = conf_keys(line);
        let same_key = current.as_ref().is_none_or(|(k, _)| *k == want);
        if same_cmd && same_key {
            return Edit::Nothing(format!(
                "{} lance déjà {exe} (ligne {}).",
                sc.label(),
                n + 1
            ));
        }
        if !same_key && used(Some(n)) {
            return taken(sc, "la configuration de Hyprland");
        }
        let mut new_line = format!("{}exec, {exe}", &line[..pos]);
        if let Some((_, end)) = current.filter(|_| !same_key) {
            let head = &line[..line.find('=').unwrap_or(0)];
            new_line = format!("{head}= {mods}, {key},{}", &new_line[end..]);
        }
        let what = match (same_key, same_cmd) {
            (false, false) => "combinaison et commande remplacées",
            (false, true) => "combinaison remplacée",
            _ => "commande remplacée",
        };
        let preview = vec![
            Line::Note(format!("ligne {} : {what}", n + 1)),
            Line::Removed(line.to_string()),
            Line::Added(new_line.clone()),
        ];
        return Edit::Change(replace_line(text, n, &new_line), preview);
    }
    if used(None) {
        return taken(sc, "la configuration de Hyprland");
    }
    let bind = format!("bindd = {mods}, {key}, Projets (zrepo-rs), exec, {exe}");
    let block = [
        "",
        "# zrepo-rs : lanceur de projets (ajouté par zrepo-rs setup).",
        &bind,
    ];
    let mut preview = vec![Line::Note("ajouté à la fin du fichier".into())];
    preview.extend(block[1..].iter().map(|l| Line::Added(l.to_string())));
    Edit::Change(append(text, &block), preview)
}

/// Hyprland 0.53 a changé la syntaxe des règles de fenêtre.
fn new_rule_syntax() -> bool {
    let out = std::process::Command::new("hyprctl")
        .args(["version", "-j"])
        .output()
        .ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    // "tag": "v0.56.2-..." ; en cas de doute, la syntaxe récente.
    let minor = text.as_deref().and_then(|t| {
        let rest = &t[t.find("\"tag\"")?..];
        let v = &rest[rest.find("v0.")? + 3..];
        v.split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse::<u32>()
            .ok()
    });
    minor.is_none_or(|m| m >= 53)
}

fn conf_rules(text: &str, new_syntax: bool) -> Edit {
    if has_zrepo_rule(text, "#", &["windowrule"]) {
        return Edit::Nothing(
            "La règle de fenêtre flottante pour zrepo-rs est déjà présente.".into(),
        );
    }
    let rules: Vec<String> = if new_syntax {
        vec![
            "windowrule = float on, center on, opacity 1 1, match:class ^zrepo-rs$".into(),
            "windowrule = border_size 0, rounding 0, no_shadow on, no_blur on, match:class ^zrepo-rs$, match:title ^zrepo-rs$".into(),
        ]
    } else {
        let all = "class:^(zrepo-rs)$";
        let launcher = "class:^(zrepo-rs)$, title:^(zrepo-rs)$";
        vec![
            format!("windowrulev2 = float, {all}"),
            format!("windowrulev2 = center, {all}"),
            format!("windowrulev2 = noborder, {launcher}"),
            format!("windowrulev2 = rounding 0, {launcher}"),
            format!("windowrulev2 = noshadow, {launcher}"),
            format!("windowrulev2 = noblur, {launcher}"),
        ]
    };
    let mut block = vec![
        String::new(),
        "# zrepo-rs : la fenêtre flotte, centrée ; le lanceur dessine lui-même ses coins et son ombre.".to_string(),
    ];
    block.extend(rules);
    let refs: Vec<&str> = block.iter().map(String::as_str).collect();
    let mut preview = vec![Line::Note("ajouté à la fin du fichier".into())];
    preview.extend(block[1..].iter().map(|l| Line::Added(l.clone())));
    Edit::Change(append(text, &refs), preview)
}

fn replace_line(text: &str, n: usize, new_line: &str) -> String {
    let mut out: Vec<&str> = text.lines().collect();
    out[n] = new_line;
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn append(text: &str, block: &[&str]) -> String {
    let mut s = text.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    for l in block {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Écrit les modifications après une copie de sauvegarde horodatée de chaque
/// fichier. Refuse tout (rien n'est écrit) si l'un des fichiers a changé
/// depuis l'aperçu. Un lien symbolique (dotfiles gérés par stow...) est suivi :
/// c'est le fichier réel qui est réécrit, avec ses permissions d'origine.
/// Rend la liste des sauvegardes.
pub fn apply(p: &Proposal) -> Result<Vec<PathBuf>, String> {
    // 1. Tous les fichiers sont-ils encore tels qu'à l'aperçu ?
    let mut targets = Vec::new();
    for c in &p.changes {
        if read(&c.file)? != c.old_text {
            return Err(format!(
                "{} a été modifié depuis l'aperçu : rien n'a été écrit. Revenez à cette étape \
                 pour un nouvel aperçu.",
                c.file.display()
            ));
        }
        let real = fs::canonicalize(&c.file)
            .map_err(|e| format!("Chemin illisible {} : {e}", c.file.display()))?;
        targets.push(real);
    }
    // 2. Écriture, fichier par fichier.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backups = Vec::new();
    for (c, real) in p.changes.iter().zip(&targets) {
        let mut bak = PathBuf::from(format!("{}.bak.{stamp}", real.display()));
        let mut i = 1;
        while bak.exists() {
            bak = PathBuf::from(format!("{}.bak.{stamp}-{i}", real.display()));
            i += 1;
        }
        fs::copy(real, &bak)
            .map_err(|e| format!("Sauvegarde impossible de {} : {e}", real.display()))?;
        write_keeping_permissions(real, &c.new_text)
            .map_err(|e| format!("Écriture impossible de {} : {e}", real.display()))?;
        backups.push(bak);
    }
    Ok(backups)
}

/// Remplacement atomique (fichier temporaire dans le même dossier, puis
/// rename) qui garde les permissions du fichier remplacé.
fn write_keeping_permissions(real: &Path, text: &str) -> std::io::Result<()> {
    let perms = fs::metadata(real)?.permissions();
    let tmp = PathBuf::from(format!("{}.zrepo-rs-tmp", real.display()));
    let r = fs::write(&tmp, text)
        .and_then(|()| fs::set_permissions(&tmp, perms))
        .and_then(|()| fs::rename(&tmp, real));
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const OMARCHY: &str = "-- Add a new binding.\n\
-- o.bind(\"SUPER + SHIFT + R\", \"SSH\", \"alacritty -e ssh your-server\")\n\
o.bind(\"SUPER + R\", \"Projets (zrepo-rs)\", \"omarchy-launch-tui zrepo-rs\")\n\
hl.unbind(\"F9\")\n";

    fn sc(s: &str) -> Shortcut {
        Shortcut::parse(s).unwrap()
    }

    fn change(e: Edit) -> (String, Vec<Line>) {
        match e {
            Edit::Change(t, p) => (t, p),
            Edit::Nothing(n) => panic!("rien à faire : {n}"),
        }
    }

    #[test]
    fn omarchy_remplace_la_commande() {
        let (text, preview) = change(lua_binding(
            OMARCHY,
            "/home/u/.local/bin/zrepo-rs",
            true,
            &sc("Super+R"),
        ));
        assert!(text.contains(
            "o.bind(\"SUPER + R\", \"Projets (zrepo-rs)\", \"/home/u/.local/bin/zrepo-rs\")\n"
        ));
        assert!(!text.contains("omarchy-launch-tui zrepo-rs\")\n"));
        // Le commentaire d'exemple et la suite sont intacts.
        assert!(text.contains("-- o.bind(\"SUPER + SHIFT + R\""));
        assert!(text.ends_with("hl.unbind(\"F9\")\n"));
        assert_eq!(
            preview[0],
            Line::Note("ligne 3 : commande remplacée".into())
        );
        // Deuxième passage : déjà fait.
        assert!(matches!(
            lua_binding(&text, "/home/u/.local/bin/zrepo-rs", true, &sc("Super+R")),
            Edit::Nothing(_)
        ));
    }

    #[test]
    fn lua_sans_raccourci_existant() {
        let (text, _) = change(lua_binding(
            "hl.unbind(\"F9\")",
            "/opt/zrepo-rs",
            true,
            &sc("Super+R"),
        ));
        assert!(
            text.ends_with("o.bind(\"SUPER + R\", \"Projets (zrepo-rs)\", \"/opt/zrepo-rs\")\n")
        );
        let (text, _) = change(lua_binding(
            "hl.bind(\"SUPER + R\", hl.dsp.exec_cmd(\"rofi\"))\n",
            "/opt/zrepo-rs",
            false,
            &sc("Alt+Super+R"),
        ));
        assert!(text.contains(
            "hl.bind(\"SUPER + ALT + R\", hl.dsp.exec_cmd(\"/opt/zrepo-rs\"), { description = \"Projets (zrepo-rs)\" })"
        ));
    }

    #[test]
    fn regles_lua_une_seule_fois() {
        let (text, _) = change(lua_rules("require(\"hypr.autostart\")\n"));
        assert!(text.contains(
            "    float = true, center = true, tag = \"-default-opacity\", opacity = \"1 1\","
        ));
        assert!(matches!(lua_rules(&text), Edit::Nothing(_)));
    }

    #[test]
    fn ancien_format_conf() {
        let conf = "bindd = SUPER, R, Projets (zrepo-rs), exec, zrepo-rs\n";
        let (text, _) = change(conf_binding(conf, "/opt/zrepo-rs", &sc("Super+R")));
        assert_eq!(
            text,
            "bindd = SUPER, R, Projets (zrepo-rs), exec, /opt/zrepo-rs\n"
        );
        let (text, _) = change(conf_binding(
            "bind = SUPER, R, exec, wofi\n",
            "/opt/zrepo-rs",
            &sc("Super+Alt+R"),
        ));
        assert!(text.ends_with("bindd = SUPER ALT, R, Projets (zrepo-rs), exec, /opt/zrepo-rs\n"));
        let (text, _) = change(conf_rules("", true));
        assert!(
            text.contains("windowrule = float on, center on, opacity 1 1, match:class ^zrepo-rs$")
        );
        assert!(matches!(conf_rules(&text, true), Edit::Nothing(_)));
        let (text, _) = change(conf_rules("", false));
        assert!(text.contains("windowrulev2 = float, class:^(zrepo-rs)$"));
    }

    #[test]
    fn mot_zrepo() {
        assert!(launches_zrepo("omarchy-launch-tui zrepo-rs"));
        assert!(launches_zrepo("/home/u/.local/bin/zrepo-rs --refresh"));
        assert!(launches_zrepo("uwsm-app -- zrepo-rs"));
        assert!(!launches_zrepo("Projets (zrepo-rs)"));
        assert!(!launches_zrepo("zrepo-rs-old"));
        // L'ancienne application est indépendante : son raccourci ne doit
        // jamais être considéré comme celui de zrepo-rs ni être remplacé.
        assert!(!launches_zrepo("zrepo"));
        assert!(!launches_zrepo("/home/u/.local/bin/zrepo"));
    }

    #[test]
    fn coexistence_avec_ancien_raccourci_lua() {
        let old_bind = "o.bind(\"SUPER + R\", \"Ancien zrepo\", \"/home/u/.local/bin/zrepo\")\n";
        // La même touche que l'ancien zrepo : refusée, l'ancienne ligne reste.
        assert!(matches!(
            lua_binding(old_bind, "/opt/zrepo-rs", true, &sc("Super+R")),
            Edit::Nothing(_)
        ));
        let (with_new_bind, _) = change(lua_binding(
            old_bind,
            "/opt/zrepo-rs",
            true,
            &sc("Super+Alt+R"),
        ));
        assert!(with_new_bind.contains(old_bind));
        assert!(
            with_new_bind
                .contains("o.bind(\"SUPER + ALT + R\", \"Projets (zrepo-rs)\", \"/opt/zrepo-rs\")")
        );

        let old_rule = "hl.window_rule({ match = { class = \"^zrepo$\" }, float = true })\n";
        let (with_new_rule, _) = change(lua_rules(old_rule));
        assert!(with_new_rule.contains(old_rule));
        assert!(with_new_rule.contains("class = \"^zrepo-rs$\""));
    }

    #[test]
    fn coexistence_avec_ancien_raccourci_conf() {
        let old_bind = "bindd = SUPER, R, Ancien zrepo, exec, /home/u/.local/bin/zrepo\n";
        assert!(matches!(
            conf_binding(old_bind, "/opt/zrepo-rs", &sc("Super+R")),
            Edit::Nothing(_)
        ));
        let (with_new_bind, _) =
            change(conf_binding(old_bind, "/opt/zrepo-rs", &sc("Super+Alt+R")));
        assert!(with_new_bind.contains(old_bind));
        assert!(
            with_new_bind.contains("bindd = SUPER ALT, R, Projets (zrepo-rs), exec, /opt/zrepo-rs")
        );

        let old_rule = "windowrulev2 = float, class:^(zrepo)$\n";
        let (with_new_rule, _) = change(conf_rules(old_rule, false));
        assert!(with_new_rule.contains(old_rule));
        assert!(with_new_rule.contains("windowrulev2 = float, class:^(zrepo-rs)$"));
    }

    #[test]
    fn lua_refuse_une_combinaison_occupee() {
        let text = "o.bind(\"SUPER + R\", \"Ancien zrepo\", \"zrepo\")\n\
o.bind(\"SUPER + ALT + R\", \"Terminal\", \"alacritty\")\n\
o.bind(\"SUPER + CTRL + R\", \"Menu\", \"wofi\")\n";
        match lua_binding(text, "/opt/zrepo-rs", true, &sc("Ctrl+Super+R")) {
            Edit::Nothing(note) => assert!(note.contains("déjà utilisé")),
            Edit::Change(_, _) => panic!("un raccourci occupé ne doit pas être réécrit"),
        }
    }

    #[test]
    fn conf_refuse_une_combinaison_occupee() {
        let text = "bindd = SUPER, R, Ancien zrepo, exec, zrepo\n\
bindd = SUPER ALT, R, Terminal, exec, alacritty\n\
bindd = SUPER CTRL, R, Menu, exec, wofi\n";
        match conf_binding(text, "/opt/zrepo-rs", &sc("Super+Ctrl+R")) {
            Edit::Nothing(note) => assert!(note.contains("déjà utilisé")),
            Edit::Change(_, _) => panic!("un raccourci occupé ne doit pas être réécrit"),
        }
    }

    #[test]
    fn changement_de_combinaison() {
        let (text, preview) = change(lua_binding(
            OMARCHY,
            "omarchy-launch-tui zrepo-rs",
            true,
            &sc("Super+Shift+Space"),
        ));
        assert!(text.contains(
            "o.bind(\"SUPER + SHIFT + SPACE\", \"Projets (zrepo-rs)\", \"omarchy-launch-tui zrepo-rs\")\n"
        ));
        assert_eq!(
            preview[0],
            Line::Note("ligne 3 : combinaison remplacée".into())
        );
        // L'ordre des modificateurs ne compte pas.
        assert!(matches!(
            lua_binding(
                &text,
                "omarchy-launch-tui zrepo-rs",
                true,
                &sc("Shift+Super+Space")
            ),
            Edit::Nothing(_)
        ));

        let conf = "bindd = SUPER, R, Projets (zrepo-rs), exec, zrepo-rs\n";
        let (text, _) = change(conf_binding(conf, "/opt/zrepo-rs", &sc("Ctrl+Alt+P")));
        assert_eq!(
            text,
            "bindd = CTRL ALT, P, Projets (zrepo-rs), exec, /opt/zrepo-rs\n"
        );
    }

    #[test]
    fn application_avec_sauvegarde() {
        let dir = env::temp_dir().join(format!("zrepo-rs-hypr-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("bindings.lua");
        fs::write(&f, OMARCHY).unwrap();
        let (new_text, preview) =
            change(lua_binding(OMARCHY, "/opt/zrepo-rs", true, &sc("Super+R")));
        let p = Proposal {
            changes: vec![Change {
                file: f.clone(),
                old_text: OMARCHY.into(),
                new_text: new_text.clone(),
                preview,
            }],
            notes: Vec::new(),
        };
        let baks = apply(&p).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), new_text);
        assert_eq!(fs::read_to_string(&baks[0]).unwrap(), OMARCHY);
        // Le fichier a changé depuis l'aperçu : refus.
        assert!(apply(&p).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    fn change_of(file: &Path, old: &str, new: &str) -> Change {
        Change {
            file: file.to_path_buf(),
            old_text: old.into(),
            new_text: new.into(),
            preview: Vec::new(),
        }
    }

    #[test]
    fn rien_n_est_ecrit_si_un_seul_fichier_a_change() {
        let dir = env::temp_dir().join(format!("zrepo-rs-hypr-tous-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.lua"), dir.join("b.lua"));
        fs::write(&a, "a\n").unwrap();
        fs::write(&b, "b modifié ailleurs\n").unwrap();
        let p = Proposal {
            changes: vec![change_of(&a, "a\n", "A\n"), change_of(&b, "b\n", "B\n")],
            notes: Vec::new(),
        };
        assert!(apply(&p).is_err());
        assert_eq!(fs::read_to_string(&a).unwrap(), "a\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2); // aucune sauvegarde
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn lien_symbolique_et_permissions_conserves() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = env::temp_dir().join(format!("zrepo-rs-hypr-lien-{}", std::process::id()));
        fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let real = dir.join("dotfiles/bindings.lua");
        let link = dir.join("bindings.lua");
        fs::write(&real, "x\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&real, &link).unwrap();
        let p = Proposal {
            changes: vec![change_of(&link, "x\n", "y\n")],
            notes: Vec::new(),
        };
        let baks = apply(&p).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "y\n");
        assert_eq!(fs::read_to_string(&link).unwrap(), "y\n");
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // macOS canonicalise /var en /private/var : comparer deux chemins
        // canoniques pour vérifier que la sauvegarde reste près de la cible.
        let real_parent = fs::canonicalize(dir.join("dotfiles")).unwrap();
        assert!(baks[0].starts_with(real_parent));
        fs::remove_dir_all(&dir).unwrap();
    }
}
