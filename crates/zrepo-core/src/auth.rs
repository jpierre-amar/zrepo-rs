//! Acces GitHub sans gh : connexion par le navigateur (OAuth Device Flow),
//! jeton personnel colle, ou jeton de gh. Le jeton vit dans le trousseau du
//! systeme, jamais dans config.toml.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    error::{EX_AUTH, EX_FAIL, Fail, Res},
    net, sys,
};

/// Application OAuth de zrepo-rs, fixee a la compilation. Sans elle, seuls le
/// jeton personnel et celui de gh sont proposes.
pub const CLIENT_ID: Option<&str> = option_env!("ZREPO_RS_GITHUB_CLIENT_ID");

const KEYRING_SERVICE: &str = "zrepo-rs";
const KEYRING_USER: &str = "github";
const SCOPE: &str = "repo read:org";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    Env,
    Keyring,
    GhCli,
}

impl TokenSource {
    pub fn label(self) -> &'static str {
        match self {
            TokenSource::Env => "variable d'environnement",
            TokenSource::Keyring => "trousseau du système",
            TokenSource::GhCli => "gh auth token",
        }
    }
}

/// Une variable vide vaut une variable absente.
fn env_token() -> Option<String> {
    ["ZREPO_RS_GITHUB_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|t| t.trim().to_string())
        .find(|t| !t.is_empty())
}

fn keyring_entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
}

fn keyring_token() -> Option<String> {
    keyring_entry()
        .and_then(|e| e.get_password())
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Ordre : environnement, trousseau, puis gh s'il est installe et connecte.
pub fn token() -> Option<(String, TokenSource)> {
    if let Some(t) = env_token() {
        return Some((t, TokenSource::Env));
    }
    if let Some(t) = keyring_token() {
        return Some((t, TokenSource::Keyring));
    }
    gh_cli_token().map(|t| (t, TokenSource::GhCli))
}

pub fn store_token(t: &str) -> Res<()> {
    let t = t.trim();
    if t.is_empty() {
        return Err(Fail::new(EX_AUTH, "Jeton GitHub vide."));
    }
    forget_github_cache();
    keyring_entry()
        .and_then(|e| e.set_password(t))
        .map_err(|e| {
            Fail::new(
                EX_FAIL,
                format!(
                    "Impossible d'enregistrer le jeton dans le trousseau du système : {e}\n\
                     À défaut, définissez la variable ZREPO_RS_GITHUB_TOKEN."
                ),
            )
        })
}

/// Changement de compte : la liste GitHub en cache n'est plus la bonne.
/// (Le cache porte aussi l'empreinte du jeton : c'est une double securite.)
fn forget_github_cache() {
    let cache = crate::config::Config::default().cache_file;
    let _ = std::fs::remove_file(crate::sources::github_id_file(&cache));
    let _ = std::fs::remove_file(cache);
}

/// Supprimer un jeton absent n'est pas une erreur.
pub fn delete_token() -> Res<()> {
    forget_github_cache();
    match keyring_entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Fail::new(
            EX_FAIL,
            format!("Impossible de retirer le jeton du trousseau : {e}"),
        )),
    }
}

pub fn gh_cli_token() -> Option<String> {
    let out = sys::output_ok(sys::command("gh").args(["auth", "token"]))?;
    let t = String::from_utf8_lossy(&out).trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Login GitHub du jeton ; sert aussi a verifier qu'il est valide.
pub fn whoami(token: &str) -> Res<String> {
    #[derive(Deserialize)]
    struct User {
        login: String,
    }
    let mut resp = net::agent()
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(net::fail)?;
    match resp.status().as_u16() {
        200 => resp
            .body_mut()
            .read_json::<User>()
            .map(|u| u.login)
            .map_err(net::fail),
        401 => Err(Fail::new(
            EX_AUTH,
            "Jeton GitHub refusé (invalide, expiré ou révoqué).",
        )),
        s => {
            let msg = net::api_message(&mut resp);
            Err(Fail::new(
                EX_FAIL,
                format!("GitHub a répondu {s} : {msg}")
                    .trim_end_matches(" : ")
                    .to_string(),
            ))
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval: u64,   // secondes entre deux device_poll
    pub expires_in: u64, // secondes
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    Pending,
    /// GitHub demande de ralentir : ajouter 5 s a l'intervalle.
    SlowDown,
    Done(String),
}

/// Erreurs OAuth definitives (`{"error": ...}`).
fn oauth_fail(code: &str, desc: &str) -> Fail {
    match code {
        "expired_token" | "token_expired" => Fail::new(
            EX_AUTH,
            "Le code de connexion a expiré. Recommencez la connexion.",
        ),
        "access_denied" => Fail::new(EX_AUTH, "Connexion refusée sur GitHub."),
        "device_flow_disabled" => Fail::new(
            EX_FAIL,
            "La connexion par le navigateur n'est pas activée pour l'application GitHub de zrepo-rs.\n\
             Utilisez un jeton personnel ou celui de gh.",
        ),
        "incorrect_client_credentials" => Fail::new(
            EX_FAIL,
            "Identifiant d'application GitHub (client_id) invalide.",
        ),
        "incorrect_device_code" => Fail::new(
            EX_AUTH,
            "Code de connexion inconnu de GitHub. Recommencez la connexion.",
        ),
        _ if desc.is_empty() => Fail::new(EX_FAIL, format!("Erreur GitHub : {code}")),
        _ => Fail::new(EX_FAIL, format!("Erreur GitHub : {code} ({desc})")),
    }
}

/// Reponse JSON brute ; les erreurs OAuth arrivent avec un statut 200 et
/// sont interpretees par l'appelant.
fn oauth_post(url: &str, form: &[(&str, &str)]) -> Res<Value> {
    let mut resp = net::agent()
        .post(url)
        .header("Accept", "application/json")
        .send_form(form.iter().copied())
        .map_err(net::fail)?;
    let status = resp.status().as_u16();
    resp.body_mut().read_json().map_err(|e| {
        if status == 200 {
            net::fail(e)
        } else {
            Fail::new(EX_FAIL, format!("GitHub a répondu {status}."))
        }
    })
}

fn oauth_error(v: &Value) -> Option<(&str, &str)> {
    let code = v["error"].as_str()?;
    Some((code, v["error_description"].as_str().unwrap_or("")))
}

/// Premiere etape : le code a afficher et l'adresse ou le saisir.
pub fn device_start(client_id: &str) -> Res<DeviceCode> {
    let v = oauth_post(
        "https://github.com/login/device/code",
        &[("client_id", client_id), ("scope", SCOPE)],
    )?;
    if let Some((code, desc)) = oauth_error(&v) {
        return Err(oauth_fail(code, desc));
    }
    serde_json::from_value(v)
        .map_err(|e| Fail::new(EX_FAIL, format!("Réponse GitHub illisible : {e}")))
}

/// Un seul essai ; c'est a l'appelant d'attendre `interval` entre deux appels.
pub fn device_poll(client_id: &str, dc: &DeviceCode) -> Res<Poll> {
    let v = oauth_post(
        "https://github.com/login/oauth/access_token",
        &[
            ("client_id", client_id),
            ("device_code", &dc.device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )?;
    poll_result(&v)
}

fn poll_result(v: &Value) -> Res<Poll> {
    match oauth_error(v) {
        Some(("authorization_pending", _)) => Ok(Poll::Pending),
        Some(("slow_down", _)) => Ok(Poll::SlowDown),
        Some((code, desc)) => Err(oauth_fail(code, desc)),
        None => v["access_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(|t| Poll::Done(t.into()))
            .ok_or_else(|| Fail::new(EX_FAIL, "Réponse GitHub sans jeton.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erreurs_du_device_flow() {
        assert_eq!(oauth_fail("expired_token", "").code, EX_AUTH);
        assert_eq!(oauth_fail("access_denied", "x").code, EX_AUTH);
        assert!(oauth_fail("expired_token", "").msg.contains("expiré"));
        assert_eq!(
            oauth_fail("autre", "détail").msg,
            "Erreur GitHub : autre (détail)"
        );
    }

    #[test]
    fn reponses_du_polling() {
        let p = |v| poll_result(&v);
        let pending = serde_json::json!({"error": "authorization_pending"});
        assert_eq!(p(pending).ok(), Some(Poll::Pending));
        let slow = serde_json::json!({"error": "slow_down", "interval": 10});
        assert_eq!(p(slow).ok(), Some(Poll::SlowDown));
        let done = serde_json::json!({"access_token": "gho_x", "token_type": "bearer"});
        assert_eq!(p(done).ok(), Some(Poll::Done("gho_x".into())));
        let expired = serde_json::json!({"error": "expired_token"});
        assert_eq!(p(expired).err().map(|f| f.code), Some(EX_AUTH));
        let denied = serde_json::json!({"error": "access_denied"});
        assert!(p(denied).is_err());
    }

    #[test]
    fn reponse_device_code() {
        let v = serde_json::json!({
            "device_code": "dc", "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 899, "interval": 5
        });
        let dc: DeviceCode = serde_json::from_value(v).unwrap();
        assert_eq!(dc.user_code, "ABCD-1234");
        assert_eq!(dc.interval, 5);
    }
}
