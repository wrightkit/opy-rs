use super::settings_json::{SettingsJson, write_settings_json};
use super::*;

const REQUIRED_SETTINGS_GROUP: &str = "gamemodes";

enum SettingsBlockShape {
    Empty,
    DirectiveOnlyExtensions,
    AuthoredMembers,
}

impl SettingsBlockShape {
    fn of(top: &[(String, SettingsJson)], extensions: &[String]) -> Self {
        match (top.is_empty(), extensions.is_empty()) {
            (true, true) => Self::Empty,
            (true, false) => Self::DirectiveOnlyExtensions,
            (false, _) => Self::AuthoredMembers,
        }
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn emit_settings(&mut self) {
        let Some(settings) = self.program.settings.as_ref() else {
            return;
        };
        let mut unemittable = false;
        for error in workshop_rs::settings::check_emission(settings) {
            unemittable = true;
            self.issue("unsupported-settings", error.to_string(), error.span());
        }
        if unemittable {
            return;
        }

        let mut top: Vec<(String, SettingsJson)> = Vec::new();
        let mut extensions: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        let last = settings.children.len().saturating_sub(1);
        for (index, child) in settings.children.iter().enumerate() {
            if !seen.insert(child.name().to_string()) {
                self.refuse_settings(child, "is a duplicate top-level group");
            }
            match child {
                SettingsNode::Workshop { children, .. } => {
                    top.push(("workshop".to_string(), self.settings_workshop(children)));
                }
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => match name.as_str() {
                    "workshop" => self.refuse_settings(
                        child,
                        "has no OPY settings form; `workshop` reparses as workshop settings",
                    ),
                    "main" | "lobby" => {
                        let display = display_names(&[format!("{name}.")]);
                        let path = [PathPart::Part(name.as_str())];
                        top.push((
                            name.clone(),
                            self.settings_object(children, &path, None, &display),
                        ));
                    }
                    "extensions" => {
                        let flags = children
                            .iter()
                            .position(|member| matches!(member, SettingsNode::Flag { .. }));
                        match flags {
                            None => {
                                let display = display_names(&["extensions.".to_string()]);
                                top.push((
                                    name.clone(),
                                    self.settings_object(
                                        children,
                                        &[PathPart::Part("extensions")],
                                        None,
                                        &display,
                                    ),
                                ));
                            }
                            Some(first) => {
                                let mut flag_names = HashSet::new();
                                for member in &children[first..] {
                                    let SettingsNode::Flag { name, .. } = member else {
                                        self.refuse_settings(
                                            member,
                                            "follows a presence-only member in `extensions`",
                                        );
                                        continue;
                                    };
                                    if !flag_names.insert(name.clone()) {
                                        self.refuse_settings(
                                            member,
                                            "is a duplicate `#!extension` name",
                                        );
                                    }
                                    let path = [
                                        PathPart::Part("extensions"),
                                        PathPart::Part(name.as_str()),
                                    ];
                                    match workshop_rs::settings::definition(&path) {
                                        Some(definition)
                                            if matches!(
                                                definition.domain(),
                                                SettingValueDomain::PresenceOnly
                                            ) =>
                                        {
                                            extensions.push(name.clone());
                                        }
                                        _ => self.refuse_settings(
                                            member,
                                            "is not a declared Workshop extension",
                                        ),
                                    }
                                }
                                if first == 0 {
                                    if index != last {
                                        self.refuse_settings(
                                            child,
                                            "carries only `#!extension` directives outside the last group",
                                        );
                                    }
                                } else {
                                    let display = display_names(&["extensions.".to_string()]);
                                    top.push((
                                        name.clone(),
                                        self.settings_object(
                                            &children[..first],
                                            &[PathPart::Part("extensions")],
                                            None,
                                            &display,
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    "gamemodes" => {
                        top.push(("gamemodes".to_string(), self.settings_modes(children)));
                    }
                    "heroes" => {
                        top.push(("heroes".to_string(), self.settings_teams(children)));
                    }
                    _ => {
                        top.push((name.clone(), self.opaque_group_object(children)));
                    }
                },
                member => self.refuse_settings(
                    member,
                    "cannot appear at the settings top level; groups are expected",
                ),
            }
        }

        match SettingsBlockShape::of(&top, &extensions) {
            SettingsBlockShape::Empty => {
                self.issue(
                    "unsupported-settings",
                    "an empty settings block has no OPY form",
                    settings.span,
                );
                return;
            }
            SettingsBlockShape::DirectiveOnlyExtensions => {}
            SettingsBlockShape::AuthoredMembers => {
                if !top.iter().any(|(key, _)| key == REQUIRED_SETTINGS_GROUP) {
                    self.issue(
                        "unsupported-settings",
                        "settings has no `gamemodes` group, which the OPY frontend requires",
                        settings.span,
                    );
                    return;
                }
            }
        }
        if !top.is_empty() {
            self.out.push_str("settings ");
            write_settings_json(&mut self.out, &SettingsJson::Object(top), 0);
            self.out.push_str("\n\n");
        }
        for name in &extensions {
            self.out.push_str("#!extension ");
            self.out.push_str(name);
            self.out.push('\n');
        }
        if !extensions.is_empty() {
            self.out.push('\n');
        }
    }

    fn settings_modes(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for mode in children {
            let (name, children) = match mode {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => (name, children),
                _ => {
                    self.refuse_settings(
                        mode,
                        "cannot appear inside `gamemodes`; groups are expected",
                    );
                    continue;
                }
            };
            if !seen.insert(name.clone()) {
                self.refuse_settings(mode, "is a duplicate mode group");
            }
            let display = display_names(&[
                format!("gamemodes.{name}."),
                "gamemodes.general.".to_string(),
            ]);
            let path = [PathPart::Part("gamemodes"), PathPart::Part(name.as_str())];
            let mut members = Vec::new();
            let mut member_names = HashSet::new();
            for (index, member) in children.iter().enumerate() {
                if member.name() == "enabled" {
                    match (index, member) {
                        (0, SettingsNode::Bool { value: false, .. }) => {
                            members.push(("enabled".to_string(), SettingsJson::Bool(false)));
                            member_names.insert("enabled".to_string());
                        }
                        _ => self.refuse_settings(
                            member,
                            "is an `enabled` marker only `enabled: false` in first position reproduces",
                        ),
                    }
                    continue;
                }
                let Some((key, json)) = self.settings_member(member, &path, None, &display) else {
                    continue;
                };
                if !member_names.insert(key.clone()) {
                    self.refuse_settings(member, "is a duplicate settings member");
                }
                members.push((key, json));
            }
            entries.push((name.clone(), SettingsJson::Object(members)));
        }
        SettingsJson::Object(entries)
    }

    fn settings_teams(&mut self, children: &[SettingsNode]) -> SettingsJson {
        const TEAM: [PathPart<'static>; 2] = [PathPart::Part("heroes"), PathPart::Team];
        const HERO: [PathPart<'static>; 3] =
            [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero];
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for team in children {
            let (name, children) = match team {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => (name, children),
                _ => {
                    self.refuse_settings(
                        team,
                        "cannot appear inside `heroes`; groups are expected",
                    );
                    continue;
                }
            };
            if !seen.insert(name.clone()) {
                self.refuse_settings(team, "is a duplicate team group");
            }
            let roster = |member: &SettingsNode| {
                matches!(member, SettingsNode::List { name, .. } if matches!(
                    name.as_str(),
                    "enabledHeroes" | "disabledHeroes"
                ))
            };
            let rest_at = children
                .iter()
                .position(|member| matches!(member, SettingsNode::Group { .. }) || roster(member))
                .unwrap_or(children.len());
            let rosters_at = children[rest_at..]
                .iter()
                .position(roster)
                .map(|index| rest_at + index)
                .unwrap_or(children.len());
            let general = &children[..rest_at];
            let rest = &children[rest_at..rosters_at];
            let rosters = &children[rosters_at..];
            for member in rest {
                if roster(member) {
                    self.refuse_settings(
                        member,
                        "is a hero roster before the last hero group; rosters reparse last",
                    );
                }
            }
            for member in rosters {
                if !roster(member) {
                    self.refuse_settings(
                        member,
                        "follows a hero roster; team `general` members reparse first",
                    );
                }
            }
            if rosters
                .iter()
                .filter(|member| member.name() == "enabledHeroes")
                .count()
                + rosters
                    .iter()
                    .filter(|member| member.name() == "disabledHeroes")
                    .count()
                > 1
            {
                self.refuse_settings(
                    team,
                    "lists both `enabledHeroes` and `disabledHeroes`, which the frontend rejects",
                );
            }

            let display = display_names(&[
                "heroes.<team>.".to_string(),
                "heroes.general.".to_string(),
                "heroes.team1.".to_string(),
                "heroes.team2.".to_string(),
                "heroes.allTeams.".to_string(),
            ]);
            let mut members = Vec::new();
            let mut member_names = HashSet::new();
            for member in rest {
                let (hero_name, hero_children) = match member {
                    SettingsNode::Group { name, children, .. }
                    | SettingsNode::RawGroup { name, children, .. } => (name, children),
                    _ => {
                        self.refuse_settings(
                            member,
                            "is a non-group member among hero groups; it would reparse as an empty group",
                        );
                        continue;
                    }
                };
                if !crate::compiler::settings::resolves_to_hero(hero_name) {
                    self.refuse_settings(member, "is not a declared hero");
                    continue;
                }
                if crate::compiler::settings::canonical_hero_name(hero_name) != hero_name.as_str() {
                    self.refuse_settings(
                        member,
                        "is a hero alias that reparses under its canonical name",
                    );
                    continue;
                }
                if !member_names.insert(hero_name.clone()) {
                    self.refuse_settings(member, "is a duplicate hero group");
                }
                members.push((
                    hero_name.clone(),
                    self.settings_object(hero_children, &HERO, Some(hero_name.as_str()), &display),
                ));
            }
            for member in rosters {
                let SettingsNode::List { elements, .. } = member else {
                    continue;
                };
                for element in elements {
                    if crate::compiler::settings::canonical_hero_name(&element.value)
                        != element.value
                    {
                        self.refuse_settings(
                            member,
                            "carries a hero alias that reparses under its canonical name",
                        );
                        break;
                    }
                }
                if !member_names.insert(member.name().to_string()) {
                    self.refuse_settings(member, "is a duplicate hero roster");
                }
                members.push((
                    member.name().to_string(),
                    SettingsJson::Array(
                        elements
                            .iter()
                            .map(|element| SettingsJson::String(element.value.clone()))
                            .collect(),
                    ),
                ));
            }
            if !general.is_empty() {
                if !member_names.insert("general".to_string()) {
                    self.refuse_settings(
                        team,
                        "carries a `general` member the hero-name position cannot express",
                    );
                }
                members.push((
                    "general".to_string(),
                    self.settings_object(general, &TEAM, None, &display),
                ));
            }
            for member in general {
                if member.name() == "general" || roster(member) {
                    self.refuse_settings(
                        member,
                        "cannot be reproduced at a team's `general` position",
                    );
                }
            }
            entries.push((name.clone(), SettingsJson::Object(members)));
        }
        SettingsJson::Object(entries)
    }

    fn settings_object(
        &mut self,
        children: &[SettingsNode],
        path: &[PathPart<'_>],
        hero: Option<&str>,
        display: &HashSet<String>,
    ) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let Some(entry) = self.settings_member(member, path, hero, display) else {
                continue;
            };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    pub(super) fn refuse_settings(&mut self, member: &SettingsNode, detail: &str) {
        self.issue(
            "unsupported-settings",
            format!("settings member '{}' {detail}", member.name()),
            member.span(),
        );
    }
}
