//! OPY enum-source spellings and their canonical Workshop member identities.
//!
//! This module is the single table for which `Domain.MEMBER` forms OPY source
//! accepts: the lowerer resolves member access through [`canonical_member`],
//! and the lookup answers enum-domain and enum-member questions through
//! [`domain_members`]. Workshop-owned enum member lists stay in the
//! `workshop-rs` catalog; only the OPY-side spelling differences (renamed
//! domains, renamed or legacy member spellings, and OPY-only `Color`
//! constants) are declared here.

use workshop_rs::catalog::{Catalog, Locale};

/// The display-name locale OPY lookups and member resolution answer in.
pub(crate) fn en_us() -> Locale {
    Locale::new("en-US")
}

/// The OPY source domain spellings that rename a canonical catalog domain.
///
/// `Clip` and `AsyncBehavior` are the upstream OPY names for catalog
/// domains the Workshop data names differently; the catalog ids stay
/// accepted as source domain names too.
pub(crate) const DOMAIN_RENAMES: &[(&str, &str)] =
    &[("Clip", "Clipping"), ("AsyncBehavior", "StartRuleBehavior")];

/// The canonical catalog domain a source-level domain name resolves to.
pub(crate) fn catalog_domain(domain: &str) -> &str {
    DOMAIN_RENAMES
        .iter()
        .find(|(source, _)| *source == domain)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(domain)
}

/// An OPY `Domain.MEMBER` spelling whose canonical catalog member id differs.
struct MemberRename {
    /// The source-level domain name (e.g. `Clip`, not `Clipping`).
    domain: &'static str,
    /// The OPY member spelling.
    opy: &'static str,
    /// The canonical catalog member it resolves to.
    catalog: &'static str,
    /// Legacy upstream spellings the reference still accepts (`Hero.MCCREE`).
    /// These are listed as lookup aliases rather than canonical spellings.
    legacy: bool,
}

const MEMBER_RENAMES: &[MemberRename] = &[
    // Upstream map member spellings differ from the canonical map ids.
    MemberRename {
        domain: "Map",
        opy: "BLIZZ_WORLD",
        catalog: "BLIZZARD_WORLD",
        legacy: false,
    },
    MemberRename {
        domain: "Map",
        opy: "BLIZZ_WORLD_WINTER",
        catalog: "BLIZZARD_WORLD_WINTER",
        legacy: false,
    },
    MemberRename {
        domain: "Map",
        opy: "ROUTE66",
        catalog: "ROUTE_66",
        legacy: false,
    },
    MemberRename {
        domain: "Map",
        opy: "VOLSKAYA",
        catalog: "VOLSKAYA_INDUSTRIES",
        legacy: false,
    },
    // `Clip` members rename onto `Clipping` catalog members.
    MemberRename {
        domain: "Clip",
        opy: "NONE",
        catalog: "DO_NOT_CLIP",
        legacy: false,
    },
    MemberRename {
        domain: "Clip",
        opy: "SURFACES",
        catalog: "CLIP_AGAINST_SURFACES",
        legacy: false,
    },
    MemberRename {
        domain: "SpecVisibility",
        opy: "ALWAYS",
        catalog: "VISIBLE_ALWAYS",
        legacy: false,
    },
    MemberRename {
        domain: "SpecVisibility",
        opy: "NEVER",
        catalog: "VISIBLE_NEVER",
        legacy: false,
    },
    MemberRename {
        domain: "EffectReeval",
        opy: "VISIBILITY_POSITION_AND_RADIUS",
        catalog: "VISIBLE_TO_POSITION_AND_RADIUS",
        legacy: false,
    },
    MemberRename {
        domain: "HudReeval",
        opy: "VISIBILITY_AND_COLOR",
        catalog: "VISIBLE_TO_AND_COLOR",
        legacy: false,
    },
    MemberRename {
        domain: "HudReeval",
        opy: "VISIBILITY_STRING_AND_COLOR",
        catalog: "VISIBLE_TO_STRING_AND_COLOR",
        legacy: false,
    },
    // Upstream hero member spellings differ from the canonical hero ids.
    MemberRename {
        domain: "Hero",
        opy: "DOMINA",
        catalog: "JINYU",
        legacy: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "DMON",
        catalog: "D_MON",
        legacy: false,
    },
    MemberRename {
        domain: "Hero",
        opy: "SOLDIER",
        catalog: "SOLDIER_76",
        legacy: false,
    },
    // Legacy upstream hero spellings the reference still accepts.
    MemberRename {
        domain: "Hero",
        opy: "MCCREE",
        catalog: "CASSIDY",
        legacy: true,
    },
    MemberRename {
        domain: "Hero",
        opy: "HAMMOND",
        catalog: "WRECKING_BALL",
        legacy: true,
    },
    // `Team.1`/`Team.2` are the upstream team spellings.
    MemberRename {
        domain: "Team",
        opy: "1",
        catalog: "TEAM_1",
        legacy: false,
    },
    MemberRename {
        domain: "Team",
        opy: "2",
        catalog: "TEAM_2",
        legacy: false,
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
        .map(|entry| {
            let rename = MEMBER_RENAMES
                .iter()
                .find(|rename| {
                    rename.domain == domain && !rename.legacy && rename.catalog == entry.member
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
                    rename.domain == domain && rename.legacy && rename.catalog == entry.member
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
