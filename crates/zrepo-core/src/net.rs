//! Client HTTP commun (ureq + rustls) : User-Agent, delais, classement des
//! erreurs. Les statuts HTTP sont rendus tels quels, a chaque appelant de
//! les interpreter.

use std::time::Duration;

use ureq::{Agent, Body, http::Response};

use crate::error::{EX_FAIL, EX_NET, Fail};

pub type Resp = Response<Body>;

pub fn agent() -> Agent {
    Agent::config_builder()
        .user_agent("zrepo-rs")
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Tout ce qui n'est pas une reponse lisible : reseau, DNS, TLS, delai.
pub fn fail(e: ureq::Error) -> Fail {
    match e {
        ureq::Error::Json(e) => Fail::new(EX_FAIL, format!("Réponse GitHub illisible : {e}")),
        e => Fail::new(
            EX_NET,
            format!("GitHub injoignable (pas de réseau ?) : {e}"),
        ),
    }
}

/// Le message d'erreur de l'API GitHub (`{"message": ...}`), s'il y en a un.
pub fn api_message(resp: &mut Resp) -> String {
    resp.body_mut()
        .read_json::<serde_json::Value>()
        .ok()
        .and_then(|v| v["message"].as_str().map(String::from))
        .unwrap_or_default()
}
