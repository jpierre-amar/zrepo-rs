//! Détection des outils pour l'assistant (« Zed détecté ? »). Mêmes règles que
//! le lancement dans zrepo-core, en lecture seule.

use std::{env, path::PathBuf};

/// Premier exécutable du PATH portant ce nom (avec .exe sous Windows).
pub fn which(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    env::split_paths(&env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |e| dir.join(format!("{bin}{e}"))))
        .find(|p| p.is_file())
}

/// Où Zed a été trouvé, pour l'afficher.
pub fn zed() -> Option<String> {
    let names: &[&str] = if cfg!(any(windows, target_os = "macos")) {
        &["zed"]
    } else {
        &["zeditor", "zed"]
    };
    if let Some(p) = names.iter().find_map(|n| which(n)) {
        return Some(p.display().to_string());
    }
    let extra: Vec<PathBuf> = if cfg!(target_os = "macos") {
        let mut v = vec![PathBuf::from("/Applications/Zed.app")];
        if let Some(h) = env::var_os("HOME") {
            v.push(PathBuf::from(h).join("Applications/Zed.app"));
        }
        v
    } else if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(|l| vec![PathBuf::from(l).join(r"Programs\Zed\bin\zed.exe")])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    extra
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.display().to_string())
}

/// Herdr installé (indépendamment du réglage `herdr.enabled`) ? Jamais sous
/// Windows, où zrepo-rs ne le pilote pas.
pub fn herdr() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    which("herdr").map(|p| p.display().to_string())
}
