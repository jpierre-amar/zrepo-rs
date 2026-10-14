//! zrepo-rs — lanceur de projets (local, distant ou GitHub) vers Zed ou Herdr.
//!
//! Sans argument : la fenêtre de lancement (l'assistant d'abord s'il n'y a pas
//! encore de config.toml). Les sous-commandes texte gardent les codes de sortie
//! de la v1.

// Pas de console noire derrière la fenêtre sous Windows ; les sous-commandes
// texte se rattachent à celle du terminal parent (`attach_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod detect;
mod gui;
mod hypr;

use std::process::ExitCode;

use zrepo_core::error::{EX_FAIL, EX_OK};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse(&args) {
        Ok(cmd) => run(cmd),
        Err(f) => {
            attach_console();
            eprintln!("{}", f.msg);
            f.code
        }
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}

fn run(cmd: cli::Command) -> i32 {
    use cli::Command;
    let result = match cmd {
        Command::Launcher { refresh, target } => {
            return gui::run(gui::Mode::Launcher { refresh, target });
        }
        Command::Setup => return gui::run(gui::Mode::Setup),
        Command::List { refresh } => {
            attach_console();
            cli::list(refresh)
        }
        Command::Open { key, target } => {
            attach_console();
            cli::open(&key, target)
        }
        Command::Doctor => {
            attach_console();
            cli::doctor()
        }
        Command::Help => {
            attach_console();
            println!("{}", cli::USAGE);
            Ok(())
        }
        Command::Version => {
            attach_console();
            println!("zrepo-rs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    };
    match result {
        Ok(()) => EX_OK,
        Err(f) => {
            if !f.msg.is_empty() {
                eprintln!("{}", f.msg);
            }
            if f.code == EX_OK { EX_FAIL } else { f.code }
        }
    }
}

/// Sous Windows, un binaire « windows » n'a pas de console : on emprunte celle
/// du terminal qui l'a lancé, sauf si la sortie est déjà redirigée (fichier,
/// tube), auquel cas les poignées héritées fonctionnent telles quelles.
#[cfg(windows)]
pub fn attach_console() {
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        },
    };
    let missing = |h: HANDLE| h.is_null() || h == INVALID_HANDLE_VALUE;
    // SAFETY: appels Win32 sans pointeur fourni par nous ; un échec est sans effet.
    unsafe {
        if missing(GetStdHandle(STD_OUTPUT_HANDLE)) || missing(GetStdHandle(STD_ERROR_HANDLE)) {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}
