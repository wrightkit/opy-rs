//! The shared nearest-spelling matcher for rejected OPY input (issue #469).
//!
//! One fold/score/rank pipeline produces the `did you mean` candidates on
//! `unknown-action`, `unknown-value`, `unknown-member`, `unknown-enum-member`
//! and rejected settings keys, over the same manifest, catalog and settings
//! data the compiler resolves against — the owner ships no second name
//! table. The vocabulary lookup (issue #465) ranks through this matcher, so
//! a rejection's candidates match the top entries of a lookup on the
//! rejected spelling.
//!
//! Every returned list is deterministic: equal scores order by the
//! candidate's spelling, and lists are bounded to [`CANDIDATE_LIMIT`].

use workshop_rs::catalog::{Catalog, Kind, Locale};
use workshop_rs::settings::{self, PathPart, SettingValueDomain};

use crate::manifest::{AliasKind, Function, FunctionKind, Manifest};

/// The bound on any reported candidate list.
pub(crate) const CANDIDATE_LIMIT: usize = 8;

/// One reportable spelling plus every form it matches on: the owner
/// spelling itself, `en-US` catalog display names, and reviewed source
/// aliases. Display-name forms keep display-name guesses ("Create HUD
/// Text") fold-exact against the spelling they name (`hudText`).
pub(crate) struct MatchCandidate {
    spelling: String,
    forms: Vec<String>,
}

impl MatchCandidate {
    /// A candidate that matches on its spelling alone (user-defined names,
    /// settings keys, enum members without a catalog display name).
    fn bare(spelling: impl Into<String>) -> MatchCandidate {
        let spelling = spelling.into();
        MatchCandidate {
            forms: vec![spelling.clone()],
            spelling,
        }
    }

    /// A manifest-declared callable: source id, reviewed source aliases,
    /// and the `en-US` display spellings of its catalog entry.
    fn function(function: &Function, manifest: &Manifest, catalog: &Catalog) -> MatchCandidate {
        let mut forms = vec![function.id.clone()];
        forms.extend(
            manifest
                .aliases()
                .iter()
                .filter(|alias| {
                    alias_target(alias, manifest)
                        .is_some_and(|target| std::ptr::eq(target, function))
                })
                .map(|alias| alias.source.clone()),
        );
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

/// Bare candidates for spellings that have no catalog display forms.
pub(crate) fn bare_candidates<I, S>(spellings: I) -> Vec<MatchCandidate>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    spellings.into_iter().map(MatchCandidate::bare).collect()
}

/// The value-position call spellings an `unknown-value` diagnostic can
/// suggest: manifest values, their reviewed aliases, and the by-name
/// special calls (`sorted`, `createWorkshopSetting*`).
pub(crate) fn value_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    // Member values join the pool too: a receiverless guess still finds the
    // name (`startForcingPlayerToBeHero` names `startForcingHero`); a wrong
    // call shape is a separate diagnostic axis from the spelling.
    let mut pool = function_candidates(manifest, catalog, FunctionKind::is_value);
    pool.extend(
        crate::lower::special_forms::SPECIAL_VALUE_CALLS
            .iter()
            .copied()
            .map(MatchCandidate::bare),
    );
    pool
}

/// The statement-position call spellings an `unknown-action` diagnostic
/// can suggest, member actions included (see [`value_candidates`]).
pub(crate) fn action_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    function_candidates(manifest, catalog, FunctionKind::is_action)
}

/// The member-call spellings an `unknown-member` diagnostic can suggest
/// (`eventPlayer.member(...)`, `player.member`).
pub(crate) fn member_candidates(manifest: &Manifest, catalog: &Catalog) -> Vec<MatchCandidate> {
    function_candidates(manifest, catalog, FunctionKind::is_member)
}

/// The members of a builtin domain an `unknown-enum-member` diagnostic
/// can suggest. The pool is the resolver's own acceptance surface:
/// catalog member ids (plus `en-US` display names and, for `Map`, the
/// underscore-stripped spellings the resolver folds through) and the
/// accepted alternates from
/// [`crate::lower::MEMBER_SPELLING_ALIASES`].
pub(crate) fn enum_member_candidates(
    catalog: &Catalog,
    source_domain: &str,
    catalog_domain: &str,
) -> Vec<MatchCandidate> {
    let locale = Locale::new("en-US");
    let mut pool: Vec<MatchCandidate> = catalog
        .enum_domain(catalog_domain)
        .map(|domain| {
            domain
                .members
                .iter()
                .map(|member| {
                    let mut forms = vec![member.member.clone()];
                    forms.extend(member.spellings(&locale).iter().cloned());
                    if source_domain == "Map" {
                        let stripped: String =
                            member.member.chars().filter(|c| *c != '_').collect();
                        if stripped != member.member {
                            forms.push(stripped);
                        }
                    }
                    MatchCandidate {
                        spelling: member.member.clone(),
                        forms,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    pool.extend(
        crate::lower::MEMBER_SPELLING_ALIASES
            .iter()
            .filter(|(domain, ..)| *domain == source_domain)
            .map(|(_, spelling, _)| MatchCandidate::bare(*spelling)),
    );
    pool
}

fn function_candidates(
    manifest: &Manifest,
    catalog: &Catalog,
    position: impl Fn(FunctionKind) -> bool,
) -> Vec<MatchCandidate> {
    manifest
        .functions()
        .iter()
        .filter(|function| position(function.kind))
        .map(|function| MatchCandidate::function(function, manifest, catalog))
        .collect()
}

/// The function an alias resolves to, through its declared kind.
fn alias_target<'a>(
    alias: &crate::manifest::Alias,
    manifest: &'a Manifest,
) -> Option<&'a Function> {
    match alias.kind {
        AliasKind::FunctionAlias => manifest.function(&alias.target),
        AliasKind::MemberAlias => manifest.member(&alias.target),
    }
}

// ---------------------------------------------------------------------------
// Ranking — fold both sides to lowercase alphanumerics, then score
// ---------------------------------------------------------------------------

/// Fold a spelling or phrase to a comparable form: lowercase letters and
/// digits only; `_`, `.`, `:`, `%`, and whitespace are dropped.
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Score a query against one match form; lower is closer, `None` is
/// unrelated. Exact folded equality beats prefix beats substring beats
/// edit distance.
fn score(query: &str, form: &str) -> Option<u32> {
    let query = fold(query);
    let form = fold(form);
    if query.is_empty() || form.is_empty() {
        return None;
    }
    if query == form {
        return Some(0);
    }
    if form.starts_with(&query) {
        return Some((form.len() - query.len()) as u32 + 2);
    }
    if query.starts_with(&form) {
        return Some((query.len() - form.len()) as u32 + 4);
    }
    // Substring matching on very short spellings is pure noise (a one-letter
    // display name like Icon's "X" hides inside unrelated folds).
    if form.len().min(query.len()) >= 3 && (form.contains(&query) || query.contains(&form)) {
        return Some((form.len().max(query.len()) - form.len().min(query.len())) as u32 + 10);
    }
    let distance = levenshtein(&query, &form);
    // Short names need tight edits; longer names admit proportionally
    // larger distance (a ≥50%-similar guess like `startForcingPlayerToBeHero`
    // still finds `startForcingHero`).
    let close = distance * 3 <= query.len() as u32 + 1
        || distance * 2 <= query.len().max(form.len()) as u32;
    close.then_some(distance * 10 + 20)
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

/// Rank `pool` by closeness to `query`, best first, bounded to
/// [`CANDIDATE_LIMIT`]. A candidate's score is its best match form; the
/// reported text is always the owner spelling. Ties order by spelling so
/// repeated runs return the same list in the same order.
pub(crate) fn rank(query: &str, pool: &[MatchCandidate]) -> Vec<String> {
    let mut scored: Vec<(u32, &str)> = pool
        .iter()
        .filter_map(|candidate| {
            candidate
                .forms
                .iter()
                .filter_map(|form| score(query, form))
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

/// Rank `pool`, falling back to the whole pool spelled out (sorted,
/// bounded) when nothing is near: for a closed candidate space — an
/// enum domain's members, the keys valid at one settings path — the
/// candidates are literally its members.
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

/// Append a `(did you mean …?)` suffix naming the closest `ranked`
/// candidates (at most three; the list must already be [`rank`]ed). A
/// message that already ends in one — a canonical settings rejection
/// embeds its own suggestion — has it replaced, so the rejection reports
/// one candidate list, not two.
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

// ---------------------------------------------------------------------------
// Settings candidates — the keys or members valid at the rejected path
// ---------------------------------------------------------------------------

/// The candidates a settings-emission rejection carries: the leaf keys or
/// enum members valid at the rejected member's path, resolved by walking
/// the source `settings` tree to the reported span. The owner's own single
/// `suggestion` (already in the message) is the candidate of record for
/// positions whose spelling pool is not enumerable through the public
/// settings API (hero group names, list element spellings).
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
        // A leaf member: the key either resolves (the value or kind is the
        // problem) or is itself outside the emission table.
        Some(SettingsMatch::Member { path, name }) => {
            let member_path: Vec<PathPart<'_>> = {
                let mut full = path;
                full.push(PathPart::Part(name));
                full
            };
            match settings::definition(&member_path) {
                Some(definition) => {
                    // A `BoolEnum` key is Boolean-typed over an enum domain
                    // and accepts `true` only.
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
        // A hero group under `heroes.<team>`: hero names are the accepted
        // spelling pool, which the owner only exposes through `suggestion`.
        Some(SettingsMatch::Group { path })
            if matches!(
                path.as_slice(),
                [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero]
            ) =>
        {
            suggestion.into_iter().map(str::to_string).collect()
        }
        // A team group or a structural rejection: no enumerable pool.
        Some(SettingsMatch::Group { .. }) => Vec::new(),
        // No node matched the reported span (list-element spans land here):
        // the owner's suggestion is the only available candidate.
        None => suggestion.into_iter().map(str::to_string).collect(),
    }
}

/// The leaf keys valid at `parent_path` — every declaration whose last part
/// resolves at that parent (so `gamemodes.ffa` inherits `gamemodes.general`).
fn sibling_keys(parent_path: &[PathPart<'_>]) -> Vec<String> {
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

/// The settings node found at a reported span, with its emission-table path.
enum SettingsMatch<'a> {
    Member {
        path: Vec<PathPart<'a>>,
        name: &'a str,
    },
    Group {
        path: Vec<PathPart<'a>>,
    },
}

/// Walk the settings tree to the node whose span the rejection reported,
/// building the emission-table path (`gamemodes.<mode>`, `heroes.<team>`,
/// `heroes.<team>.<hero>`) on the way down.
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

/// The path part a group name occupies, given the path so far.
fn settings_path_part<'a>(name: &'a str, path: &[PathPart<'a>]) -> PathPart<'a> {
    match path {
        [PathPart::Part("heroes")] => PathPart::Team,
        [PathPart::Part("heroes"), PathPart::Team] => PathPart::Hero,
        _ => PathPart::Part(name),
    }
}

/// Whether the HIR node span is the span a `WorkshopError` reported.
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
        // A canonical settings rejection already carries its own `did you
        // mean` suffix; the ranked candidates replace it instead of
        // stacking a second one.
        let base = "settings key 'main.descriptino' is outside the emission table \
                    (did you mean 'Description'?)"
            .to_string();
        assert_eq!(
            did_you_mean(base.clone(), &["description".to_string()]),
            "settings key 'main.descriptino' is outside the emission table \
             (did you mean 'description'?)"
        );
        // With no candidates the owner's suffix stays — it is the only
        // candidate information the rejection carries.
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
        // The Workshop display name "Create HUD Text" folds onto `hudText`.
        assert_eq!(rank("createHudText", &pool)[0], "hudText");
        // A receiverless guess at a member action still finds its name.
        assert_eq!(
            rank("startForcingPlayerToBeHero", &pool)[0],
            "startForcingHero"
        );
    }

    #[test]
    fn enum_member_candidates_cover_the_accepted_spelling_surface() {
        let catalog = Catalog::builtin().expect("builtin catalog");
        let pool = enum_member_candidates(&catalog, "Hero", "Hero");
        let ranked = rank("SOLDIER76", &pool);
        assert!(
            ranked.iter().any(|spelling| spelling == "SOLDIER"),
            "the alias spelling must be a candidate: {ranked:?}"
        );
        assert_eq!(rank("MCCEE", &pool)[0], "MCCREE");
        // Display names match too ("Soldier: 76" folds onto the member id).
        let map_pool = enum_member_candidates(&catalog, "Map", "Map");
        assert_eq!(rank("blizzard world", &map_pool)[0], "BLIZZARD_WORLD");
    }
}
