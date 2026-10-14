//! Equivalent de `zrepo-rs list [--refresh]` : verification manuelle du core.

use std::process::ExitCode;

use zrepo_core::{config::Config, sources};

fn main() -> ExitCode {
    let refresh = std::env::args()
        .skip(1)
        .any(|a| a == "--refresh" || a == "-r");
    let run = || -> zrepo_core::error::Res<()> {
        let cfg = Config::load()?;
        let (entries, warnings) = sources::collect(&cfg, refresh)?;
        for w in warnings {
            eprintln!("{w}");
        }
        for e in entries {
            println!("{}", e.to_tsv());
        }
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(f) => {
            eprintln!("{}", f.msg);
            ExitCode::from(f.code.clamp(0, 255) as u8)
        }
    }
}
