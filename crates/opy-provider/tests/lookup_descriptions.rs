//! LPP 1.8 `description` and `aliases` on `lpp/lookup` entries.

mod common;

use common::Session;
use serde_json::{Value, json};
use workshop_rs::catalog::{Catalog, Kind, Locale};

fn lookup(session: &mut Session, params: Value) -> Vec<Value> {
    let mut params = params;
    params["languageId"] = json!("opy");
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/lookup",
        "params": params,
    }));
    response["result"]["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("lookup failed: {response}"))
        .clone()
}

fn entry_for<'a>(entries: &'a [Value], spelling: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| entry["spelling"] == spelling)
        .unwrap_or_else(|| panic!("no entry {spelling}: {entries:?}"))
}

#[test]
fn descriptions_equal_the_catalog_text_in_lpp_1_8() {
    let catalog = Catalog::builtin_ref().expect("catalog");
    let locale = Locale::new("en-US");
    let mut session = Session::spawn();
    session.initialize_version("1.8");
    let entries = lookup(&mut session, json!({ "query": "startDamageModification" }));
    let entry = entry_for(&entries, "startDamageModification");
    let expected = catalog
        .description(Kind::Action, "startDamageModification", &locale)
        .expect("catalog description");
    assert_eq!(entry["description"], expected.text);
    let parameters = entry["callable"]["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 4);
    for (position, parameter) in parameters.iter().enumerate() {
        let expected = catalog
            .param_description(Kind::Action, "startDamageModification", position, &locale)
            .expect("catalog parameter description");
        assert_eq!(parameter["description"], expected.text, "{parameter}");
    }
    assert!(
        entry.get("aliases").is_none(),
        "an entry without aliases omits the field: {entry}"
    );

    let scoped = lookup(
        &mut session,
        json!({ "within": { "kind": "callable", "value": entry["identity"] } }),
    );
    for (position, parameter) in scoped.iter().enumerate() {
        let expected = catalog
            .param_description(Kind::Action, "startDamageModification", position, &locale)
            .expect("catalog parameter description");
        assert_eq!(parameter["description"], expected.text, "{parameter}");
        assert!(parameter["parameter"].get("description").is_none());
    }
    session.shutdown();
}

#[test]
fn an_alias_query_reaches_the_canonical_entry_and_both_carry_the_alias() {
    let mut session = Session::spawn();
    session.initialize_version("1.8");
    for query in ["updateEveryTick", "updateEveryFrame"] {
        let entries = lookup(&mut session, json!({ "query": query }));
        let entry = entry_for(&entries, "updateEveryFrame");
        assert_eq!(entry["aliases"], json!(["updateEveryTick"]), "{query}");
        assert!(
            entries
                .iter()
                .all(|entry| entry["spelling"] != "updateEveryTick"),
            "the alias is never a spelling: {entries:?}"
        );
    }
    session.shutdown();
}

#[test]
fn an_enum_member_alias_is_spelled_like_the_member_spelling() {
    let mut session = Session::spawn();
    session.initialize_version("1.8");
    let entries = lookup(&mut session, json!({ "query": "Hero.MCCREE" }));
    let entry = entry_for(&entries, "Hero.CASSIDY");
    assert_eq!(entry["aliases"], json!(["Hero.MCCREE"]));
    session.shutdown();
}

#[test]
fn description_and_aliases_are_absent_before_lpp_1_8() {
    let mut session = Session::spawn();
    session.initialize_version("1.7");
    let mut entries = lookup(&mut session, json!({ "query": "startDamageModification" }));
    entries.extend(lookup(&mut session, json!({ "query": "updateEveryTick" })));
    entries.extend(lookup(
        &mut session,
        json!({ "within": { "kind": "callable", "value": "opy:callable/startDamageModification" } }),
    ));
    assert!(!entries.is_empty());
    for entry in &entries {
        let text = entry.to_string();
        assert!(!text.contains("\"description\""), "{text}");
        assert!(!text.contains("\"aliases\""), "{text}");
    }
    session.shutdown();
}
