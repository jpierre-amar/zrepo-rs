//! Ouverture dans Herdr : nouvel espace sur le dossier du projet, avec
//! l'agent (codex --yolo par defaut) demarre dedans. Un projet distant va sur
//! sa machine Herdr ; rien n'est monte ni copie.

use std::{
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::Value;

use crate::{
    config::Config,
    error::{EX_DEPS, EX_HERDR, Fail, Res},
    sys,
};

/// Client de l'API Herdr ; `machine` vide = serveur local. C'est le seul
/// endroit qui sache ou parler.
struct Herdr {
    machine: Option<String>,
}

impl Herdr {
    fn cmd(&self) -> Command {
        let mut c = sys::command("herdr");
        if let Some(m) = &self.machine {
            c.args(["--machine", m]);
        }
        c
    }

    fn json(&self, args: &[&str]) -> Option<Value> {
        let out = sys::output_ok(self.cmd().args(args))?;
        serde_json::from_slice(&out).ok()
    }

    /// stdout et stderr reunis, pour pouvoir montrer l'erreur telle quelle.
    fn run(&self, args: &[&str]) -> (bool, String, String) {
        match self.cmd().args(args).stdin(Stdio::null()).output() {
            Ok(o) => (
                o.status.success(),
                String::from_utf8_lossy(&o.stdout).into_owned(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
            ),
            Err(e) => (false, String::new(), e.to_string()),
        }
    }
}

pub fn server_up() -> bool {
    sys::output_ok(sys::command("herdr").args(["workspace", "list"])).is_some()
}

pub fn machine_count() -> usize {
    sys::output_ok(sys::command("herdr").args(["machine", "list", "--json"]))
        .and_then(|o| serde_json::from_slice::<Value>(&o).ok())
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(0)
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Les cles REMOTE portent un alias ssh ; Herdr ne connait que les machines
/// enregistrees (`herdr machine add`).
fn machine_for_host(host: &str) -> Option<String> {
    let out = sys::output_ok(sys::command("herdr").args(["machine", "list", "--json"]))?;
    let v: Value = serde_json::from_slice(&out).ok()?;
    v.as_array()?
        .iter()
        .find(|m| m["target"].as_str() == Some(host) || m["label"].as_str() == Some(host))
        .and_then(|m| text(&m["label"]))
}

#[cfg(target_os = "linux")]
fn parent_pid(pid: u32) -> Option<u32> {
    use std::fs;
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Le nom du processus peut contenir des espaces : on lit apres la parenthese.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Clients Herdr (pas le serveur headless), par pid.
#[cfg(target_os = "linux")]
fn herdr_client_pids() -> Vec<u32> {
    use std::fs;
    let Ok(rd) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "herdr")
        })
        .filter(|pid| {
            let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            !cmdline.split(|&b| b == 0).skip(1).any(|a| a == b"server")
        })
        .collect()
}

/// Adresse Hyprland de la fenetre qui heberge un client Herdr : on remonte de
/// chaque client vers un pid connu de Hyprland. Un echec n'est jamais fatal.
#[cfg(target_os = "linux")]
fn window_address() -> Option<String> {
    if !sys::has("hyprctl") {
        return None;
    }
    let out = sys::output_ok(sys::command("hyprctl").args(["clients", "-j"]))?;
    let clients: Value = serde_json::from_slice(&out).ok()?;
    let clients = clients.as_array()?;
    for start in herdr_client_pids() {
        let mut pid = start;
        for _ in 0..8 {
            if pid <= 1 {
                break;
            }
            if let Some(c) = clients
                .iter()
                .find(|c| c["pid"].as_u64() == Some(pid.into()))
            {
                return text(&c["address"]);
            }
            match parent_pid(pid) {
                Some(p) => pid = p,
                None => break,
            }
        }
    }
    None
}

/// Pas de focus de fenetre hors Hyprland.
#[cfg(not(target_os = "linux"))]
fn window_address() -> Option<String> {
    None
}

/// Les lanceurs Omarchy, s'ils existent ; sinon c'est a l'utilisateur.
fn launch_window(progress: &dyn Fn(&str)) -> bool {
    if let Some(bin) = sys::which("omarchy-launch-terminal-herdr") {
        sys::spawn_detached(&bin, &[])
    } else if let Some(bin) = sys::which("omarchy-launch-tui") {
        sys::spawn_detached(&bin, &["herdr"])
    } else {
        progress("Aucun lanceur de terminal connu : ouvrez Herdr vous-même dans un terminal.");
        false
    }
}

/// Sans client attache, le serveur peut ne pas tourner du tout : la fenetre
/// est donc ouverte *avant* toute creation, puis on attend que l'API reponde.
fn ensure_window(cfg: &Config, progress: &dyn Fn(&str)) -> bool {
    if let Some(addr) = window_address() {
        let _ = sys::output_ok(sys::command("hyprctl").args([
            "dispatch",
            "focuswindow",
            &format!("address:{addr}"),
        ]));
        return true;
    }
    progress("Ouverture de Herdr…");
    if !launch_window(progress) {
        return false;
    }
    for _ in 0..cfg.herdr.wait {
        if server_up() {
            return true;
        }
        thread::sleep(Duration::from_secs(1));
    }
    server_up()
}

/// Nom d'agent Herdr : [a-z][a-z0-9_-]{0,31}, unique parmi les agents vivants.
fn agent_name(h: &Herdr, raw: &str) -> String {
    let cleaned: String = raw
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .skip_while(|c| !c.is_ascii_lowercase())
        .collect();
    let base: String = if cleaned.is_empty() {
        "projet".into()
    } else {
        cleaned.chars().take(32).collect()
    };

    let taken: Vec<String> = h
        .json(&["agent", "list"])
        .and_then(|v| {
            v["result"]["agents"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| text(&x["name"])).collect())
        })
        .unwrap_or_default();

    let mut cand = base.clone();
    let mut n = 2;
    while taken.contains(&cand) && n <= 99 {
        let suffix = format!("-{n}");
        cand = format!("{}{suffix}", &base[..base.len().min(32 - suffix.len())]);
        n += 1;
    }
    cand
}

/// Code d'une erreur JSON de Herdr (`{"error":{"code":...}}`), sur stdout ou stderr.
fn error_code(out: &str, err: &str) -> Option<String> {
    [out, err].into_iter().find_map(|s| {
        let v: Value = serde_json::from_str(s.trim()).ok()?;
        text(&v["error"]["code"])
    })
}

/// Regle de detection qui bloque l'agent, d'apres `herdr agent explain --json`.
fn blocking_rule(explain: &Value) -> Option<String> {
    text(&explain["matched_rule"]["id"])
}

fn not_ready_text(kind: &str, rule: Option<&str>, pane: &str, label: &str) -> String {
    match rule {
        Some("trust_directory") => {
            format!("Codex demande de faire confiance à ce dossier : répondez dans le pane {pane}.")
        }
        Some(r) => format!(
            "{kind} attend une réponse au démarrage (règle Herdr « {r} ») : \
             répondez dans le pane {pane}.\nS'il ne démarre pas, lancez-le à la main : {label}"
        ),
        None => format!(
            "{kind} attend une réponse au démarrage : regardez le pane {pane}.\n\
             S'il ne démarre pas, lancez-le à la main : {label}"
        ),
    }
}

/// L'agent attend une confirmation (« Trust this folder? »...) : on montre le
/// pane a l'utilisateur et on lui laisse la reponse. Rien n'est envoye au pane.
fn not_ready(h: &Herdr, cfg: &Config, agent: &str, pane: &str) -> String {
    let rule = [agent, pane].into_iter().find_map(|t| {
        h.json(&["agent", "explain", t, "--json"])
            .as_ref()
            .and_then(blocking_rule)
    });
    if !h.run(&["agent", "focus", agent]).0 {
        let _ = h.run(&["agent", "focus", pane]);
    }
    not_ready_text(&cfg.herdr.agent, rule.as_deref(), pane, &cfg.agent_label())
}

pub fn open(cfg: &Config, target: &str, progress: &dyn Fn(&str)) -> Res<()> {
    if !sys::has("herdr") {
        return Err(Fail::new(EX_DEPS, "herdr introuvable dans le PATH."));
    }
    let kind = cfg.herdr.agent.as_str();

    let (cwd, machine) = match target.strip_prefix("ssh://") {
        Some(rest) => {
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            let Some(m) = machine_for_host(host) else {
                return Err(Fail::new(
                    EX_HERDR,
                    format!(
                        "« {host} » n'est pas une machine enregistrée dans Herdr.\n\
                         Ajoutez-la puis réessayez : herdr machine add {host} --label \"{host}\""
                    ),
                ));
            };
            (format!("/{path}"), Some(m))
        }
        None => (target.to_string(), None),
    };
    let h = Herdr { machine };
    let name = cwd
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("projet")
        .to_string();

    // La fenetre d'abord : elle demarre le serveur si besoin. Son absence
    // n'empeche pas la creation, elle la rend seulement visible.
    ensure_window(cfg, progress);

    // Deja ouvert sur ce dossier : on y retourne plutot que d'empiler un 2e agent.
    let existing = h.json(&["pane", "list"]).and_then(|v| {
        v["result"]["panes"].as_array()?.iter().find_map(|p| {
            (p["agent"].as_str() == Some(kind) && p["cwd"].as_str() == Some(cwd.as_str()))
                .then(|| text(&p["workspace_id"]))
                .flatten()
        })
    });
    if let Some(ws) = existing {
        progress(&format!(
            "{kind} tourne déjà dans {cwd} — retour à l'espace {ws}."
        ));
        let _ = h.run(&["workspace", "focus", &ws]);
        return Ok(());
    }

    progress(&format!("Herdr : nouvel espace « {name} » dans {cwd}"));
    let (ok, out, err) = h.run(&[
        "workspace",
        "create",
        "--cwd",
        &cwd,
        "--label",
        &name,
        "--focus",
    ]);
    if !ok {
        return Err(Fail::new(
            EX_HERDR,
            format!("Herdr n'a pas pu créer l'espace :\n{out}{err}"),
        ));
    }
    let pane = serde_json::from_str::<Value>(&out)
        .ok()
        .and_then(|v| text(&v["result"]["root_pane"]["pane_id"]))
        .ok_or_else(|| {
            Fail::new(
                EX_HERDR,
                format!("Réponse inattendue de Herdr : {out}{err}"),
            )
        })?;

    // L'espace survit a un echec de demarrage : il reste utilisable a la main,
    // et rien ne doit etre ferme ici — Herdr ne reutilise pas les identifiants.
    let agent = agent_name(&h, &name);
    let mut args = vec![
        "agent", "start", &agent, "--kind", kind, "--pane", &pane, "--",
    ];
    args.extend(cfg.herdr.args.iter().map(String::as_str));
    let (ok, out, err) = h.run(&args);
    if !ok && error_code(&out, &err).as_deref() == Some("agent_not_ready") {
        return Err(Fail::new(EX_HERDR, not_ready(&h, cfg, &agent, &pane)));
    }
    if !ok {
        return Err(Fail::new(
            EX_HERDR,
            format!(
                "Espace créé, mais {kind} n'a pas démarré :\n{out}{err}\n\
                 À lancer à la main dans le pane {pane} : {}",
                cfg.agent_label()
            ),
        ));
    }
    progress(&format!(
        "{} démarré dans {pane} ({agent})",
        cfg.agent_label()
    ));
    if let Some(m) = &h.machine {
        progress(&format!(
            "Machine « {m} » : sélectionnez-la dans la barre latérale de Herdr."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erreur_agent_not_ready_reconnue() {
        let e =
            r#"{"error":{"code":"agent_not_ready","message":"agent x is blocked during startup"}}"#;
        assert_eq!(error_code("", e).as_deref(), Some("agent_not_ready"));
        assert_eq!(
            error_code(&format!("{e}\n"), "").as_deref(),
            Some("agent_not_ready")
        );
        assert_eq!(error_code("pas du json", "boom"), None);
        assert_eq!(error_code(r#"{"result":{}}"#, ""), None);
    }

    #[test]
    fn regle_bloquante_et_message() {
        let v: Value = serde_json::from_str(
            r#"{"agent":"codex","state":"blocked","matched_rule":{"id":"trust_directory","state":"blocked"}}"#,
        )
        .unwrap();
        let rule = blocking_rule(&v);
        assert_eq!(rule.as_deref(), Some("trust_directory"));
        assert_eq!(
            not_ready_text("codex", rule.as_deref(), "w3:p1", "codex --yolo"),
            "Codex demande de faire confiance à ce dossier : répondez dans le pane w3:p1."
        );
        assert_eq!(
            blocking_rule(&serde_json::json!({"state": "blocked"})),
            None
        );

        let other = not_ready_text("codex", Some("startup_update"), "w3:p1", "codex --yolo");
        assert!(other.contains("startup_update") && other.contains("w3:p1"));
        let unknown = not_ready_text("codex", None, "w3:p1", "codex --yolo");
        assert!(unknown.contains("w3:p1") && !unknown.contains('{'));
    }
}
