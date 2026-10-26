//! Raccourci clavier global : la combinaison choisie dans l'assistant.
//!
//! Forme texte, dans config.toml et pour l'agent : « Ctrl+Alt+R »,
//! « Alt+Space », « Super+Shift+F5 ». Super est la touche Cmd sous macOS, la
//! touche Windows ou SUPER ailleurs. Sous macOS, la touche désigne une position
//! du clavier (celle du A d'un clavier QWERTY est le Q d'un AZERTY) : le
//! libellé affiché est donc gardé à part (`[ui] shortcut_label`).

use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    /// « A » à « Z », « 0 » à « 9 », « Space », « F1 » à « F24 ».
    pub key: String,
}

/// Nom canonique d'une touche acceptée ; None sinon.
fn key_name(raw: &str) -> Option<String> {
    let k = raw.trim();
    let up = k.to_ascii_uppercase();
    if up.len() == 1 && up.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(up);
    }
    if up == "SPACE" {
        return Some("Space".into());
    }
    let n = up.strip_prefix('F')?.parse::<u8>().ok()?;
    (1..=24).contains(&n).then(|| format!("F{n}"))
}

impl Shortcut {
    /// Combinaison proposée tant que l'utilisateur n'en a pas choisi.
    pub fn suggested() -> Shortcut {
        let s = if cfg!(target_os = "macos") {
            "Alt+Space"
        } else if cfg!(windows) {
            "Ctrl+Alt+R"
        } else {
            "Super+R"
        };
        Shortcut::parse(s).unwrap_or_default()
    }

    /// Une touche de fonction peut se passer de modificateur, pas les autres.
    pub fn parse(s: &str) -> Option<Shortcut> {
        let mut parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = key_name(parts.pop()?)?;
        let mut sc = Shortcut {
            key,
            ..Shortcut::default()
        };
        for m in parts {
            let flag = match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut sc.ctrl,
                "alt" | "option" => &mut sc.alt,
                "shift" => &mut sc.shift,
                "super" | "cmd" | "command" | "win" => &mut sc.super_key,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        (sc.has_modifier() || sc.key.starts_with('F') && sc.key.len() > 1).then_some(sc)
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.super_key
    }

    fn mods(&self) -> impl Iterator<Item = &'static str> + '_ {
        [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.super_key, "Super"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
    }

    /// Libellé lisible, avec `key` à la place du nom de touche s'il est donné.
    pub fn label_with(&self, key: Option<&str>) -> String {
        let key = key.unwrap_or(if self.key == "Space" {
            "Espace"
        } else {
            &self.key
        });
        if cfg!(target_os = "macos") {
            // Ordre habituel de macOS : ⌃ ⌥ ⇧ ⌘.
            let mut v: Vec<&str> = [
                (self.ctrl, "⌃"),
                (self.alt, "⌥"),
                (self.shift, "⇧"),
                (self.super_key, "⌘"),
            ]
            .into_iter()
            .filter_map(|(on, s)| on.then_some(s))
            .collect();
            v.push(key);
            return v.join(" ");
        }
        let super_name = if cfg!(windows) { "Win" } else { "Super" };
        let mut v: Vec<&str> = self
            .mods()
            .map(|m| if m == "Super" { super_name } else { m })
            .collect();
        v.push(key);
        v.join(" + ")
    }

    pub fn label(&self) -> String {
        self.label_with(None)
    }

    fn hypr_mods(&self) -> Vec<&'static str> {
        [
            (self.super_key, "SUPER"),
            (self.ctrl, "CTRL"),
            (self.alt, "ALT"),
            (self.shift, "SHIFT"),
        ]
        .into_iter()
        .filter_map(|(on, s)| on.then_some(s))
        .collect()
    }

    /// Touche au sens de Hyprland (nom xkb, sans distinction de casse).
    pub fn hypr_key(&self) -> String {
        self.key.to_ascii_uppercase()
    }

    /// Configuration Lua : « SUPER + ALT + R ».
    pub fn hypr_lua(&self) -> String {
        let mut v = self.hypr_mods();
        let key = self.hypr_key();
        v.push(&key);
        v.join(" + ")
    }

    /// hyprland.conf : modificateurs « SUPER ALT », puis la touche à part.
    pub fn hypr_conf(&self) -> (String, String) {
        (self.hypr_mods().join(" "), self.hypr_key())
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in self.mods() {
            write!(f, "{m}+")?;
        }
        f.write_str(&self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::Shortcut;

    #[test]
    fn lecture_et_ecriture() {
        let s = Shortcut::parse("cmd + shift + r").unwrap();
        assert!(s.super_key && s.shift && !s.ctrl && !s.alt);
        assert_eq!(s.key, "R");
        assert_eq!(s.to_string(), "Shift+Super+R");
        assert_eq!(Shortcut::parse(&s.to_string()), Some(s));
        assert_eq!(
            Shortcut::parse("Alt+space").unwrap().to_string(),
            "Alt+Space"
        );
        assert_eq!(Shortcut::parse("F5").unwrap().key, "F5");
        assert_eq!(Shortcut::parse("Ctrl+7").unwrap().key, "7");
    }

    #[test]
    fn combinaisons_refusees() {
        // Une lettre seule capterait la frappe ordinaire.
        assert_eq!(Shortcut::parse("R"), None);
        assert_eq!(Shortcut::parse("Space"), None);
        assert_eq!(Shortcut::parse("Ctrl+Ctrl+R"), None);
        assert_eq!(Shortcut::parse("Hyper+R"), None);
        assert_eq!(Shortcut::parse("Ctrl+F25"), None);
        assert_eq!(Shortcut::parse("Ctrl+"), None);
        assert_eq!(Shortcut::parse(""), None);
    }

    #[test]
    fn formes_hyprland() {
        let s = Shortcut::parse("Alt+Super+Space").unwrap();
        assert_eq!(s.hypr_lua(), "SUPER + ALT + SPACE");
        assert_eq!(s.hypr_conf(), ("SUPER ALT".into(), "SPACE".into()));
        assert_eq!(Shortcut::parse("Super+R").unwrap().hypr_lua(), "SUPER + R");
    }

    #[test]
    fn libelle() {
        let s = Shortcut::parse("Ctrl+Alt+Space").unwrap();
        let l = s.label();
        assert!(l.contains("Espace"));
        assert!(s.label_with(Some("A")).ends_with('A'));
        assert!(Shortcut::suggested().has_modifier());
    }
}
