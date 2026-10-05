//! OPY enum-source spellings and their canonical Workshop member identities.
//!
//! This module is the single table for which `Domain.MEMBER` forms OPY source
//! accepts: the lowerer resolves member access through [`canonical_member`],
//! and the lookup answers enum-domain and enum-member questions through
//! [`domain_members`]. Workshop-owned enum member lists stay in the
//! `workshop-rs` catalog; only the OPY-side spelling differences (renamed or
//! aliased domains, renamed members, reference-accepted alternate member
//! spellings, and OPY-only `Color` constants) are declared here. The lookup
//! reports exactly the spellings the pinned reference accepts — catalog ids
//! that upstream rejects stay query keys (`MatchKind::CatalogId`), never
//! advertised members or aliases.

use workshop_rs::catalog::{Catalog, Locale};

/// The display-name locale OPY lookups and member resolution answer in.
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

/// The canonical catalog domain a source-level domain name resolves to.
pub(crate) fn catalog_domain(domain: &str) -> &str {
    DOMAIN_RENAMES
        .iter()
        .chain(DOMAIN_ALIASES)
        .find(|(source, _)| *source == domain)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(domain)
}

/// The source-level spelling a catalog domain is reported under:
/// `Clipping` reports as `Clip`. Catalog names that are themselves
/// reference spellings (including `StartRuleBehavior`, whose `AsyncBehavior`
/// alias is an additional hit) report unchanged.
pub(crate) fn opy_domain(domain: &str) -> &str {
    DOMAIN_RENAMES
        .iter()
        .find(|(_, canonical)| *canonical == domain)
        .map(|(source, _)| *source)
        .unwrap_or(domain)
}

/// Catalog members with no reference source spelling: `LIJIANG_TOWER_LUNAR`
/// exists in the catalog while upstream's map list has no spelling for it.
/// The lowerer still accepts the catalog id (acceptance superset), but
/// lookup must not advertise it.
const NON_SOURCE_MEMBERS: &[(&str, &str)] = &[("Map", "LIJIANG_TOWER_LUNAR")];

/// An OPY `Domain.MEMBER` spelling whose canonical catalog member id differs.
struct MemberRename {
    /// The source-level domain name (e.g. `Clip`, not `Clipping`).
    domain: &'static str,
    /// The OPY member spelling.
    opy: &'static str,
    /// The canonical catalog member it resolves to.
    catalog: &'static str,
    /// Alternate upstream spellings the reference still accepts
    /// (`Hero.MCCREE` resolves to `CASSIDY`; `HudPosition.ACTUALLY_LEFT`
    /// emits `Left`). These are listed as lookup aliases rather than
    /// canonical spellings.
    alias: bool,
}

const MEMBER_RENAMES: &[MemberRename] = &[
    // Upstream map member spellings differ from the canonical map ids.
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
    // `Gamemode` members follow the same camel-to-snake convention.
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
    // `HudPosition.ACTUALLY_LEFT` is an upstream alias that emits `Left`.
    MemberRename {
        domain: "HudPosition",
        opy: "ACTUALLY_LEFT",
        catalog: "LEFT",
        alias: true,
    },
    // `Clip` members rename onto `Clipping` catalog members.
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
    // Upstream hero member spellings differ from the canonical hero ids.
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
    // Legacy upstream hero spellings the reference still accepts.
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

/// An OPY-only `Color` member: a color constant the reference expands to an
/// `rgb(r, g, b, 255)` call instead of a canonical enum member.
const EXTRA_COLOR_MEMBERS: &[(&str, i32, i32, i32)] = &[
    ("LIGHT_RED", 255, 112, 122),
    ("LIGHT_PURPLE", 210, 127, 243),
    ("LIGHT_VIOLET", 203, 135, 255),
    ("LIGHT_GRAY", 168, 168, 168),
];

/// The `(red, green, blue)` channels of an OPY-only `Color` member, when the
/// member is one of the OPY-only color constants.
pub(crate) fn extra_color_member(member: &str) -> Option<(i32, i32, i32)> {
    EXTRA_COLOR_MEMBERS
        .iter()
        .find(|(name, ..)| *name == member)
        .map(|(_, red, green, blue)| (*red, *green, *blue))
}

/// Resolve an OPY `domain.member` access to its canonical Workshop identity,
/// returning `(catalog domain, canonical member)`. `None` is the
/// `unknown-enum-member` rejection.
///
/// Map member ids are additionally matched with underscores stripped:
/// `Map.KINGSROW` is accepted alongside `Map.KINGS_ROW` (existing
/// opy-rs acceptance, tracked separately from the canonical spellings).
pub(crate) fn canonical_member(
    domain: &str,
    member: &str,
    catalog: &Catalog,
) -> Option<(String, String)> {
    let catalog_domain = catalog_domain(domain);
    let enum_domain = catalog.enum_domain(catalog_domain)?;
    let catalog_member = MEMBER_RENAMES
        .iter()
        .find(|rename| rename.domain == domain && rename.opy == member)
        .map(|rename| rename.catalog)
        .unwrap_or(member);
    enum_domain
        .members
        .iter()
        .find(|candidate| candidate.member == catalog_member)
        .map(|candidate| (catalog_domain.to_string(), candidate.member.clone()))
        .or_else(|| {
            (domain == "Map").then(|| {
                let normalized = catalog_member.replace('_', "");
                enum_domain
                    .members
                    .iter()
                    .find(|candidate| candidate.member.replace('_', "") == normalized)
                    .map(|candidate| (catalog_domain.to_string(), candidate.member.clone()))
            })?
        })
        // `Team.{n}` spells a member by its display name (`Team 1`).
        .or_else(|| {
            if domain == "Team" && member.parse::<u32>().is_ok() {
                catalog.resolve_enum_member(catalog_domain, &en_us(), &format!("Team {member}"))
            } else {
                None
            }
        })
        // `HudReeval.VISIBILITY_STRING_AND_COLOR` renames onto a catalog member
        // id that is itself accepted; the rename stays the member even if a
        // catalog data change drops the underlying id.
        .or_else(|| {
            (domain == "HudReeval" && member == "VISIBILITY_STRING_AND_COLOR")
                .then(|| (catalog_domain.to_string(), catalog_member.to_string()))
        })
}

/// One OPY enum member within a source-level domain.
pub(crate) struct DomainMember {
    /// The canonical OPY member spelling (`SOLDIER`, `NONE`, `1`).
    pub member: String,
    /// Other OPY spellings accepted for the same member (`MCCREE`, `HAMMOND`).
    pub aliases: Vec<String>,
    /// The canonical catalog member it resolves to, when it has one
    /// (`None` for OPY-only members such as `Color.LIGHT_RED`).
    pub catalog_member: Option<String>,
    /// The en-US Workshop display name of the canonical member.
    pub display_name: Option<String>,
}

/// The OPY members a source-level domain accepts, in catalog order.
/// `None` when the source domain has no catalog domain behind it
/// (contextual domains such as `ChaseReeval`, or unknown domains).
pub(crate) fn domain_members(domain: &str, catalog: &Catalog) -> Option<Vec<DomainMember>> {
    let enum_domain = catalog.enum_domain(catalog_domain(domain))?;
    let mut members: Vec<DomainMember> = enum_domain
        .members
        .iter()
        .filter(|entry| {
            !NON_SOURCE_MEMBERS
                .iter()
                .any(|(d, member)| *d == domain && *member == entry.member)
        })
        .map(|entry| {
            let rename = MEMBER_RENAMES
                .iter()
                .find(|rename| {
                    rename.domain == domain && !rename.alias && rename.catalog == entry.member
                })
                .map(|rename| rename.opy.to_string());
            let member = rename.unwrap_or_else(|| entry.member.clone());
            // Aliases are the extra OPY spellings the pinned reference
            // accepts (`Hero.MCCREE`). A canonical catalog id is never one:
            // `Hero.SOLDIER_76` is a reference rejection, and the catalog id
            // still matches queries through `MatchKind::CatalogId`.
            let aliases: Vec<String> = MEMBER_RENAMES
                .iter()
                .filter(|rename| {
                    rename.domain == domain && rename.alias && rename.catalog == entry.member
                })
                .map(|rename| rename.opy.to_string())
                .collect();
            DomainMember {
                member,
                aliases,
                catalog_member: Some(entry.member.clone()),
                display_name: entry.spelling(&en_us()).map(str::to_string),
            }
        })
        .collect();
    if domain == "Color" {
        members.extend(EXTRA_COLOR_MEMBERS.iter().map(|(name, ..)| DomainMember {
            member: (*name).to_string(),
            aliases: Vec::new(),
            catalog_member: None,
            display_name: None,
        }));
    }
    Some(members)
}

/// The canonical OPY member spellings of `domain` — the `unknown-enum-member`
/// candidate list. Unknown or unlistable domains yield an empty list.
pub(crate) fn member_spellings(domain: &str, catalog: &Catalog) -> Vec<String> {
    domain_members(domain, catalog)
        .map(|members| members.into_iter().map(|entry| entry.member).collect())
        .unwrap_or_default()
}
