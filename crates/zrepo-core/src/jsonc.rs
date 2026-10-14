//! Zed ecrit son settings.json en JSONC : commentaires et virgules finales.
//! On les retire avant de confier le texte a serde_json.

pub fn strip(src: &str) -> String {
    without_trailing_commas(&without_comments(src))
}

fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

fn without_trailing_commas(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        if in_str {
            out.push(c);
            if c == '\\' && i < b.len() {
                out.push(b[i]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = b[i..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::strip;

    #[test]
    fn retire_commentaires_et_virgules() {
        let src = r#"{
  // commentaire
  "a": "http://x", /* bloc */
  "b": [1, 2,],
  "c": "pas // un commentaire",
}"#;
        let v: serde_json::Value = serde_json::from_str(&strip(src)).unwrap();
        assert_eq!(v["a"], "http://x");
        assert_eq!(v["b"].as_array().unwrap().len(), 2);
        assert_eq!(v["c"], "pas // un commentaire");
    }
}
