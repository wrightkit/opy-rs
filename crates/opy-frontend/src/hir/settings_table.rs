//! The OPY settings identity surface used by Opy HIR validation.
//!
//! PROVENANCE: observed from the pinned oracle 9.7.10 en-US output of the
//! oracle-success settings programs (the `compile.workshop` settings section
//! of the wright snapshots pixelart/santa/broken-weapons/client-to-server,
//! plus the parabola/crosshair/inputhud oracle runs) at OverPy commit
//! `eea67ad`. This is observed-behavior data, not copied OverPy source
//! (LICENSE-BOUNDARY policy). It was authored in the Wright monorepo
//! (`wright-ir/src/settings/table.rs`, issue #86) and ported here as part of
//! the opy-rs frontend extraction.
//!
//! Workshop-independence boundary: this table carries only the *identity*
//! surface the frontend needs to validate `settings { ... }` blocks — exact
//! key paths, leaf value kinds, and the known mode/team/hero/map/enum-member
//! spellings. The localized Workshop display names and the emission
//! rendering data remain with the Workshop emitter (wright-workshop /
//! `workshop-rs`); opy-rs does not copy emission or locale data.
//!
//! Behavioral note: validation only checks *existence* of names, so this
//! identity-only table produces exactly the same diagnostics as the wright
//! table (`settings-unknown-key` / `settings-unknown-value` / `settings-
//! invalid` with the same messages).

/// A leaf key kind: how a settings leaf validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyKind {
    /// A quoted string (`Description: "..."`).
    String,
    /// A boolean rendered `On`/`Off`.
    Bool,
    /// A plain number.
    Number,
    /// A number rendered with a `%` suffix (`Respawn Time Scalar: 30%`).
    Percent,
    /// A string-valued enumeration with a per-domain member map
    /// (`Enum(domain)`).
    Enum(&'static str),
    /// A list of map names (`enabled maps`).
    ListMap,
    /// A list of hero names (`enabled heroes`).
    ListHero,
}

/// One segment of an exact settings path.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PathPart<'a> {
    /// A literal key (mode names under `gamemodes` are literal keys too:
    /// per-key subsets are exact-path entries, #86).
    Part(&'a str),
    /// Any team slot (allTeams).
    Team,
    /// Any hero-config slot.
    Hero,
}

impl<'b> PartialEq<PathPart<'b>> for PathPart<'_> {
    fn eq(&self, other: &PathPart<'b>) -> bool {
        match (self, other) {
            (PathPart::Part(left), PathPart::Part(right)) => left == right,
            (PathPart::Team, PathPart::Team) => true,
            (PathPart::Hero, PathPart::Hero) => true,
            _ => false,
        }
    }
}

impl Eq for PathPart<'_> {}

/// One table entry: an exact key path and its kind.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TableEntry {
    pub path: &'static [PathPart<'static>],
    pub kind: KeyKind,
}

macro_rules! entry {
    ($path:expr, $kind:expr) => {
        TableEntry {
            path: &$path,
            kind: $kind,
        }
    };
}

/// The fixture-evidenced settings surface.
///
/// Slot sets (evidenced): teams {allTeams}, heroes {mei} config groups +
/// the 10 ListHero names. `enabled: true` is not evidenced; it renders with
/// no prefix. Keys outside this table (e.g. team1Slots, scoreToWin,
/// gamemodeStartTrigger, spawnHealthPacks, healthPackRespawnTime%,
/// abilityCooldown%, healingReceived%, primaryFireKb%, enableSpawningWithUlt,
/// resetPlayersAfterGoalScored, scoreLeadToWin, gameLengthInSec,
/// heroes.<team>.general, roleLimit under general, heroLimit under a named
/// mode) are `settings-unknown-key` at validation (only evidenced in
/// oracle-failing programs; corpus-bounded).
pub(crate) static ENTRIES: &[TableEntry] = &[
    // main
    entry!(
        [PathPart::Part("main"), PathPart::Part("description")],
        KeyKind::String
    ),
    entry!(
        [PathPart::Part("main"), PathPart::Part("modeName")],
        KeyKind::String
    ),
    // lobby
    entry!(
        [PathPart::Part("lobby"), PathPart::Part("ffaSlots")],
        KeyKind::Number
    ),
    // gamemodes.<mode> — per-key subsets (exact-path entries, #86):
    // enabledMaps under modes {assault, control, escort, hybrid, skirmish,
    // ffa}; enabled/roleLimit/enableCompetitiveRules under {assault, control,
    // escort, hybrid}; heroLimit/respawnTime%/enableHeroSwitching/
    // enableRandomHeroes under general only (general is a literal group name,
    // not a mode slot).
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("assault"),
            PathPart::Part("enabled")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("control"),
            PathPart::Part("enabled")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("escort"),
            PathPart::Part("enabled")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("hybrid"),
            PathPart::Part("enabled")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("assault"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("control"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("escort"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("hybrid"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("skirmish"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("ffa"),
            PathPart::Part("enabledMaps")
        ],
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("assault"),
            PathPart::Part("roleLimit")
        ],
        KeyKind::Enum("roleLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("control"),
            PathPart::Part("roleLimit")
        ],
        KeyKind::Enum("roleLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("escort"),
            PathPart::Part("roleLimit")
        ],
        KeyKind::Enum("roleLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("hybrid"),
            PathPart::Part("roleLimit")
        ],
        KeyKind::Enum("roleLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("assault"),
            PathPart::Part("enableCompetitiveRules")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("control"),
            PathPart::Part("enableCompetitiveRules")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("escort"),
            PathPart::Part("enableCompetitiveRules")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("hybrid"),
            PathPart::Part("enableCompetitiveRules")
        ],
        KeyKind::Bool
    ),
    // gamemodes.general
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("general"),
            PathPart::Part("heroLimit")
        ],
        KeyKind::Enum("heroLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("general"),
            PathPart::Part("respawnTime%")
        ],
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("general"),
            PathPart::Part("enableHeroSwitching")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Part("general"),
            PathPart::Part("enableRandomHeroes")
        ],
        KeyKind::Bool
    ),
    // heroes.<team>
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Part("enabledHeroes")
        ],
        KeyKind::ListHero
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Part("disabledHeroes")
        ],
        KeyKind::ListHero
    ),
    // heroes.<team>.<hero> config groups
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enablePrimaryFire")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableSecondaryFire")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableAbility1")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableAbility2")
        ],
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("health%")
        ],
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("passiveUltGen%")
        ],
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("combatUltGen%")
        ],
        KeyKind::Percent
    ),
];

/// The known game-mode keys (evidenced: assault, control, escort, hybrid,
/// skirmish, ffa, general).
static MODE_KEYS: &[&str] = &[
    "assault", "control", "escort", "hybrid", "skirmish", "ffa", "general",
];

/// The known map keys inside `enabledMaps` lists.
static MAP_KEYS: &[&str] = &["workshopIsland", "kingsRowWinter"];

/// The known hero keys inside hero lists and hero-config groups.
static HERO_KEYS: &[&str] = &[
    "ashe",
    "bastion",
    "dva",
    "doomfist",
    "echo",
    "moira",
    "reinhardt",
    "hammond",
    "zenyatta",
    "mei",
];

/// The known team keys inside `heroes` (evidenced: allTeams).
static TEAM_KEYS: &[&str] = &["allTeams"];

/// A known enum member (domain, member).
///
/// `roleLimit` has exactly one evidenced member ("2OfEachRolePerTeam",
/// pixelart + broken-weapons); "off" appears only in the not-acquired
/// skirmish_elim source and is rejected (settings-unknown-value) until a
/// snapshot evidences it. `heroLimit` "off" is evidenced (santa,
/// clientToServer, parabola, crosshair, inputhud).
static ENUM_MEMBERS: &[(&str, &str)] = &[("roleLimit", "2OfEachRolePerTeam"), ("heroLimit", "off")];

/// Look up a settings leaf entry by its exact path.
pub(crate) fn lookup(path: &[PathPart<'_>]) -> Option<&'static TableEntry> {
    ENTRIES.iter().find(|entry| {
        entry.path.len() == path.len() && entry.path.iter().zip(path.iter()).all(|(a, b)| a == b)
    })
}

fn key_known(keys: &[&str], key: &str) -> bool {
    keys.contains(&key)
}

/// Whether the key is a known game-mode identity.
pub(crate) fn mode_known(key: &str) -> bool {
    key_known(MODE_KEYS, key)
}

/// Whether the key is a known map identity.
pub(crate) fn map_known(key: &str) -> bool {
    key_known(MAP_KEYS, key)
}

/// Whether the key is a known hero identity.
pub(crate) fn hero_known(key: &str) -> bool {
    key_known(HERO_KEYS, key)
}

/// Whether the key is a known team identity.
pub(crate) fn team_known(key: &str) -> bool {
    key_known(TEAM_KEYS, key)
}

/// Whether the member is a known spelling in the enum domain.
pub(crate) fn enum_member_known(domain: &str, member: &str) -> bool {
    ENUM_MEMBERS.contains(&(domain, member))
}

/// A human-readable rendering of a path (diagnostics).
pub(crate) fn path_string(path: &[PathPart<'_>]) -> String {
    path.iter()
        .map(|part| match part {
            PathPart::Part(name) => (*name).to_string(),
            PathPart::Team => "<team>".to_string(),
            PathPart::Hero => "<hero>".to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}
