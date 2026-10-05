//! Integration tests for the vocabulary lookup surface
//! (`opy_rs::lookup`) and the candidate-bearing diagnostics that share its
//! ranking data: display-name and guessed-name resolution, enum member
//! inventories, settings keys, scope/limit controls, and the structured
//! `candidates` field `check` and `compile` report.

use std::path::Path;

use opy_rs::lookup::{
    LookupHit, LookupOutcome, LookupQuery, LookupScope, MatchKind, SettingValueKind, lookup,
    lookup_str,
};
use opy_rs::tooling::{DiagnosticSeverity, check};

fn results(text: &str) -> Vec<LookupHit> {
    lookup_str(text).results().to_vec()
}

fn function_hit(hit: &LookupHit) -> (&str, &str) {
    match hit {
        LookupHit::Function {
            spelling,
            signature,
            ..
        } => (spelling, signature),
        other => panic!("expected a function hit, got {other:?}"),
    }
}

#[test]
fn opy_spelling_resolves_to_function_with_signature() {
    let hits = results("hudText");
    let (spelling, signature) = function_hit(&hits[0]);
    assert_eq!(spelling, "hudText");
    assert!(signature.starts_with("hudText("), "signature: {signature}");
    match &hits[0] {
        LookupHit::Function { matched_on, .. } => assert_eq!(*matched_on, MatchKind::OpySpelling),
        _ => unreachable!(),
    }
}

#[test]
fn workshop_display_name_resolves_to_opy_function() {
    let hits = results("Create HUD Text");
    let (spelling, signature) = function_hit(&hits[0]);
    assert_eq!(spelling, "hudText");
    assert!(
        signature.contains("visibleTo"),
        "params listed: {signature}"
    );
    match &hits[0] {
        LookupHit::Function { matched_on, .. } => assert_eq!(*matched_on, MatchKind::DisplayName),
        _ => unreachable!(),
    }
}

#[test]
fn canonical_catalog_id_resolves_to_opy_function() {
    let hits = results("createHudText");
    let (spelling, _) = function_hit(&hits[0]);
    assert_eq!(spelling, "hudText");
    match &hits[0] {
        LookupHit::Function {
            matched_on,
            catalog_id,
            ..
        } => {
            assert_eq!(*matched_on, MatchKind::CatalogId);
            assert_eq!(catalog_id.as_deref(), Some("createHudText"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn declared_alias_resolves_to_its_target_function() {
    // `buttonString` is a manifest-declared function alias of
    // `inputBindingString`.
    let hits = results("buttonString");
    let (spelling, _) = function_hit(&hits[0]);
    assert_eq!(spelling, "inputBindingString");
    match &hits[0] {
        LookupHit::Function { matched_on, .. } => assert_eq!(*matched_on, MatchKind::Alias),
        _ => unreachable!(),
    }
}

#[test]
fn guessed_name_resolves_to_nearest_function() {
    // The historical agent guess `startForcingPlayerToBeHero` is the upstream
    // display name of `startForcingHero`.
    let hits = results("startForcingPlayerToBeHero");
    let (spelling, _) = function_hit(&hits[0]);
    assert_eq!(spelling, "startForcingHero");
}

#[test]
fn enum_domain_lists_opy_members() {
    let hits = results("Clip");
    let domain = hits
        .iter()
        .find_map(|hit| match hit {
            LookupHit::EnumDomain {
                domain, members, ..
            } => Some((domain, members)),
            _ => None,
        })
        .expect("a Clip domain hit");
    assert_eq!(domain.0, "Clip");
    let spellings: Vec<&str> = domain
        .1
        .as_ref()
        .expect("Clip has catalog members")
        .iter()
        .map(|member| member.spelling.as_str())
        .collect();
    assert_eq!(spellings, ["NONE", "SURFACES"]);
}

#[test]
fn renamed_opy_member_resolves_to_canonical_member() {
    let hits = results("Map.KINGSROW");
    match &hits[0] {
        LookupHit::EnumMember {
            spelling,
            domain,
            member,
            matched_on,
            ..
        } => {
            assert_eq!(spelling, "Map.KINGS_ROW");
            assert_eq!(domain, "Map");
            assert_eq!(member, "KINGS_ROW");
            assert_eq!(*matched_on, MatchKind::OpySpelling);
        }
        other => panic!("expected an enum member hit, got {other:?}"),
    }
}

#[test]
fn legacy_hero_spelling_resolves_to_current_member() {
    // `Hero.SOLDIER` is the OPY spelling of the Soldier: 76 hero; the
    // canonical catalog id is `SOLDIER_76`.
    let hits = results("Hero.SOLDIER");
    match hits
        .iter()
        .find(|hit| matches!(hit, LookupHit::EnumMember { domain, .. } if domain == "Hero"))
    {
        Some(LookupHit::EnumMember {
            spelling,
            member,
            display_name,
            aliases,
            ..
        }) => {
            assert_eq!(spelling, "Hero.SOLDIER");
            assert_eq!(member, "SOLDIER_76");
            assert_eq!(display_name.as_deref(), Some("Soldier: 76"));
            assert!(aliases.iter().any(|alias| alias == "SOLDIER_76"));
        }
        other => panic!("expected a Hero member hit, got {other:?}"),
    }
}

#[test]
fn opy_only_color_members_are_lookup_visible() {
    let hits = results("Color.LIGHT_RED");
    match &hits[0] {
        LookupHit::EnumMember {
            spelling, domain, ..
        } => {
            assert_eq!(spelling, "Color.LIGHT_RED");
            assert_eq!(domain, "Color");
        }
        other => panic!("expected an enum member hit, got {other:?}"),
    }
}

#[test]
fn member_display_name_resolves_to_enum_member() {
    // The issue's exact acceptance phrasing: `soldier 76` → `Hero.SOLDIER`.
    for query in ["soldier 76", "Soldier: 76"] {
        let hits = results(query);
        assert!(
            hits.iter().any(|hit| matches!(
                hit,
                LookupHit::EnumMember {
                    spelling,
                    ..
                } if spelling == "Hero.SOLDIER"
            )),
            "'{query}' must resolve to Hero.SOLDIER: {hits:?}"
        );
    }
}

#[test]
fn signature_follows_the_call_syntax_form() {
    // Required: `name: Type`; required enum: `name: Domain(m1|m2)` when the
    // domain is small, `name: Domain(count)` when it is not. Optional:
    // `name = default`, or `name?` for a null default; optional enum slots
    // show the default, never the member list.
    let hits = results("hudText");
    let signature = match &hits[0] {
        LookupHit::Function { signature, .. } => signature.clone(),
        other => panic!("expected a function hit, got {other:?}"),
    };
    assert!(
        signature.contains("[header?]") && signature.contains("[sortOrder = 0]"),
        "optional slots show defaults: {signature}"
    );
    assert!(
        signature.contains("HudReeval.") && !signature.contains("HudReeval("),
        "optional enum slots show the default value only: {signature}"
    );

    let hits = results("startForcingButton");
    match &hits[0] {
        LookupHit::Function {
            signature, params, ..
        } => {
            assert!(
                signature.contains("button: Button(PRIMARY_FIRE|"),
                "small required domain lists members: {signature}"
            );
            let button = params
                .iter()
                .find(|p| p.name == "button")
                .expect("button param");
            assert!(button.required);
            assert!(
                button
                    .members
                    .as_ref()
                    .is_some_and(|members| members.iter().any(|m| m.spelling == "PRIMARY_FIRE")),
                "structured members listed: {button:?}"
            );
        }
        other => panic!("expected a function hit, got {other:?}"),
    }

    let hits = results("startForcingHero");
    match &hits[0] {
        LookupHit::Function {
            signature, params, ..
        } => {
            assert!(
                signature.contains("hero: Hero(53)"),
                "large domains report member count: {signature}"
            );
            let hero = params
                .iter()
                .find(|p| p.name == "hero")
                .expect("hero param");
            assert_eq!(hero.member_count, Some(53));
            assert!(hero.members.is_none(), "large domains stay collapsed");
        }
        other => panic!("expected a function hit, got {other:?}"),
    }
}

#[test]
fn settings_key_resolves_by_leaf_name() {
    let hits = results("scoreToWin");
    let setting = hits
        .iter()
        .find_map(|hit| match hit {
            LookupHit::Setting { path, value, .. } => Some((path, value)),
            _ => None,
        })
        .expect("a settings hit");
    assert!(
        setting.0.starts_with("gamemodes.") && setting.0.ends_with(".scoreToWin"),
        "the leaf resolves to a per-mode path: {}",
        setting.0
    );
    assert_eq!(setting.1.kind, SettingValueKind::Number);
}

#[test]
fn settings_enum_key_lists_member_values() {
    let hits = results("lobby.mapRotation");
    let setting = hits
        .iter()
        .find_map(|hit| match hit {
            LookupHit::Setting { path, value, .. } if path == "lobby.mapRotation" => Some(value),
            _ => None,
        })
        .expect("the mapRotation key resolves by path");
    assert_eq!(setting.kind, SettingValueKind::Enum);
    assert!(
        setting.members.iter().any(|member| member == "afterAGame"),
        "members: {:?}",
        setting.members
    );
}

#[test]
fn scope_filters_to_one_namespace() {
    let mut query = LookupQuery::new("Clip");
    query.scope = LookupScope::FUNCTIONS;
    let hits = lookup(&query).results().to_vec();
    assert!(
        hits.iter()
            .all(|hit| matches!(hit, LookupHit::Function { .. })),
        "functions scope must not return enum hits: {hits:?}"
    );
}

#[test]
fn limit_bounds_returned_hits() {
    let mut query = LookupQuery::new("a");
    query.limit = 3;
    match lookup(&query) {
        LookupOutcome::Matched { results, limit, .. } => {
            assert_eq!(limit, 3);
            assert!(results.len() <= 3);
        }
        other => panic!("expected a matched outcome, got {other:?}"),
    }
}

#[test]
fn unsupported_queries_report_reasons() {
    match lookup_str("   ") {
        LookupOutcome::Unsupported { reason, .. } => assert!(reason.contains("empty")),
        other => panic!("empty text must be unsupported: {other:?}"),
    }
    let mut query = LookupQuery::new("hudText");
    query.locale = Some("de-DE".to_string());
    match lookup(&query) {
        LookupOutcome::Unsupported { reason, .. } => assert!(reason.contains("en-US")),
        other => panic!("non-en-US locale must be unsupported: {other:?}"),
    }
    let mut query = LookupQuery::new("hudText");
    query.scope = LookupScope {
        functions: false,
        enums: false,
        settings: false,
    };
    match lookup(&query) {
        LookupOutcome::Unsupported { .. } => {}
        other => panic!("an empty scope must be unsupported: {other:?}"),
    }
}

#[test]
fn lookup_outcome_serializes_camel_case() {
    let outcome = lookup_str("Hero.SOLDIER");
    let json = serde_json::to_value(&outcome).expect("the outcome serializes");
    assert_eq!(json["kind"], "matched");
    assert!(json["scope"]["functions"].as_bool().unwrap());
    let first = &json["results"][0];
    assert_eq!(first["matchedOn"], "opySpelling");
    assert!(first.get("displayName").is_some() || first.get("domain").is_some());
}

// ---------------------------------------------------------------------------
// Candidate-bearing diagnostics
// ---------------------------------------------------------------------------

fn first_error(source: &str) -> opy_rs::tooling::Diagnostic {
    check(source, "main.opy", Path::new(""))
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .expect("the source produces an error diagnostic")
}

#[test]
fn unknown_action_carries_ranked_candidates() {
    let diagnostic = first_error(
        "rule \"r\":\n    @Event global\n    hudTex(allPlayers(), null, null, null, HudReeval.VISIBILITY_AND_STRING, 0)\n",
    );
    assert_eq!(diagnostic.code, "unknown-action");
    assert_eq!(
        diagnostic.candidates.first().map(String::as_str),
        Some("hudText")
    );
}

#[test]
fn unknown_value_carries_ranked_candidates() {
    let diagnostic = first_error(
        "globalvar x\nrule \"r\":\n    @Event global\n    x = buttonStr(Button.RELOAD)\n",
    );
    assert_eq!(diagnostic.code, "unknown-value");
    assert!(
        diagnostic
            .candidates
            .iter()
            .any(|candidate| candidate == "buttonString"),
        "the alias spelling must be a candidate: {:?}",
        diagnostic.candidates
    );
}

#[test]
fn unknown_member_call_carries_ranked_candidates() {
    let diagnostic =
        first_error("rule \"r\":\n    @Event eachPlayer\n    eventPlayer.setHealthh(50)\n");
    assert_eq!(diagnostic.code, "unknown-member");
    assert!(
        diagnostic
            .candidates
            .iter()
            .any(|candidate| candidate == "setHealth"),
        "membership pool must rank setHealth: {:?}",
        diagnostic.candidates
    );
}

#[test]
fn unknown_enum_member_carries_member_candidates() {
    let diagnostic =
        first_error("globalvar x\nrule \"r\":\n    @Event global\n    x = Color.REDD\n");
    assert_eq!(diagnostic.code, "unknown-enum-member");
    assert!(
        diagnostic
            .candidates
            .iter()
            .any(|candidate| candidate == "RED"),
        "enum member candidates must include RED: {:?}",
        diagnostic.candidates
    );
}

#[test]
fn unknown_settings_key_carries_sibling_key_candidates() {
    let diagnostic = first_error(concat!(
        "settings {\n",
        "    \"main\": {\"description\": \"t\"},\n",
        "    \"gamemodes\": {\"ffa\": {\"scoreToWinn\": 5}}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    assert_eq!(diagnostic.code, "workshop-emission");
    assert!(
        diagnostic
            .candidates
            .iter()
            .any(|candidate| candidate == "scoreToWin"),
        "settings candidates must include scoreToWin: {:?}",
        diagnostic.candidates
    );
}

#[test]
fn compile_error_carries_the_same_candidates() {
    // `compile` and `check` share one pipeline: the returned `OpyError`
    // carries the candidate list of the same name rejection.
    let error = opy_rs::compile(
        "globalvar x\nrule \"r\":\n    @Event global\n    x = Color.REDD\n",
        "main.opy",
        Path::new(""),
    )
    .expect_err("the misspelled member must fail");
    assert_eq!(error.code, "unknown-enum-member");
    assert!(
        error.candidates.iter().any(|candidate| candidate == "RED"),
        "compile must carry the same candidates: {:?}",
        error.candidates
    );
}
