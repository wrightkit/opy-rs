use super::*;

pub(super) enum SettingsJson {
    Object(Vec<(String, SettingsJson)>),
    Array(Vec<SettingsJson>),
    String(String),
    Number(String),
    Bool(bool),
}

/// Write `value` as JSONC the settings parser reads back — the layout the
/// pinned OverPy's `JSON.stringify(result, null, 4)` produces: four-space
/// indentation, empty containers inline, and no trailing comma.
pub(super) fn write_settings_json(out: &mut String, value: &SettingsJson, indent: usize) {
    let pad = |out: &mut String| {
        for _ in 0..indent {
            out.push(' ');
        }
    };
    match value {
        SettingsJson::Object(entries) if entries.is_empty() => out.push_str("{}"),
        SettingsJson::Object(entries) => {
            out.push_str("{\n");
            for (index, (key, value)) in entries.iter().enumerate() {
                pad(out);
                for _ in 0..4 {
                    out.push(' ');
                }
                out.push('"');
                push_json_escaped(out, key);
                out.push_str("\": ");
                write_settings_json(out, value, indent + 4);
                if index + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out);
            out.push('}');
        }
        SettingsJson::Array(elements) if elements.is_empty() => out.push_str("[]"),
        SettingsJson::Array(elements) => {
            out.push_str("[\n");
            for (index, element) in elements.iter().enumerate() {
                pad(out);
                for _ in 0..4 {
                    out.push(' ');
                }
                write_settings_json(out, element, indent + 4);
                if index + 1 < elements.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out);
            out.push(']');
        }
        SettingsJson::String(value) => {
            out.push('"');
            push_json_escaped(out, value);
            out.push('"');
        }
        SettingsJson::Number(value) => out.push_str(value),
        SettingsJson::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
    }
}

fn push_json_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            other => out.push(other),
        }
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn settings_member(
        &mut self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
        display: &HashSet<String>,
    ) -> Option<(String, SettingsJson)> {
        if matches!(path, [PathPart::Part("extensions")]) {
            self.refuse_settings(
                member,
                "is not a presence-only extension; only `#!extension` members survive here",
            );
            return None;
        }
        let name = member.name().to_string();
        match member {
            SettingsNode::Number { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(
                    definition.domain(),
                    SettingValueDomain::Number(_) | SettingValueDomain::Percent(_)
                ) {
                    self.refuse_settings(member, "is a number member under a non-numeric setting");
                    return None;
                }
                Some((name, SettingsJson::Number(format!("{value}"))))
            }
            SettingsNode::Bool { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(definition.domain(), SettingValueDomain::Boolean) {
                    self.refuse_settings(member, "is a boolean member under a non-boolean setting");
                    return None;
                }
                Some((name, SettingsJson::Bool(*value)))
            }
            SettingsNode::String { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(
                    definition.domain(),
                    SettingValueDomain::String | SettingValueDomain::Enum { .. }
                ) {
                    self.refuse_settings(member, "is a string member under a non-string setting");
                    return None;
                }
                if crate::compile_time::has_unbound_formatters(value) {
                    self.refuse_settings(
                        member,
                        "carries a formatter the OPY settings evaluator rejects",
                    );
                    return None;
                }
                if *value == "afterAGame" && name == "mapRotation" {
                    return Some((name, SettingsJson::String("afterGame".to_string())));
                }
                if name == "mapRotation" && value == "afterGame" {
                    self.refuse_settings(
                        member,
                        "carries a `mapRotation` value the OPY frontend rewrites",
                    );
                    return None;
                }
                if matches!(path, [PathPart::Part("main")])
                    && matches!(name.as_str(), "modeName" | "description")
                    && crate::compiler::lowering::escape_bad_words(value) != *value
                {
                    self.refuse_settings(
                        member,
                        "is a `main` string the filtered-word escape rewrites",
                    );
                    return None;
                }
                Some((name, SettingsJson::String(value.clone())))
            }
            SettingsNode::List { elements, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                match definition.domain() {
                    SettingValueDomain::MapList => {
                        if elements.iter().any(|element| element.variant.is_some()) {
                            self.refuse_settings(member, "carries a map variant qualifier");
                            return None;
                        }
                    }
                    SettingValueDomain::HeroList => {
                        for element in elements {
                            if crate::compiler::settings::canonical_hero_name(&element.value)
                                != element.value
                            {
                                self.refuse_settings(
                                    member,
                                    "carries a hero alias that reparses under its canonical name",
                                );
                                return None;
                            }
                        }
                    }
                    _ => {
                        self.refuse_settings(member, "is a list member under a non-list setting");
                        return None;
                    }
                }
                Some((
                    name,
                    SettingsJson::Array(
                        elements
                            .iter()
                            .map(|element| SettingsJson::String(element.value.clone()))
                            .collect(),
                    ),
                ))
            }
            SettingsNode::Flag { .. } => {
                self.refuse_settings(
                    member,
                    "is a presence-only member; only `#!extension` under `extensions` carries one",
                );
                None
            }
            SettingsNode::Raw { value, .. } => {
                if self.member_definition(member, path, hero).is_some()
                    || display.contains(name.as_str())
                    || display.contains(format!("{name}:{value}").as_str())
                {
                    self.refuse_settings(
                        member,
                        "reparses under a catalogued display name rather than staying raw",
                    );
                    return None;
                }
                if !emittable_member_name(name.as_str()) || !emittable_raw_value(value) {
                    self.refuse_settings(member, "carries text a `name: value` line cannot write");
                    return None;
                }
                Some((name, SettingsJson::String(value.clone())))
            }
            SettingsNode::RawValue { value, .. } => {
                if !value.is_empty() {
                    self.refuse_settings(member, "has a value that reparses as a raw member");
                    return None;
                }
                if self.member_definition(member, path, hero).is_some()
                    || display.contains(name.as_str())
                    || !emittable_member_name(name.as_str())
                {
                    self.refuse_settings(
                        member,
                        "reparses under a catalogued display name rather than staying raw",
                    );
                    return None;
                }
                Some((name, SettingsJson::String(String::new())))
            }
            SettingsNode::Group { children, .. } | SettingsNode::RawGroup { children, .. } => {
                match self.member_definition(member, path, hero) {
                    Some(definition)
                        if matches!(
                            definition.domain(),
                            SettingValueDomain::MapList | SettingValueDomain::HeroList
                        ) =>
                    {
                        self.refuse_settings(
                            member,
                            "is a block under a list setting; it reparses as a list",
                        );
                        None
                    }
                    // A boolean-domain member collapses every non-boolean
                    // value to `enabled: false` upstream-style, so a block
                    // never reaches the group form.
                    Some(definition)
                        if matches!(definition.domain(), SettingValueDomain::Boolean) =>
                    {
                        self.refuse_settings(
                            member,
                            "is a block under a boolean setting; it reparses as `false`",
                        );
                        None
                    }
                    Some(_) => Some((name, self.opaque_object(children))),
                    None => {
                        if display.contains(name.as_str()) || !emittable_member_name(name.as_str())
                        {
                            self.refuse_settings(
                                member,
                                "reparses under a catalogued display name rather than staying raw",
                            );
                            return None;
                        }
                        Some((name, self.opaque_object(children)))
                    }
                }
            }
            SettingsNode::Workshop { .. } => {
                self.refuse_settings(member, "has no OPY settings form at a leaf path");
                None
            }
        }
    }

    fn member_definition(
        &self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
    ) -> Option<workshop_rs::settings::SettingDefinition> {
        let mut full = path.to_vec();
        full.push(PathPart::Part(member.name()));
        let definition = workshop_rs::settings::definition(&full)?;
        match hero {
            Some(hero) if !crate::compiler::settings::hero_setting_applies(hero, member.name()) => {
                None
            }
            _ => Some(definition),
        }
    }

    fn catalogued_member(
        &mut self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
    ) -> Option<workshop_rs::settings::SettingDefinition> {
        match self.member_definition(member, path, hero) {
            Some(definition) => Some(definition),
            None => {
                self.refuse_settings(
                    member,
                    "is a typed member under an uncatalogued key; it reparses as raw text",
                );
                None
            }
        }
    }

    pub(super) fn opaque_object(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Raw { name, value, .. }
                    if emittable_member_name(name) && emittable_raw_value(value) =>
                {
                    Some((name.clone(), SettingsJson::String(value.clone())))
                }
                SettingsNode::RawValue { name, value, .. }
                    if value.is_empty() && emittable_member_name(name) =>
                {
                    Some((name.clone(), SettingsJson::String(String::new())))
                }
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. }
                    if emittable_member_name(name) =>
                {
                    Some((name.clone(), self.opaque_object(children)))
                }
                _ => {
                    self.refuse_settings(member, "has no OPY form inside an opaque settings block");
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    pub(super) fn opaque_group_object(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. }
                    if emittable_member_name(name) =>
                {
                    Some((name.clone(), self.opaque_group_object(children)))
                }
                _ => {
                    self.refuse_settings(
                        member,
                        "has no OPY form inside an uncatalogued settings group",
                    );
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    pub(super) fn settings_workshop(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Raw { name, value, .. } if emittable_member_name(name) => {
                    let json = if value == "On" {
                        Some(SettingsJson::Bool(true))
                    } else if value == "Off" {
                        Some(SettingsJson::Bool(false))
                    } else if emittable_raw_value(value)
                        && crate::compiler::settings::is_number_text(value)
                    {
                        Some(SettingsJson::String(value.clone()))
                    } else if let Some(inner) = value
                        .strip_prefix('[')
                        .and_then(|rest| rest.strip_suffix(']'))
                    {
                        if crate::compile_time::has_unbound_formatters(inner) {
                            None
                        } else {
                            Some(SettingsJson::Array(vec![SettingsJson::String(
                                inner.to_string(),
                            )]))
                        }
                    } else {
                        None
                    };
                    match json {
                        Some(json) => Some((name.clone(), json)),
                        None => {
                            self.refuse_settings(
                                member,
                                "is a `settings.workshop` value OPY cannot write",
                            );
                            None
                        }
                    }
                }
                _ => {
                    self.refuse_settings(
                        member,
                        "is not a `name: value` member inside `settings.workshop`",
                    );
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }
}
