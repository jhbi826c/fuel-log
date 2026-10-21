/// Bundled brand logos (`icons/brands/<file>.png`) and the brand names that use them.
const LOGOS: [(&str, &[&str]); 21] = [
    ("aral", &["aral"]),
    ("avia", &["avia", "avia xpress"]),
    ("baywa", &["baywa"]),
    ("bft", &["bft"]),
    ("eni", &["agip", "eni"]),
    ("esso", &["esso"]),
    ("globus", &["globus"]),
    ("hem", &["hem"]),
    ("hoyer", &["hoyer"]),
    ("jet", &["jet"]),
    ("oil", &["oil!", "oil"]),
    ("omv", &["omv"]),
    ("orlen", &["orlen"]),
    ("q1", &["q1"]),
    ("score", &["score"]),
    ("shell", &["shell"]),
    ("sprint", &["sprint"]),
    ("star", &["star"]),
    ("tamoil", &["tamoil"]),
    ("totalenergies", &["totalenergies", "total"]),
    ("westfalen", &["westfalen"]),
];

/// Whether `s` starts with `word` followed by a non-alphanumeric character or the end (case-insensitive).
fn starts_with_word(s: &str, word: &str) -> bool {
    s.get(..word.len()).is_some_and(|p| p.eq_ignore_ascii_case(word))
        && s[word.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
}

/// Logo path for a station, matched on its brand or the start of its name.
pub fn logo(brand: &str, name: &str) -> Option<String> {
    let (brand, name) = (brand.trim(), name.trim());
    LOGOS
        .iter()
        .find(|(_, names)| names.iter().any(|n| brand.eq_ignore_ascii_case(n)))
        .or_else(|| LOGOS.iter().find(|(_, names)| names.iter().any(|n| starts_with_word(brand, n) || starts_with_word(name, n))))
        .map(|(file, _)| format!("icons/brands/{file}.png"))
}

/// Letter shown when there is no logo.
pub fn initial(brand: &str, name: &str) -> String {
    let s = if brand.trim().is_empty() { name } else { brand };
    s.trim().chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches() {
        assert_eq!(logo("ARAL", "Aral Tankstelle").as_deref(), Some("icons/brands/aral.png"));
        assert_eq!(logo("TOTAL", "").as_deref(), Some("icons/brands/totalenergies.png"));
        assert_eq!(logo("", "Shell Dresden").as_deref(), Some("icons/brands/shell.png"));
        assert_eq!(logo("AVIA Xpress", "").as_deref(), Some("icons/brands/avia.png"));
        assert_eq!(logo("OIL!", "").as_deref(), Some("icons/brands/oil.png"));
        assert_eq!(logo("Freie Tankstelle", "Jetzt tanken"), None);
        assert_eq!(logo("Raiffeisen", ""), None);
        assert_eq!(initial("", " bft Willich"), "B");
        assert_eq!(initial("", ""), "?");
    }

    #[test]
    fn logo_files_exist() {
        for (file, _) in LOGOS {
            let path = format!("{}/icons/brands/{file}.png", env!("CARGO_MANIFEST_DIR"));
            assert!(std::path::Path::new(&path).exists(), "{path}");
        }
    }
}
