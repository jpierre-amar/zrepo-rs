//! Une ligne de la liste de projets et son format TSV.
//!
//! Format (7 champs) : kind  key  name  owner  visibility  description  localpath
//!   kind = LOCAL | REMOTE | GITHUB
//!   key  = chemin absolu (LOCAL), ssh://hote/chemin (REMOTE)
//!          ou owner/repo (GITHUB) — identifiant non ambigu

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Local,
    Remote,
    Github,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Local => "LOCAL",
            Kind::Remote => "REMOTE",
            Kind::Github => "GITHUB",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "LOCAL" => Some(Kind::Local),
            "REMOTE" => Some(Kind::Remote),
            "GITHUB" => Some(Kind::Github),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub key: String,
    pub name: String,
    pub owner: String,
    pub visibility: String,
    pub description: String,
    pub localpath: String,
}

/// Une tabulation ou un saut de ligne decalerait tous les champs TSV.
pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

impl Entry {
    pub fn to_tsv(&self) -> String {
        [
            self.kind.as_str(),
            &self.key,
            &self.name,
            &self.owner,
            &self.visibility,
            &self.description,
            &self.localpath,
        ]
        .join("\t")
    }

    pub fn from_tsv(line: &str) -> Option<Self> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 || f[1].is_empty() {
            return None;
        }
        Some(Self {
            kind: Kind::parse(f[0])?,
            key: f[1].into(),
            name: f[2].into(),
            owner: f[3].into(),
            visibility: f[4].into(),
            description: f[5].into(),
            localpath: f[6].into(),
        })
    }
}
