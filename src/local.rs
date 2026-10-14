//! Depots git sous ~/Projects (jusqu'a ZREPO_SCAN_DEPTH niveaux).

use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use crate::{
    config::Config,
    entry::{Entry, Kind},
    sys,
};

pub fn entries(cfg: &Config) -> Vec<Entry> {
    let mut dirs = Vec::new();
    walk(&cfg.projects_dir, 1, cfg.scan_depth, &mut dirs);
    dirs.sort();
    // Une tabulation dans le chemin decalerait tous les champs TSV.
    dirs.retain(|d| d.to_str().is_some_and(|s| !s.contains(['\t', '\n'])));

    // Un `git remote` par depot : en parallele, la liste reste instantanee.
    let ids: Vec<Option<String>> = thread::scope(|s| {
        let handles: Vec<_> = dirs
            .iter()
            .map(|d| s.spawn(|| sys::repo_identity(d)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });

    dirs.iter()
        .zip(ids)
        .map(|(dir, id)| {
            let path = dir.to_string_lossy().into_owned();
            Entry {
                kind: Kind::Local,
                key: path.clone(),
                name: dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                owner: id
                    .as_deref()
                    .and_then(|i| i.split('/').next())
                    .unwrap_or("-")
                    .into(),
                visibility: "-".into(),
                description: id.unwrap_or_else(|| "depot local".into()),
                localpath: path,
            }
        })
        .collect()
}

/// Comme `find DIR -mindepth 1 -maxdepth MAX -name .git -type d` : les liens
/// symboliques ne sont pas suivis, et on ne descend pas dans les .git.
fn walk(dir: &Path, depth: usize, max: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if e.file_name() == ".git" {
            out.push(dir.to_path_buf());
        } else if depth < max {
            walk(&e.path(), depth + 1, max, out);
        }
    }
}
