//! Icône de zrepo-rs.exe : seulement quand la *cible* est Windows (le script de
//! build s'exécute sur l'hôte, d'où CARGO_CFG_TARGET_OS plutôt que cfg!).

fn main() {
    println!("cargo:rerun-if-changed=zrepo-rs.rc");
    println!("cargo:rerun-if-changed=../../packaging/zrepo-rs.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    match embed_resource::compile("zrepo-rs.rc", embed_resource::NONE) {
        embed_resource::CompilationResult::Ok | embed_resource::CompilationResult::NotWindows => {}
        // Compilation croisée sans rc.exe ni llvm-rc : binaire sans icône.
        embed_resource::CompilationResult::NotAttempted(why) if !cfg!(windows) => {
            println!("cargo:warning=icône Windows non intégrée : {why}");
        }
        other => panic!("icône Windows : {other}"),
    }
}
