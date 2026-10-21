use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::settings::{self, PathPart, SettingValueDomain};

use crate::manifest::{AliasKind, Function, FunctionKind, Manifest};

pub(crate) const CANDIDATE_LIMIT: usize = 8;

pub(crate) struct MatchCandidate {
    spelling: String,
    forms: Vec<String>,
}

impl MatchCandidate {
    fn bare(spelling: impl Into<String>) -> MatchCandidate {
        let spelling = spelling.into();
        MatchCandidate {
            forms: vec![spelling.clone()],
            spelling,
        }
    }

    fn function(function: &Function, catalog: &Catalog) -> MatchCandidate {
        let mut forms = vec![function.id.clone()];
        if let Some(catalog_id) = &function.catalog_id {
            let kind = match function.kind {
                FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
            };
            if let Some(entry) = catalog.entry(kind, catalog_id) {
                let locale = Locale::new("en-US");
                forms.extend(entry.spellings(&locale).iter().cloned());
            }
        }
        MatchCandidate {
            spelling: function.id.clone(),
            forms,
        }
    }
}

pub(crate) fn bare_candidates<I, S>(spellings: I) -> Vec<MatchCandidate>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    spellings.into_iter().map(MatchCandidate::bare).collect()
}

pub(crate) fn value_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    let mut pool = function_candidates(manifest, catalog, FunctionKind::is_value);
    pool.extend(
        crate::lower::special_forms::SPECIAL_VALUE_CALLS
            .iter()
            .map(|(spelling, _)| MatchCandidate::bare(*spelling)),
    );
    pool
}

pub(crate) fn action_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    function_candidates(manifest, catalog, FunctionKind::is_action)
}

pub(crate) fn member_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    function_candidates(manifest, catalog, FunctionKind::is_member)
}

pub(crate) fn enum_member_candidates(
    catalog: &Catalog,
    source_domain: &str,
) -> Vec<MatchCandidate> {
    let mut pool = Vec::new();
    for member in crate::enums::domain_members(source_domain, catalog).unwrap_or_default() {
        let mut forms = vec![member.member.clone()];
        forms.extend(member.display_name.iter().cloned());
        pool.push(MatchCandidate {
            spelling: member.member,
            forms,
        });
        pool.extend(member.aliases.iter().cloned().map(MatchCandidate::bare));
    }
    pool
}

fn function_candidates(
    manifest: &Manifest,
    catalog: &Catalog,
    position: impl Fn(FunctionKind) -> bool,
) -> Vec<MatchCandidate> {
    let mut pool: Vec<MatchCandidate> = manifest
        .functions()
        .iter()
        .filter(|function| position(function.kind))
        .map(|function| MatchCandidate::function(function, catalog))
        .collect();
    pool.extend(
        manifest
            .aliases()
            .iter()
            .filter(|alias| {
                alias_target(alias, manifest).is_some_and(|target| position(target.kind))
            })
            .map(|alias| MatchCandidate::bare(alias.source.clone())),
    );
    pool
}

pub(crate) fn alias_target<'a>(
    alias: &crate::manifest::Alias,
    manifest: &'a Manifest,
) -> Option<&'a Function> {
    match alias.kind {
        AliasKind::FunctionAlias => manifest.function(&alias.target),
        AliasKind::MemberAlias => manifest.member(&alias.target),
    }
}

pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub(crate) fn score_folded(query: &str, form: &str) -> Option<u32> {
    let form = fold(form);
    if query.is_empty() || form.is_empty() {
        return None;
    }
    if query == form {
        return Some(0);
    }
    if form.starts_with(query) {
        return Some((form.len() - query.len()) as u32 + 2);
    }
    if query.starts_with(&form) {
        return Some((query.len() - form.len()) as u32 + 4);
    }
    if form.len().min(query.len()) >= 3 && (form.contains(query) || query.contains(&form)) {
        return Some((form.len().max(query.len()) - form.len().min(query.len())) as u32 + 10);
    }
    let distance = levenshtein(query, &form);
    let close = distance * 3 <= query.len() as u32 + 1
        || distance * 2 <= query.len().max(form.len()) as u32;
    close.then_some(distance * 10 + 20)
}

#[cfg(test)]
fn path_score(query: &str, path: &str) -> Option<u32> {
    path_score_folded(&fold_path_query(query).0, path)
}

pub(crate) fn path_score_folded(segments: &[String], path: &str) -> Option<u32> {
    if segments.iter().any(String::is_empty) {
        return None;
    }
    let path_segments: Vec<&str> = path.split('.').collect();
    if segments.len() > path_segments.len() {
        return None;
    }
    let mut penalty = 0u32;
    for (index, segment) in segments.iter().enumerate() {
        let target = path_segments[index];
        if (target.starts_with('<') && target.ends_with('>')) || fold(target) == *segment {
            continue;
        }
        let target_folded = fold(target);
        if index + 1 == segments.len() && target_folded.starts_with(segment.as_str()) {
            penalty += (target_folded.len() - segment.len()) as u32 + 2;
            continue;
        }
        return None;
    }
    Some(penalty + (path_segments.len() - segments.len()) as u32)
}

pub(crate) fn fold_path_query(query: &str) -> (Vec<String>, String) {
    (query.trim().split('.').map(fold).collect(), fold(query))
}

fn levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<u32> = (0..=b.len() as u32).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i as u32 + 1;
        for (j, &y) in b.iter().enumerate() {
            let next = (row[j] + 1)
                .min(row[j + 1] + 1)
                .min(prev + u32::from(x != y));
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

pub(crate) fn rank(query: &str, pool: &[MatchCandidate]) -> Vec<String> {
    let query = fold(query);
    let mut scored: Vec<(u32, &str)> = pool
        .iter()
        .filter_map(|candidate| {
            candidate
                .forms
                .iter()
                .filter_map(|form| score_folded(&query, form))
                .min()
                .map(|score| (score, candidate.spelling.as_str()))
        })
        .collect();
    scored.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.cmp(y)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored
        .into_iter()
        .take(CANDIDATE_LIMIT)
        .map(|(_, spelling)| spelling.to_string())
        .collect()
}

pub(crate) fn rank_closed(query: &str, pool: &[MatchCandidate]) -> Vec<String> {
    let ranked = rank(query, pool);
    if !ranked.is_empty() {
        return ranked;
    }
    let mut spellings: Vec<String> = pool
        .iter()
        .map(|candidate| candidate.spelling.clone())
        .collect();
    spellings.sort();
    spellings.dedup();
    spellings.truncate(CANDIDATE_LIMIT);
    spellings
}

pub(crate) fn did_you_mean(message: String, ranked: &[String]) -> String {
    let Some((first, rest)) = ranked.split_first() else {
        return message;
    };
    let message = match message.rfind(" (did you mean ") {
        Some(at) if message.ends_with("?)") => &message[..at],
        _ => message.as_str(),
    };
    let others = rest
        .iter()
        .take(2)
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(" or ");
    if others.is_empty() {
        format!("{message} (did you mean '{first}'?)")
    } else {
        format!("{message} (did you mean '{first}', {others}?)")
    }
}

pub(crate) fn settings_member_candidates(
    hir_settings: Option<&crate::hir::Settings>,
    error: &workshop_rs::WorkshopError,
    suggestion: Option<&str>,
) -> Vec<String> {
    let Some(hir_settings) = hir_settings else {
        return Vec::new();
    };
    let Some(span) = crate::compiler::workshop_error_span(error) else {
        return Vec::new();
    };
    let mut path = Vec::new();
    match find_settings_node(&hir_settings.children, &mut path, &span) {
        // A member directly under `heroes.<team>` occupies a hero-name
        // slot: the pinned compiler resolves the name through
        // `tows(key, heroKw)` before reading its value, so no leaf key is
        // valid there — a `generalKeys` member is valid under the team's
        // `general` group instead (#545). Anything else keeps the owner's
        // hero-name suggestion.
        Some(SettingsMatch::Member { path, name })
            if matches!(path.as_slice(), [PathPart::Part("heroes"), PathPart::Team]) =>
        {
            if crate::compiler::settings::general_hero_keys()
                .iter()
                .any(|key| key == name)
            {
                vec!["general".to_string()]
            } else {
                suggestion.into_iter().map(str::to_string).collect()
            }
        }
        Some(SettingsMatch::Member { path, name }) => {
            let member_path: Vec<PathPart<'_>> = {
                let mut full = path;
                full.push(PathPart::Part(name));
                full
            };
            match settings::definition(&member_path) {
                Some(definition) => {
                    if matches!(definition.domain(), SettingValueDomain::Boolean)
                        && definition.enum_members().next().is_some()
                    {
                        vec!["true".to_string()]
                    } else {
                        definition
                            .enum_members()
                            .take(CANDIDATE_LIMIT)
                            .map(|member| member.id().to_string())
                            .collect()
                    }
                }
                None => {
                    let keys = sibling_keys(&member_path[..member_path.len() - 1]);
                    rank_closed(name, &bare_candidates(keys.iter()))
                }
            }
        }
        Some(SettingsMatch::Group { path })
            if matches!(
                path.as_slice(),
                [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero]
            ) =>
        {
            suggestion.into_iter().map(str::to_string).collect()
        }
        Some(SettingsMatch::Group { .. }) => Vec::new(),
        None => suggestion.into_iter().map(str::to_string).collect(),
    }
}

pub(crate) fn sibling_keys(parent_path: &[PathPart<'_>]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for definition in settings::definitions() {
        let Some(leaf) = definition.path().rsplit('.').next() else {
            continue;
        };
        let mut probe = parent_path.to_vec();
        probe.push(PathPart::Part(leaf));
        if settings::definition(&probe).is_some() && !keys.iter().any(|key| key == leaf) {
            keys.push(leaf.to_string());
        }
    }
    keys
}

enum SettingsMatch<'a> {
    Member {
        path: Vec<PathPart<'a>>,
        name: &'a str,
    },
    Group {
        path: Vec<PathPart<'a>>,
    },
}

fn find_settings_node<'a>(
    children: &'a [crate::hir::types::SettingsNode],
    path: &mut Vec<PathPart<'a>>,
    span: &workshop_rs::source::Span,
) -> Option<SettingsMatch<'a>> {
    use crate::hir::types::SettingsNode as Node;
    for node in children {
        if let Node::Group { name, children, .. } = node {
            path.push(settings_path_part(name, path));
            if let Some(found) = find_settings_node(children, path, span) {
                return Some(found);
            }
            if settings_span_matches(node.span(), span) {
                return Some(SettingsMatch::Group {
                    path: std::mem::take(path),
                });
            }
            path.pop();
        } else if settings_span_matches(node.span(), span) {
            return Some(SettingsMatch::Member {
                path: path.clone(),
                name: settings_member_name(node),
            });
        }
    }
    None
}

fn settings_path_part<'a>(name: &'a str, path: &[PathPart<'a>]) -> PathPart<'a> {
    match path {
        [PathPart::Part("heroes")] => PathPart::Team,
        [PathPart::Part("heroes"), PathPart::Team] => PathPart::Hero,
        _ => PathPart::Part(name),
    }
}

fn settings_span_matches(
    span: Option<&crate::hir::Span>,
    reported: &workshop_rs::source::Span,
) -> bool {
    let Some(span) = span else { return false };
    span.file == reported.file.index() as u32
        && span.start.line == reported.start.line
        && span.start.col == reported.start.col
        && span.end.line == reported.end.line
        && span.end.col == reported.end.col
}

fn settings_member_name(node: &crate::hir::types::SettingsNode) -> &str {
    use crate::hir::types::SettingsNode as Node;
    match node {
        Node::Number { name, .. }
        | Node::Bool { name, .. }
        | Node::String { name, .. }
        | Node::Raw { name, .. }
        | Node::List { name, .. } => name,
        Node::Group { .. } => unreachable!("groups are not leaf members"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_is_deterministic_and_bounded() {
        let pool = bare_candidates((0..20).map(|i| format!("tolerant{i}")));
        let first = rank("tolerant", &pool);
        let second = rank("tolerant", &pool);
        assert_eq!(first, second);
        assert!(first.len() <= CANDIDATE_LIMIT);
        let mut sorted = first.clone();
        sorted.sort();
        assert_eq!(first, sorted, "equal scores order by spelling");
    }

    #[test]
    fn rank_prefers_exact_then_prefix_then_edit_distance() {
        let pool = bare_candidates(["waits", "wait", "weigh", "totally"]);
        assert_eq!(
            rank("wait", &pool)[..2],
            ["wait", "waits"],
            "exact beats prefix; unrelated names drop out"
        );
    }

    #[test]
    fn did_you_mean_replaces_an_embedded_suggestion() {
        let base = "settings key 'main.descriptino' is outside the emission table \
                    (did you mean 'Description'?)"
            .to_string();
        assert_eq!(
            did_you_mean(base.clone(), &["description".to_string()]),
            "settings key 'main.descriptino' is outside the emission table \
             (did you mean 'description'?)"
        );
        assert_eq!(did_you_mean(base.clone(), &[]), base);
    }

    #[test]
    fn did_you_mean_formats_up_to_three_candidates() {
        let base = "unknown action 'x'".to_string();
        assert_eq!(did_you_mean(base.clone(), &[]), "unknown action 'x'");
        assert_eq!(
            did_you_mean(base.clone(), &["hudText".to_string()]),
            "unknown action 'x' (did you mean 'hudText'?)"
        );
        assert_eq!(
            did_you_mean(base.clone(), &["a".to_string(), "b".to_string()]),
            "unknown action 'x' (did you mean 'a', 'b'?)"
        );
        assert_eq!(
            did_you_mean(
                base,
                &[
                    "a".to_string(),
                    "b".to_string(),
                    "c".to_string(),
                    "d".to_string()
                ]
            ),
            "unknown action 'x' (did you mean 'a', 'b' or 'c'?)"
        );
    }

    #[test]
    fn function_candidates_match_display_names() {
        let manifest = Manifest::builtin().expect("builtin manifest");
        let catalog = Catalog::builtin().expect("builtin catalog");
        let pool = action_candidates(manifest, &catalog);
        assert_eq!(rank("createHudText", &pool)[0], "hudText");
        assert_eq!(
            rank("startForcingPlayerToBeHero", &pool)[0],
            "startForcingHero"
        );
    }

    #[test]
    fn enum_member_candidates_cover_the_accepted_spelling_surface() {
        let catalog = Catalog::builtin().expect("builtin catalog");
        let pool = enum_member_candidates(&catalog, "Hero");
        let ranked = rank("SOLDIER76", &pool);
        assert_eq!(
            ranked.first().map(String::as_str),
            Some("SOLDIER"),
            "the canonical OPY spelling wins: {ranked:?}"
        );
        assert!(
            !ranked.iter().any(|spelling| spelling == "SOLDIER_76"),
            "the catalog id is never a candidate: {ranked:?}"
        );
        assert_eq!(rank("MCCEE", &pool)[0], "MCCREE");
        let map_pool = enum_member_candidates(&catalog, "Map");
        let ranked = rank("blizzard world", &map_pool);
        assert_eq!(ranked[0], "BLIZZ_WORLD");
        assert!(!ranked.iter().any(|spelling| spelling == "BLIZZARD_WORLD"));
    }

    #[test]
    fn enum_member_candidates_match_the_lookup_surface() {
        let catalog = Catalog::builtin().expect("builtin catalog");
        let mut expected: Vec<String> = Vec::new();
        for member in crate::enums::domain_members("Clip", &catalog).expect("Clip members") {
            expected.push(member.member);
            expected.extend(member.aliases);
        }
        let mut reported: Vec<String> = enum_member_candidates(&catalog, "Clip")
            .into_iter()
            .map(|candidate| candidate.spelling)
            .collect();
        expected.sort_unstable();
        reported.sort_unstable();
        assert_eq!(reported, expected);
    }

    #[test]
    fn path_score_matches_segment_by_segment() {
        assert_eq!(
            path_score(
                "heroes.team1.junkrat.health%",
                "heroes.<team>.<hero>.health%"
            ),
            Some(0)
        );
        assert_eq!(
            path_score("gamemodes.ffa", "gamemodes.general.scoreToWin"),
            None
        );
        assert!(path_score("gamemodes.ffa", "gamemodes.ffa.scoreToWin").is_some());
        assert_eq!(path_score("health%", "heroes.<team>.<hero>.health%"), None);
    }
}
