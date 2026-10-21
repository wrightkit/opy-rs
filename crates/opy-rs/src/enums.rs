use workshop_rs::catalog::{Catalog, EnumDomain, Locale};

pub(crate) fn en_us() -> Locale {
    Locale::new("en-US")
}

/// The OPY source domain spellings that rename a canonical catalog domain
/// whose own name is not a reference source spelling: upstream's enum is
/// named `Clip`, so `Clipping.DO_NOT_CLIP` does not compile upstream even
/// though the lowerer resolves the catalog id verbatim.
pub(crate) const DOMAIN_RENAMES: &[(&str, &str)] = &[("Clip", "Clipping")];

/// Source-level domain spellings the reference accepts as aliases of a
/// catalog domain: upstream rewrites `AsyncBehavior` to `StartRuleBehavior`
/// at parse time, so both domain spellings compile with the same member
/// set.
pub(crate) const DOMAIN_ALIASES: &[(&str, &str)] = &[("AsyncBehavior", "StartRuleBehavior")];

/// Catalog enum domains that are not `Domain.MEMBER` sources in the
/// reference: upstream rejects `EventTeam.ALL`, `Rounding.UP`, and friends
/// as ordinary member access on undeclared names. The lowerer still
/// resolves their catalog members (an existing acceptance superset), but
/// lookup must not advertise spellings the pinned reference cannot compile.
pub(crate) const NON_SOURCE_DOMAINS: &[&str] = &[
    "EventPlayer",
    "EventTeam",
    "InworldTextReeval",
    "Operation",
    "ProgressBarWorldReeval",
    "Rounding",
];

pub(crate) fn catalog_domain(domain: &str) -> &str {
    DOMAIN_RENAMES
        .iter()
        .chain(DOMAIN_ALIASES)
        .find(|(source, _)| *source == domain)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(domain)
}

pub(crate) fn opy_domain(domain: &str) -> &str {
    DOMAIN_RENAMES
        .iter()
        .find(|(_, canonical)| *canonical == domain)
        .map(|(source, _)| *source)
        .unwrap_or(domain)
}

struct NonSourceMember {
    catalog_domain: &'static str,
    member: &'static str,
}

/// Catalog members with no reference source spelling: `LIJIANG_TOWER_LUNAR`
/// and `DOCTRINE` exist in the workshop-rs catalog while no upstream
/// release defines a `Domain.MEMBER` spelling for them. Neither the lowerer
/// nor the lookup exposes them to OverPy source (issue #466). The pinned
/// oracle never gains new data, so a member leaves this list once an
/// upstream release spells it: `Map.GRIMSVOTN` left when 9.7.17 defined it
/// (issue #498).
const NON_SOURCE_MEMBERS: &[NonSourceMember] = &[
    NonSourceMember {
        catalog_domain: "Map",
        member: "LIJIANG_TOWER_LUNAR",
    },
    NonSourceMember {
        catalog_domain: "Hero",
        member: "DOCTRINE",
    },
];

/// The enum-literal receivers the pinned reference exposes: each shares the
/// base domain's member spellings (`constantValues` builds the literal
/// tables from the same `*Kw` keyword maps) but emits the member's bare
/// display name, and does not take the base receiver's member aliases —
/// upstream rewrites `Hero.MCCREE` for the `Hero` receiver only, so
/// `HeroLiteral.MCCREE` is a reference rejection (issue #466).
const LITERAL_DOMAINS: &[(&str, &str)] = &[
    ("HeroLiteral", "Hero"),
    ("MapLiteral", "Map"),
    ("GamemodeLiteral", "Gamemode"),
    ("TeamLiteral", "Team"),
    ("ButtonLiteral", "Button"),
    ("ColorLiteral", "Color"),
];

pub(crate) fn literal_domain(source_name: &str) -> Option<&'static str> {
    LITERAL_DOMAINS
        .iter()
        .find(|&&(literal, _)| literal == source_name)
        .map(|&(_, base)| base)
}

struct MemberRename {
    domain: &'static str,
    opy: &'static str,
    catalog: &'static str,
    /// Alternate upstream spellings the reference still accepts
    /// (`Hero.MCCREE` resolves to `CASSIDY`; `HudPosition.ACTUALLY_LEFT`
    /// emits `Left`). These are listed as lookup aliases rather than
    /// canonical spellings.
    alias: bool,
}

const MEMBER_RENAMES: &[MemberRename] = &[
    MemberRename {
        domain: "Map",
        opy: "BLIZZ_WORLD",
        catalog: "BLIZZARD_WORLD",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BLIZZ_WORLD_WINTER",
        catalog: "BLIZZARD_WORLD_WINTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ROUTE66",
        catalog: "ROUTE_66",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "VOLSKAYA",
        catalog: "VOLSKAYA_INDUSTRIES",
        alias: false,
    },
    // Newer catalog map ids drop the underscores the reference keeps;
    // upstream spells `busanDowntownLny` as `BUSAN_DOWNTOWN_LNY`.
    MemberRename {
        domain: "Map",
        opy: "ARENA_VICTORIAE",
        catalog: "ARENAVICTORIAE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BLACK_FOREST_WINTER",
        catalog: "BLACKFORESTWINTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BUSAN_DOWNTOWN_LNY",
        catalog: "BUSANDOWNTOWNLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BUSAN_SANCTUARY_LNY",
        catalog: "BUSANSANCTUARYLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BUSAN_STADIUM",
        catalog: "BUSANSTADIUM",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BUSAN_STADIUM_CLASSIC",
        catalog: "BUSANSTADIUMCLASSIC",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "CHATEAU_GUILLARD_HALLOWEEN",
        catalog: "CHATEAUGUILLARDHALLOWEEN",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ECOPOINT_ANTARCTICA",
        catalog: "ECOPOINTANTARCTICA",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ECOPOINT_ANTARCTICA_WINTER",
        catalog: "ECOPOINTANTARCTICAWINTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ESTADIO_DAS_RAS",
        catalog: "ESTADIODASRAS",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ILIOS_LIGHTHOUSE",
        catalog: "ILIOSLIGHTHOUSE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ILIOS_RUINS",
        catalog: "ILIOSRUINS",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ILIOS_WELL",
        catalog: "ILIOSWELL",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_CONTROL_CENTER",
        catalog: "LIJIANGCONTROLCENTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_CONTROL_CENTER_LNY",
        catalog: "LIJIANGCONTROLCENTERLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_GARDEN",
        catalog: "LIJIANGGARDEN",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_GARDEN_LNY",
        catalog: "LIJIANGGARDENLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_NIGHT_MARKET",
        catalog: "LIJIANGNIGHTMARKET",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_NIGHT_MARKET_LNY",
        catalog: "LIJIANGNIGHTMARKETLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "LIJIANG_TOWER_LNY",
        catalog: "LIJIANGTOWERLNY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "NEON_JUNCTION",
        catalog: "NEONJUNCTION",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "NEPAL_SANCTUM",
        catalog: "NEPALSANCTUM",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "NEPAL_SHRINE",
        catalog: "NEPALSHRINE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "NEPAL_VILLAGE",
        catalog: "NEPALVILLAGE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "NEPAL_VILLAGE_WINTER",
        catalog: "NEPALVILLAGEWINTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "OASIS_CITY_CENTER",
        catalog: "OASISCITYCENTER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "OASIS_GARDENS",
        catalog: "OASISGARDENS",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "OASIS_UNIVERSITY",
        catalog: "OASISUNIVERSITY",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "PLACE_LACROIX",
        catalog: "PLACELACROIX",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "PRACTICE_RANGE",
        catalog: "PRACTICERANGE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "REDWOOD_DAM",
        catalog: "REDWOODDAM",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "SYDNEY_HARBOUR_ARENA",
        catalog: "SYDNEYHARBOURARENA",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "SYDNEY_HARBOUR_ARENA_CLASSIC",
        catalog: "SYDNEYHARBOURARENACLASSIC",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_CHAMBER",
        catalog: "WORKSHOPCHAMBER",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_EXPANSE",
        catalog: "WORKSHOPEXPANSE",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_EXPANSE_NIGHT",
        catalog: "WORKSHOPEXPANSENIGHT",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_GREEN_SCREEN",
        catalog: "WORKSHOPGREENSCREEN",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_ISLAND",
        catalog: "WORKSHOPISLAND",
        alias: false,
    },
    MemberRename {
        domain: "Map",
        opy: "WORKSHOP_ISLAND_NIGHT",
        catalog: "WORKSHOPISLANDNIGHT",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "ASSAULT_BALANCED_OVERWATCH",
        catalog: "ASSAULTBALANCEDOVERWATCH",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "BOUNTY_HUNTER",
        catalog: "BOUNTYHUNTER",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "CONTROL_APRIL_FOOLS",
        catalog: "CONTROLAPRILFOOLS",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "CONTROL_BALANCED_OVERWATCH",
        catalog: "CONTROLBALANCEDOVERWATCH",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "CONTROL_COMMUNITY",
        catalog: "CONTROLCOMMUNITY",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "ESCORT_APRIL_FOOLS",
        catalog: "ESCORTAPRILFOOLS",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "ESCORT_BALANCED_OVERWATCH",
        catalog: "ESCORTBALANCEDOVERWATCH",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "ESCORT_COMMUNITY",
        catalog: "ESCORTCOMMUNITY",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "FLASHPOINT_APRIL_FOOLS",
        catalog: "FLASHPOINTAPRILFOOLS",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "FREEZETHAW_ELIMINATION",
        catalog: "FREEZETHAWELIMINATION",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "HYBRID_APRIL_FOOLS",
        catalog: "HYBRIDAPRILFOOLS",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "HYBRID_BALANCED_OVERWATCH",
        catalog: "HYBRIDBALANCEDOVERWATCH",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "HYBRID_COMMUNITY",
        catalog: "HYBRIDCOMMUNITY",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "MEIS_SNOWBALL_OFFENSIVE",
        catalog: "MEISSNOWBALLOFFENSIVE",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "PRACTICE_RANGE",
        catalog: "PRACTICERANGE",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "PUSH_APRIL_FOOLS",
        catalog: "PUSHAPRILFOOLS",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "PUSH_BALANCED_OVERWATCH",
        catalog: "PUSHBALANCEDOVERWATCH",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "PUSH_COMMUNITY",
        catalog: "PUSHCOMMUNITY",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "SNOWBALL_FFA",
        catalog: "SNOWBALLFFA",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "STADIUM_PRACTICE_RANGE",
        catalog: "STADIUMPRACTICERANGE",
        alias: false,
    },
    MemberRename {
        domain: "Gamemode",
        opy: "YETI_HUNTER",
        catalog: "YETIHUNTER",
        alias: false,
    },
    MemberRename {
        domain: "HudPosition",
        opy: "ACTUALLY_LEFT",
        catalog: "LEFT",
        alias: true,
    },
    MemberRename {
        domain: "Clip",
        opy: "NONE",
        catalog: "DO_NOT_CLIP",
        alias: false,
    },
    MemberRename {
        domain: "Clip",
        opy: "SURFACES",
        catalog: "CLIP_AGAINST_SURFACES",
        alias: false,
    },
    MemberRename {
        domain: "SpecVisibility",
        opy: "ALWAYS",
        catalog: "VISIBLE_ALWAYS",
        alias: false,
    },
    MemberRename {
        domain: "SpecVisibility",
        opy: "NEVER",
        catalog: "VISIBLE_NEVER",
        alias: false,
    },
    MemberRename {
        domain: "EffectReeval",
        opy: "VISIBILITY_POSITION_AND_RADIUS",
        catalog: "VISIBLE_TO_POSITION_AND_RADIUS",
        alias: false,
    },
    MemberRename {
        domain: "HudReeval",
        opy: "VISIBILITY_AND_COLOR",
        catalog: "VISIBLE_TO_AND_COLOR",
        alias: false,
    },
    MemberRename {
        domain: "HudReeval",
        opy: "VISIBILITY_STRING_AND_COLOR",
        catalog: "VISIBLE_TO_STRING_AND_COLOR",
        alias: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "DOMINA",
        catalog: "JINYU",
        alias: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "DMON",
        catalog: "D_MON",
        alias: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "SOLDIER",
        catalog: "SOLDIER_76",
        alias: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "MCCREE",
        catalog: "CASSIDY",
        alias: true,
    },
    MemberRename {
        domain: "Hero",
        opy: "HAMMOND",
        catalog: "WRECKING_BALL",
        alias: true,
    },
    // `Team.1`/`Team.2` are the upstream team spellings.
    MemberRename {
        domain: "Team",
        opy: "1",
        catalog: "TEAM_1",
        alias: false,
    },
    MemberRename {
        domain: "Team",
        opy: "2",
        catalog: "TEAM_2",
        alias: false,
    },
];

const EXTRA_COLOR_MEMBERS: &[(&str, i32, i32, i32)] = &[
    ("LIGHT_RED", 255, 112, 122),
    ("LIGHT_PURPLE", 210, 127, 243),
    ("LIGHT_VIOLET", 203, 135, 255),
    ("LIGHT_GRAY", 168, 168, 168),
];

pub(crate) fn extra_color_member(member: &str) -> Option<(i32, i32, i32)> {
    EXTRA_COLOR_MEMBERS
        .iter()
        .find(|(name, ..)| *name == member)
        .map(|(_, red, green, blue)| (*red, *green, *blue))
}

pub(crate) fn canonical_member(
    domain: &str,
    member: &str,
    catalog: &Catalog,
) -> Option<(String, String)> {
    // A `*Literal` receiver shares its base domain's member spellings but
    // not the receiver-keyed aliases (`HeroLiteral.MCCREE` is a reference
    // rejection; issue #466).
    let literal = literal_domain(domain);
    let source = literal.unwrap_or(domain);
    let catalog_domain = catalog_domain(source);
    let enum_domain = catalog.enum_domain(catalog_domain)?;
    resolve_member(
        source,
        catalog_domain,
        enum_domain,
        member,
        literal.is_none(),
        catalog,
    )
    .map(|member| (catalog_domain.to_string(), member))
}

fn resolve_member(
    source: &str,
    catalog_domain: &str,
    enum_domain: &EnumDomain,
    member: &str,
    allow_aliases: bool,
    catalog: &Catalog,
) -> Option<String> {
    if let Some(rename) = MEMBER_RENAMES.iter().find(|rename| {
        rename.domain == source && rename.opy == member && (allow_aliases || !rename.alias)
    }) {
        return enum_domain
            .members
            .iter()
            .find(|candidate| candidate.member == rename.catalog)
            .map(|candidate| candidate.member.clone())
            .or_else(|| {
                (source == "HudReeval" && member == "VISIBILITY_STRING_AND_COLOR")
                    .then(|| rename.catalog.to_string())
            });
    }
    if member_rejection(source, member) != EnumMemberError::Unknown {
        return None;
    }
    if let Some(candidate) = enum_domain
        .members
        .iter()
        .find(|candidate| candidate.member == member)
    {
        return Some(candidate.member.clone());
    }
    if source == "Team"
        && member.parse::<u32>().is_ok()
        && let Some((_, member)) =
            catalog.resolve_enum_member(catalog_domain, &en_us(), &format!("Team {member}"))
    {
        return Some(member);
    }
    None
}

pub(crate) fn filter_member(
    domain: &str,
    spelling: &str,
    catalog: &Catalog,
) -> Result<String, EnumMemberError> {
    let catalog_domain = catalog_domain(domain);
    let Some(enum_domain) = catalog.enum_domain(catalog_domain) else {
        return Err(EnumMemberError::Unknown);
    };
    resolve_member(
        domain,
        catalog_domain,
        enum_domain,
        spelling,
        false,
        catalog,
    )
    .ok_or_else(|| member_rejection(domain, spelling))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnumMemberError {
    Unknown,
    Unspellable,
    Misspelled(&'static str),
}

pub(crate) fn member_rejection(domain: &str, member: &str) -> EnumMemberError {
    let source = literal_domain(domain).unwrap_or(domain);
    let catalog_domain = catalog_domain(source);
    if let Some(rename) = MEMBER_RENAMES
        .iter()
        .find(|rename| rename.domain == source && !rename.alias && rename.catalog == member)
    {
        return EnumMemberError::Misspelled(rename.opy);
    }
    if NON_SOURCE_MEMBERS
        .iter()
        .any(|h| h.catalog_domain == catalog_domain && h.member == member)
    {
        return EnumMemberError::Unspellable;
    }
    EnumMemberError::Unknown
}

pub(crate) struct DomainMember {
    pub member: String,
    pub aliases: Vec<String>,
    pub catalog_member: Option<String>,
    pub display_name: Option<String>,
}

pub(crate) fn domain_members(domain: &str, catalog: &Catalog) -> Option<Vec<DomainMember>> {
    // A `*Literal` receiver answers with its base domain's members minus
    // the receiver-keyed aliases (`HeroLiteral.MCCREE` is a reference
    // rejection; issue #466).
    let literal = literal_domain(domain);
    let source = literal.unwrap_or(domain);
    let enum_domain = catalog.enum_domain(catalog_domain(source))?;
    let mut members: Vec<DomainMember> = enum_domain
        .members
        .iter()
        .filter(|entry| {
            !NON_SOURCE_MEMBERS
                .iter()
                .any(|h| h.catalog_domain == catalog_domain(source) && h.member == entry.member)
        })
        .map(|entry| {
            let rename = MEMBER_RENAMES
                .iter()
                .find(|rename| {
                    rename.domain == source && !rename.alias && rename.catalog == entry.member
                })
                .map(|rename| rename.opy.to_string());
            let member = rename.unwrap_or_else(|| entry.member.clone());
            // Aliases are the extra OPY spellings the pinned reference
            // accepts (`Hero.MCCREE`). A canonical catalog id is never one:
            // `Hero.SOLDIER_76` is a reference rejection, and the catalog id
            // still matches queries through `MatchKind::CatalogId`.
            let aliases: Vec<String> = if literal.is_some() {
                Vec::new()
            } else {
                MEMBER_RENAMES
                    .iter()
                    .filter(|rename| {
                        rename.domain == source && rename.alias && rename.catalog == entry.member
                    })
                    .map(|rename| rename.opy.to_string())
                    .collect()
            };
            DomainMember {
                member,
                aliases,
                catalog_member: Some(entry.member.clone()),
                display_name: entry.spelling(&en_us()).map(str::to_string),
            }
        })
        .collect();
    if source == "Color" {
        members.extend(EXTRA_COLOR_MEMBERS.iter().map(|(name, ..)| DomainMember {
            member: (*name).to_string(),
            aliases: Vec::new(),
            catalog_member: None,
            display_name: None,
        }));
    }
    Some(members)
}

pub(crate) fn spelling_of_member<'a>(
    catalog_domain: &'a str,
    member: &'a str,
) -> Option<(&'a str, &'a str)> {
    if NON_SOURCE_DOMAINS.contains(&catalog_domain)
        || NON_SOURCE_MEMBERS
            .iter()
            .any(|h| h.catalog_domain == catalog_domain && h.member == member)
    {
        return None;
    }
    let source = opy_domain(catalog_domain);
    match MEMBER_RENAMES
        .iter()
        .find(|rename| rename.domain == source && !rename.alias && rename.catalog == member)
    {
        Some(rename) => Some((source, rename.opy)),
        None => Some((source, member)),
    }
}

pub(crate) fn camel_case_to_upper_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        if character.is_ascii_uppercase() {
            out.push('_');
        }
        out.extend(character.to_uppercase());
    }
    out
}

pub(crate) fn upper_case_to_camel_case(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut chars = lower.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '_' {
            match chars.next() {
                Some(next) if next.is_ascii_alphanumeric() || next == '_' => {
                    out.extend(next.to_uppercase())
                }
                Some(next) => {
                    out.push('_');
                    out.push(next);
                }
                None => out.push('_'),
            }
        } else {
            out.push(character);
        }
    }
    if let Some(first) = out.chars().next() {
        out.replace_range(..first.len_utf8(), &first.to_lowercase().to_string());
    }
    out
}

pub(crate) fn event_filter_key(argument: &str) -> String {
    match argument.split_once('.') {
        Some(("Team", member)) | Some(("Hero", member)) => upper_case_to_camel_case(member),
        _ => argument.to_string(),
    }
}
