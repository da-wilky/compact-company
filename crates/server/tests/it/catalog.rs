use std::{collections::BTreeMap, fs};

use compact_company::api::error::{ErrorCode, FieldError};

fn catalog(lang: &str) -> BTreeMap<String, String> {
    let path = format!("{}/../../web/i18n/{lang}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_key_is_in_both_languages() {
    let (en, de) = (catalog("en"), catalog("de"));
    assert_eq!(en.keys().collect::<Vec<_>>(), de.keys().collect::<Vec<_>>());
    for (key, value) in en.iter().chain(&de) {
        assert!(!value.trim().is_empty(), "{key} is empty");
    }
}

#[test]
fn every_server_error_is_translated() {
    let en = catalog("en");
    let keys = ErrorCode::ALL
        .iter()
        .map(|c| c.key())
        .chain(FieldError::ALL.iter().map(|f| f.key()));
    for key in keys {
        assert!(en.contains_key(&key), "{key} is missing from the catalog");
    }
}

#[test]
fn glossary_terms_match_the_glossary() {
    let glossary =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../GLOSSARY.md")).unwrap();
    let terms: BTreeMap<String, (String, String)> = glossary
        .lines()
        .filter_map(|line| {
            let (en, rest) = line.strip_prefix("**")?.split_once("** (de: ")?;
            let de = rest.strip_suffix("):")?;
            Some((
                format!("term.{}", en.to_lowercase().replace(' ', "_")),
                (en.into(), de.into()),
            ))
        })
        .collect();
    let (en, de) = (catalog("en"), catalog("de"));
    for key in en.keys().filter(|k| k.starts_with("term.")) {
        let (term_en, term_de) = terms
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not in the glossary"));
        assert_eq!(&en[key], term_en);
        assert_eq!(&de[key], term_de);
    }
}
