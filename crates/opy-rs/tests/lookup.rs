//! Integration tests for the vocabulary lookup surface
//! (`opy_rs::lookup`) and the candidate-bearing diagnostics that share its
//! ranking data: display-name and guessed-name resolution, enum member
//! inventories, settings keys, scope/limit controls, and the `did you mean`
//! candidates `check` and `compile` report in diagnostic messages.

use std::path::Path;

use opy_rs::lookup::{
    LookupHit, LookupOutcome, LookupQuery, LookupScope, MatchKind, SettingValueKind, lookup,
    lookup_str,
};
use opy_rs::tooling::{DiagnosticSeverity, check};

fn results(text: &str) -> Vec<LookupHit> {
    lookup_str(text).results().to_vec()
}

fn function_hit(hit: &LookupHit) -> (&str, &[opy_rs::lookup::LookupParam]) {
    match hit {
        LookupHit::Function {
            spelling, params, ..
        } => (spelling, params),
        other => panic!("expected a function hit, got {other:?}"),
    }
}

#[test]
fn opy_spelling_resolves_to_function_with_params() {
    let hits = results("hudText");
    let (spelling, params) = function_hit(&hits[0]);
    assert_eq!(spelling, "hudText");
    assert!(!params.is_empty(), "callable facts list params: {params:?}");
    match &hits[0] {
        LookupHit::Function { matched_on, .. } => assert_eq!(*matched_on, MatchKind::OpySpelling),
        _ => unreachable!(),
    }
}

#[test]
fn workshop_display_name_resolves_to_opy_function() {
    let hits = results("Create HUD Text");
    let (spelling, params) = function_hit(&hits[0]);
    assert_eq!(spelling, "hudText");
    assert!(
        params.iter().any(|param| param.name == "visibleTo"),
        "params listed: {params:?}"
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
fn renamed_member_keeps_catalog_id_match_without_alias() {
    // `Hero.SOLDIER` is the OPY spelling of the Soldier: 76 hero; the
    // canonical catalog id is `SOLDIER_76`. The id is a lookup key, never an
    // advertised OPY alias — the pinned reference rejects `Hero.SOLDIER_76`.
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
            assert!(
                !aliases.iter().any(|alias| alias == "SOLDIER_76"),
                "the catalog id is not an OPY spelling: {aliases:?}"
            );
        }
        other => panic!("expected a Hero member hit, got {other:?}"),
    }
    let hits = results("SOLDIER_76");
    match &hits[0] {
        LookupHit::EnumMember {
            spelling,
            matched_on,
            ..
        } => {
            assert_eq!(spelling, "Hero.SOLDIER");
            assert_eq!(*matched_on, MatchKind::CatalogId);
        }
        other => panic!("expected an enum member hit, got {other:?}"),
    }
}

#[test]
fn legacy_hero_spelling_resolves_to_current_member() {
    // `Hero.MCCREE` is a reference-accepted alias of `Hero.CASSIDY`.
    let hits = results("Hero.MCCREE");
    match hits
        .iter()
        .find(|hit| matches!(hit, LookupHit::EnumMember { domain, .. } if domain == "Hero"))
    {
        Some(LookupHit::EnumMember {
            spelling,
            member,
            aliases,
            ..
        }) => {
            assert_eq!(spelling, "Hero.CASSIDY");
            assert_eq!(member, "CASSIDY");
            assert!(aliases.iter().any(|alias| alias == "MCCREE"));
        }
        other => panic!("expected a Hero member hit, got {other:?}"),
    }
}

#[test]
fn renamed_members_report_the_reference_spelling() {
    // Catalog member ids that are not reference spellings stay query keys;
    // the advertised spelling is the upstream one: `busanDowntownLny` is
    // `Map.BUSAN_DOWNTOWN_LNY`, `bountyHunter` is `Gamemode.BOUNTY_HUNTER`,
    // and `TEAM_1` is `Team.1`.
    for (query, expected, catalog_id) in [
        (
            "Map.BUSAN_DOWNTOWN_LNY",
            "Map.BUSAN_DOWNTOWN_LNY",
            "BUSANDOWNTOWNLNY",
        ),
        (
            "Gamemode.BOUNTY_HUNTER",
            "Gamemode.BOUNTY_HUNTER",
            "BOUNTYHUNTER",
        ),
        ("Team.1", "Team.1", "TEAM_1"),
    ] {
        let hits = results(query);
        match hits.iter().find(
            |hit| matches!(hit, LookupHit::EnumMember { spelling, .. } if spelling == expected),
        ) {
            Some(LookupHit::EnumMember {
                member, aliases, ..
            }) => {
                assert_eq!(member, catalog_id);
                assert!(
                    !aliases.iter().any(|alias| alias == catalog_id),
                    "the catalog id is not an OPY alias for {expected}"
                );
            }
            _ => panic!("'{query}' must hit {expected}: {hits:?}"),
        }
    }
}

#[test]
fn catalog_only_domains_and_members_are_not_reported() {
    // `EventTeam`, `Rounding`, and friends are catalog-only domains: the
    // pinned reference has no `Domain.MEMBER` source for them, so lookup
    // must not advertise their members.
    for query in ["EventTeam", "EventPlayer", "Rounding", "Operation"] {
        assert!(
            results(query).iter().all(|hit| !matches!(
                hit,
                LookupHit::EnumDomain { domain, .. } if domain == query
            )),
            "'{query}' must not produce a domain hit"
        );
    }
    // `Map.LIJIANG_TOWER_LUNAR` exists in the catalog but has no reference
    // spelling; no hit may advertise it.
    assert!(
        results("LIJIANG_TOWER_LUNAR").iter().all(|hit| !matches!(
            hit,
            LookupHit::EnumMember { spelling, .. } if spelling == "Map.LIJIANG_TOWER_LUNAR"
        )),
        "a catalog-only member id is never advertised"
    );
}

#[test]
fn source_alias_domain_reports_under_both_spellings() {
    // The reference rewrites `AsyncBehavior` to `StartRuleBehavior` at parse
    // time, so both domain spellings are accepted source.
    for (domain, member) in [
        ("AsyncBehavior", "RESTART"),
        ("StartRuleBehavior", "RESTART"),
    ] {
        let hits = results(&format!("{domain}.{member}"));
        assert!(
            hits.iter().any(|hit| matches!(
                hit,
                LookupHit::EnumMember { spelling, .. } if spelling == &format!("{domain}.{member}")
            )),
            "{domain}.{member} must resolve: {hits:?}"
        );
    }
}

#[test]
fn reference_alias_member_reports_canonical_hit() {
    // `HudPosition.ACTUALLY_LEFT` is an upstream member alias that emits
    // `Left`; it lists as an alias on the canonical `LEFT` hit.
    let hits = results("HudPosition.ACTUALLY_LEFT");
    match hits
        .iter()
        .find(|hit| matches!(hit, LookupHit::EnumMember { domain, .. } if domain == "HudPosition"))
    {
        Some(LookupHit::EnumMember {
            spelling,
            member,
            aliases,
            ..
        }) => {
            assert_eq!(spelling, "HudPosition.LEFT");
            assert_eq!(member, "LEFT");
            assert!(aliases.iter().any(|alias| alias == "ACTUALLY_LEFT"));
        }
        other => panic!("expected a HudPosition member hit, got {other:?}"),
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
fn callable_hits_expose_parameter_facts() {
    // The owner supplies facts only — name, type, required, default, enum
    // domain with its member inventory — and no rendered signature string;
    // rendering policy (defaults shown, member inlining, token budget) is
    // Wright's (`wright#527`, ADR-0021).
    let hits = results("hudText");
    let (_, params) = function_hit(&hits[0]);
    let header = params.iter().find(|p| p.name == "header").expect("header");
    assert!(!header.required && header.default.as_deref() == Some("null"));
    let sort = params
        .iter()
        .find(|p| p.name == "sortOrder")
        .expect("sortOrder");
    assert_eq!(sort.default.as_deref(), Some("0"));
    let reeval = params
        .iter()
        .find(|p| p.name == "reevaluation")
        .expect("reevaluation param");
    assert_eq!(reeval.domain.as_deref(), Some("HudReeval"));
    assert!(
        reeval
            .default
            .as_deref()
            .is_some_and(|default| default.starts_with("HudReeval.")),
        "enum defaults render under the OPY domain spelling: {reeval:?}"
    );

    let hits = results("startForcingButton");
    let (_, params) = function_hit(&hits[0]);
    let button = params
        .iter()
        .find(|p| p.name == "button")
        .expect("button param");
    assert!(button.required);
    assert_eq!(button.domain.as_deref(), Some("Button"));
    assert!(
        button
            .members
            .as_ref()
            .is_some_and(|members| members.iter().any(|m| m.spelling == "PRIMARY_FIRE")),
        "enum params carry the domain's member inventory: {button:?}"
    );

    // The inventory is complete regardless of domain size — member
    // inlining bounds are the caller's rendering policy, not owner data.
    let hits = results("startForcingHero");
    let (_, params) = function_hit(&hits[0]);
    let hero = params
        .iter()
        .find(|p| p.name == "hero")
        .expect("hero param");
    assert_eq!(hero.domain.as_deref(), Some("Hero"));
    assert!(
        hero.members
            .as_ref()
            .is_some_and(|members| members.len() > 32),
        "the full member inventory is a fact, not a bounded inline list: {hero:?}"
    );
    assert!(
        hero.members
            .as_ref()
            .is_some_and(|members| members.iter().any(|m| m.spelling == "SOLDIER")),
        "members report OPY spellings: {hero:?}"
    );
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
fn settings_paths_match_segment_by_segment() {
    // A path-prefix query walks segments: `gamemodes.ffa` resolves the
    // per-mode keys below it as structural path hits, not near-misses.
    let mut query = LookupQuery::new("gamemodes.ffa");
    query.scope = LookupScope::SETTINGS;
    let hits = lookup(&query).results().to_vec();
    assert!(
        hits.iter().any(|hit| matches!(
            hit,
            LookupHit::Setting { path, matched_on, .. }
                if path.starts_with("gamemodes.ffa.") && *matched_on == MatchKind::Path
        )),
        "a path prefix resolves keys below it: {hits:?}"
    );

    // Templated hero paths keep `<team>`/`<hero>` segments and the `%`
    // suffix in the owner's spelling, and concrete instantiations match
    // through the template segments.
    let hits = results("heroes.team2.ana.health");
    assert!(
        hits.iter().any(|hit| matches!(
            hit,
            LookupHit::Setting { path, matched_on, .. }
                if path == "heroes.<team>.<hero>.health%" && *matched_on == MatchKind::Path
        )),
        "concrete segments resolve the templated path verbatim: {hits:?}"
    );

    // A folded query without the path's structure is a near-name guess,
    // not a path match.
    let mut query = LookupQuery::new("gamemodesffa");
    query.scope = LookupScope::SETTINGS;
    let hits = lookup(&query).results().to_vec();
    assert!(
        hits.iter().all(|hit| !matches!(
            hit,
            LookupHit::Setting { matched_on, .. } if *matched_on == MatchKind::Path
        )),
        "a structureless guess must not report a path match: {hits:?}"
    );
    assert!(
        hits.iter()
            .any(|hit| matches!(hit, LookupHit::Setting { .. })),
        "the guess still finds settings keys by near ranking: {hits:?}"
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
fn empty_text_lists_the_vocabulary() {
    // An empty text applies no text constraint: the outcome lists the
    // in-scope entries in construction order rather than ranking guesses.
    match lookup_str("   ") {
        LookupOutcome::Matched { results, .. } => {
            assert!(!results.is_empty(), "an unconstrained listing has entries")
        }
        other => panic!("empty text must be a listing, not an error: {other:?}"),
    }
}

#[test]
fn unsupported_queries_report_reasons() {
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
        events: false,
    };
    match lookup(&query) {
        LookupOutcome::Unsupported { .. } => {}
        other => panic!("an empty scope must be unsupported: {other:?}"),
    }
}

#[test]
fn event_spelling_resolves_to_event_entry() {
    let mut query = LookupQuery::new("playerEarnedElimination");
    query.scope = LookupScope::EVENTS;
    let hits = lookup(&query).results().to_vec();
    match &hits[0] {
        LookupHit::Event {
            spelling,
            accepts_filters,
            display_name,
            matched_on,
        } => {
            assert_eq!(spelling, "playerEarnedElimination");
            assert!(accepts_filters, "player events take @Team/@Hero/@Slot");
            assert_eq!(display_name.as_deref(), Some("Player Earned Elimination"));
            assert_eq!(*matched_on, MatchKind::OpySpelling);
        }
        other => panic!("expected an event hit, got {other:?}"),
    }
}

#[test]
fn event_display_name_and_filters_resolve() {
    // Display names answer with their display name.
    let mut query = LookupQuery::new("Ongoing - Each Player");
    query.scope = LookupScope::EVENTS;
    let hits = lookup(&query).results().to_vec();
    match &hits[0] {
        LookupHit::Event {
            spelling,
            accepts_filters,
            matched_on,
            ..
        } => {
            assert_eq!(spelling, "eachPlayer");
            assert!(*accepts_filters);
            assert_eq!(*matched_on, MatchKind::DisplayName);
        }
        other => panic!("expected an event hit, got {other:?}"),
    }
    // `global` is the one event that rejects player filters.
    let mut query = LookupQuery::new("global");
    query.scope = LookupScope::EVENTS;
    let hits = lookup(&query).results().to_vec();
    assert!(
        hits.iter().any(|hit| matches!(
            hit,
            LookupHit::Event { spelling, accepts_filters, .. }
                if spelling == "global" && !accepts_filters
        )),
        "global must report accepts_filters=false: {hits:?}"
    );
}

#[test]
fn event_scope_lists_accepted_events_only() {
    // The listing is exactly the `@Event` acceptance set: `subroutine` is
    // a catalog event but not an OPY `@Event` name (subroutine rules use
    // `def`), so it must not appear or resolve.
    let mut query = LookupQuery::new("");
    query.scope = LookupScope::EVENTS;
    query.limit = usize::MAX;
    let hits = lookup(&query).results().to_vec();
    assert_eq!(hits.len(), 13, "the accepted event set: {hits:?}");
    assert!(
        hits.iter()
            .all(|hit| matches!(hit, LookupHit::Event { .. })),
        "events scope returns only event hits: {hits:?}"
    );
    assert!(
        hits.iter().all(|hit| !matches!(
            hit,
            LookupHit::Event { spelling, .. } if spelling == "subroutine"
        )),
        "subroutine is not an @Event name: {hits:?}"
    );

    let mut query = LookupQuery::new("subroutine");
    query.scope = LookupScope::EVENTS;
    assert!(
        lookup(&query).results().is_empty(),
        "subroutine must not resolve under the event scope"
    );
}

#[test]
fn event_hits_appear_in_the_default_scope() {
    // An unscoped query on a display name finds the event alongside the
    // other namespaces.
    let hits = results("Player Died");
    assert!(
        hits.iter().any(|hit| matches!(
            hit,
            LookupHit::Event { spelling, .. } if spelling == "playerDied"
        )),
        "events join the default search scope: {hits:?}"
    );
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
fn unknown_action_names_nearest_action_in_message() {
    let diagnostic = first_error(
        "rule \"r\":\n    @Event global\n    hudTex(allPlayers(), null, null, null, HudReeval.VISIBILITY_AND_STRING, 0)\n",
    );
    assert_eq!(diagnostic.code, "unknown-action");
    assert!(
        diagnostic.message.contains("did you mean 'hudText'"),
        "message names the candidate: {:?}",
        diagnostic.message
    );
}

#[test]
fn unknown_value_names_nearest_value_in_message() {
    let diagnostic = first_error(
        "globalvar x\nrule \"r\":\n    @Event global\n    x = buttonStr(Button.RELOAD)\n",
    );
    assert_eq!(diagnostic.code, "unknown-value");
    assert!(
        diagnostic.message.contains("buttonString"),
        "the alias spelling must be a candidate: {:?}",
        diagnostic.message
    );
}

#[test]
fn unknown_member_call_names_nearest_member_in_message() {
    let diagnostic =
        first_error("rule \"r\":\n    @Event eachPlayer\n    eventPlayer.setHealthh(50)\n");
    assert_eq!(diagnostic.code, "unknown-member");
    assert!(
        diagnostic.message.contains("setHealth"),
        "the member pool must rank setHealth: {:?}",
        diagnostic.message
    );
}

#[test]
fn unknown_enum_member_names_domain_members_in_message() {
    let diagnostic =
        first_error("globalvar x\nrule \"r\":\n    @Event global\n    x = Color.REDD\n");
    assert_eq!(diagnostic.code, "unknown-enum-member");
    assert!(
        diagnostic.message.contains("did you mean 'RED'"),
        "enum member candidates must include RED: {:?}",
        diagnostic.message
    );
    // The candidate pool is the rejected member's domain, not every enum
    // member: no other domain's members leak in.
    assert!(
        !diagnostic.message.contains("ALLY"),
        "off-domain members must not be suggested: {:?}",
        diagnostic.message
    );
}

#[test]
fn guessed_canonical_id_resolves_and_suggests_opy_spelling() {
    // The `SOLDIER76` guess from the tracking issue: the nearest member
    // candidate is the OPY spelling `Hero.SOLDIER`.
    let diagnostic =
        first_error("globalvar x\nrule \"r\":\n    @Event global\n    x = Hero.SOLDIER76\n");
    assert_eq!(diagnostic.code, "unknown-enum-member");
    assert!(
        diagnostic.message.contains("'SOLDIER"),
        "SOLDIER76 suggests the Hero.SOLDIER spelling: {:?}",
        diagnostic.message
    );
    // A catalog-id guess still resolves to the canonical member through
    // the lookup surface (accepted spellings are not advertised).
    let hits = results("Hero.SOLDIER_76");
    assert!(
        hits.iter().any(|hit| matches!(
            hit,
            LookupHit::EnumMember { spelling, .. } if spelling == "Hero.SOLDIER"
        )),
        "catalog id resolves to the member: {hits:?}"
    );
}

#[test]
fn unknown_settings_key_names_sibling_keys_in_message() {
    let diagnostic = first_error(concat!(
        "settings {\n",
        "    \"main\": {\"description\": \"t\"},\n",
        "    \"gamemodes\": {\"ffa\": {\"scoreToWinn\": 5}}\n",
        "}\n",
        "rule \"a\":\n    @Event global\n    wait(1)\n",
    ));
    assert_eq!(diagnostic.code, "workshop-emission");
    assert!(
        diagnostic.message.contains("scoreToWin"),
        "settings candidates must include scoreToWin: {:?}",
        diagnostic.message
    );
}

#[test]
fn compile_error_carries_the_same_message_candidates() {
    // `compile` and `check` share one pipeline: the returned `OpyError`
    // message names the same candidates.
    let error = opy_rs::compile(
        "globalvar x\nrule \"r\":\n    @Event global\n    x = Color.REDD\n",
        "main.opy",
        Path::new(""),
    )
    .expect_err("the misspelled member must fail");
    assert_eq!(error.code, "unknown-enum-member");
    assert!(
        error.message.contains("did you mean 'RED'"),
        "compile must name the same candidates: {:?}",
        error.message
    );
}
