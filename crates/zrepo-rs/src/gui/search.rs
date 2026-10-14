//! Recherche floue (nucleo-matcher) sur la liste des projets.

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use zrepo_core::entry::{Entry, Kind};

/// Ligne retenue : index dans la liste et caractères du nom à surligner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub idx: usize,
    pub hits: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Local,
    Remote,
    Github,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Local, Filter::Remote, Filter::Github];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "Tous",
            Filter::Local => "Locaux",
            Filter::Remote => "Serveurs",
            Filter::Github => "GitHub",
        }
    }

    pub fn accepts(self, kind: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::Local => kind == Kind::Local,
            Filter::Remote => kind == Kind::Remote,
            Filter::Github => kind == Kind::Github,
        }
    }
}

/// Toutes les entrées qui correspondent à `query`, les meilleures d'abord ;
/// requête vide = ordre d'origine (locaux, distants, GitHub).
pub fn search(matcher: &mut Matcher, entries: &[Entry], query: &str) -> Vec<Match> {
    if query.trim().is_empty() {
        return (0..entries.len())
            .map(|idx| Match {
                idx,
                hits: Vec::new(),
            })
            .collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let (mut buf, mut indices) = (Vec::new(), Vec::new());
    // (le nom correspond ?, score, ligne) : les correspondances dans le nom
    // passent avant celles trouvées seulement dans l'owner ou la description.
    let mut scored: Vec<(bool, u32, Match)> = Vec::new();
    for (idx, e) in entries.iter().enumerate() {
        indices.clear();
        if let Some(score) =
            pattern.indices(Utf32Str::new(&e.name, &mut buf), matcher, &mut indices)
        {
            let mut hits: Vec<usize> = indices.iter().map(|&i| i as usize).collect();
            hits.sort_unstable();
            hits.dedup();
            scored.push((true, score, Match { idx, hits }));
            continue;
        }
        let hay = format!(
            "{}  {}  {}  {}",
            e.name, e.owner, e.description, e.visibility
        );
        indices.clear();
        let Some(score) = pattern.indices(Utf32Str::new(&hay, &mut buf), matcher, &mut indices)
        else {
            continue;
        };
        // Le nom en tête de `hay` : les indices qui tombent dedans sont surlignés.
        let n = e.name.chars().count();
        let mut hits: Vec<usize> = indices
            .iter()
            .map(|&i| i as usize)
            .filter(|&i| i < n)
            .collect();
        hits.sort_unstable();
        hits.dedup();
        scored.push((false, score, Match { idx, hits }));
    }
    // À score égal, l'ordre d'origine (locaux, distants, GitHub).
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.idx.cmp(&b.2.idx))
    });
    scored.into_iter().map(|(_, _, m)| m).collect()
}

/// Nombre de résultats par filtre, dans l'ordre de `Filter::ALL`.
pub fn counts(entries: &[Entry], matches: &[Match]) -> [usize; 4] {
    let mut c = [0; 4];
    for m in matches {
        for (i, f) in Filter::ALL.iter().enumerate() {
            if f.accepts(entries[m.idx].kind) {
                c[i] += 1;
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleo_matcher::Config;

    fn e(kind: Kind, name: &str, owner: &str) -> Entry {
        Entry {
            kind,
            key: format!("{owner}/{name}"),
            name: name.into(),
            owner: owner.into(),
            visibility: "public".into(),
            description: String::new(),
            localpath: String::new(),
        }
    }

    #[test]
    fn requete_vide_garde_l_ordre() {
        let list = [e(Kind::Local, "b", "x"), e(Kind::Github, "a", "y")];
        let mut m = Matcher::new(Config::DEFAULT);
        let r = search(&mut m, &list, "  ");
        assert_eq!(r.iter().map(|m| m.idx).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(counts(&list, &r), [2, 1, 0, 1]);
    }

    #[test]
    fn lettres_surlignees_dans_le_nom() {
        let list = [
            e(Kind::Github, "zed-config", "jp"),
            e(Kind::Local, "zrepo-rs", "jp"),
            e(Kind::Remote, "autre", "srv"),
        ];
        let mut m = Matcher::new(Config::DEFAULT);
        let r = search(&mut m, &list, "zrp");
        assert_eq!(r[0].idx, 1);
        assert_eq!(r[0].hits, vec![0, 1, 3]);
        assert!(r.iter().all(|m| m.idx != 2));
        // Le nom passe avant la description, même avec un moins bon score.
        let mut list2 = list.to_vec();
        list2[2].description = "fidèle".into();
        list2.push(e(Kind::Github, "fidelizza", "jp"));
        let r = search(&mut m, &list2, "fi");
        assert_eq!(r[0].idx, 3);
        assert_eq!(r[0].hits, vec![0, 1]);
        assert_eq!(r.last().map(|m| m.idx), Some(2));
        // Une correspondance dans l'owner seulement : retenue, rien à surligner.
        let r = search(&mut m, &list, "srv");
        assert_eq!(r.len(), 1);
        assert!(r[0].hits.is_empty());
        assert_eq!(counts(&list, &r), [1, 0, 1, 0]);
    }
}
