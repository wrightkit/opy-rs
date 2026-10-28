use std::collections::BTreeMap;

use opy_rs::lookup::{
    EnumMemberEntry, LookupHit, LookupOutcome, LookupParam, LookupQuery, LookupScope, LookupWithin,
    SettingValueForm, lookup as opy_lookup,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{HandlerError, LANGUAGE_ID, Server, invalid_language};

const DEFAULT_LOOKUP_LIMIT: u64 = 20;

const RESOLVE_IDS_MAX_IDS: usize = 1024;

const CALLABLE_IDENTITY: &str = "opy:callable/";
const ENUM_IDENTITY: &str = "opy:enum/";
const ENUM_MEMBER_IDENTITY: &str = "opy:enum-member/";
const SETTING_IDENTITY: &str = "opy:setting/";
const SETTING_PATH_IDENTITY: &str = "opy:setting-path/";
const PARAM_IDENTITY: &str = "opy:param/";
const EVENT_IDENTITY: &str = "opy:event/";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupParams {
    language_id: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    within: Option<LookupWithinParam>,
    #[allow(dead_code)]
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LookupWithinParam {
    kind: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveIdsParams {
    language_id: String,
    #[serde(default)]
    calls: Vec<String>,
    #[serde(default)]
    enums: BTreeMap<String, Vec<String>>,
}

impl Server {
    pub(super) fn lookup(&self, value: Value) -> Result<Value, HandlerError> {
        let within_value = value.get("within").cloned();
        let params: LookupParams =
            serde_json::from_value(value).map_err(|_| HandlerError::invalid_params())?;
        if params.language_id != LANGUAGE_ID {
            return Err(invalid_language(&params.language_id));
        }
        if matches!(params.limit, Some(0)) {
            return Err(HandlerError::invalid_params());
        }
        let within = params.within.as_ref().map(lookup_within).transpose()?;
        let query = LookupQuery {
            text: params.query.unwrap_or_default(),
            scope: LookupScope::ALL,
            within,
            locale: None,
            limit: usize::MAX,
        };
        let scoped = query.within.is_some();
        let mut outcome = opy_lookup(&query);
        if let (LookupOutcome::UnknownWithin { .. }, Some(LookupWithin::Enum(domain))) =
            (&outcome, &query.within)
        {
            outcome = opy_lookup(&LookupQuery {
                within: Some(LookupWithin::SettingEnum(domain.clone())),
                ..query
            });
        }
        match outcome {
            LookupOutcome::Matched { results, .. } => {
                let mut entries: Vec<Value> = results
                    .iter()
                    .map(|hit| lookup_entry(hit, scoped))
                    .collect();
                if let Some(kind) = &params.kind {
                    entries.retain(|entry| entry["kind"] == *kind);
                }
                entries.truncate(
                    usize::try_from(params.limit.unwrap_or(DEFAULT_LOOKUP_LIMIT))
                        .unwrap_or(usize::MAX),
                );
                Ok(json!({ "entries": entries }))
            }
            LookupOutcome::UnknownWithin { .. } => Err(HandlerError::refusal(
                "lookup.unknownWithin",
                json!({ "within": within_value }),
                "within selector names no known scope",
            )),
            LookupOutcome::Unsupported { reason, .. } => Err(HandlerError::Lpp {
                kind: "providerFailure",
                details: json!({ "code": "lookup-unsupported" }),
                message: reason,
            }),
        }
    }

    pub(super) fn resolve_ids(&self, value: Value) -> Result<Value, HandlerError> {
        let params: ResolveIdsParams =
            serde_json::from_value(value).map_err(|_| HandlerError::invalid_params())?;
        if params.language_id != LANGUAGE_ID {
            return Err(invalid_language(&params.language_id));
        }
        let total_ids = params.calls.len() + params.enums.values().map(Vec::len).sum::<usize>();
        if total_ids > RESOLVE_IDS_MAX_IDS {
            return Err(HandlerError::invalid_params());
        }
        let calls: BTreeMap<&String, &str> = params
            .calls
            .iter()
            .filter_map(|id| opy_rs::lookup::call_spelling(id).map(|spelling| (id, spelling)))
            .collect();
        let enums: BTreeMap<&String, BTreeMap<&String, &str>> = params
            .enums
            .iter()
            .filter_map(|(domain, members)| {
                let resolved: BTreeMap<&String, &str> = members
                    .iter()
                    .filter_map(|member| {
                        opy_rs::lookup::enum_member_spelling(domain, member)
                            .map(|spelling| (member, spelling))
                    })
                    .collect();
                (!resolved.is_empty()).then_some((domain, resolved))
            })
            .collect();
        Ok(json!({ "calls": calls, "enums": enums }))
    }
}

fn lookup_within(within: &LookupWithinParam) -> Result<LookupWithin, HandlerError> {
    let value = within.value.as_str();
    Ok(match within.kind.as_str() {
        "callable" => LookupWithin::Callable(identity_value(value, CALLABLE_IDENTITY).to_string()),
        "enum" => LookupWithin::Enum(identity_value(value, ENUM_IDENTITY).to_string()),
        "settings" => LookupWithin::Settings(value.to_string()),
        _ => return Err(HandlerError::invalid_params()),
    })
}

fn identity_value<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.strip_prefix(prefix).unwrap_or(value)
}

fn path_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or_default()
}

fn lookup_entry(hit: &LookupHit, scoped: bool) -> Value {
    match hit {
        LookupHit::Function {
            spelling,
            function_kind,
            receiver,
            params,
            display_name,
            ..
        } => {
            let mut callable = json!({
                "parameters": params.iter().map(callable_param_wire).collect::<Vec<_>>(),
            });
            if let Some(receiver) = receiver {
                callable["receiver"] = serde_json::to_value(receiver).expect("category string");
            }
            json!({
                "identity": format!("{CALLABLE_IDENTITY}{spelling}"),
                "kind": function_kind.as_str(),
                "spelling": spelling,
                "displayName": display_name.as_deref().unwrap_or(spelling),
                "callable": callable,
            })
        }
        LookupHit::EnumDomain {
            domain,
            members,
            display_name,
            ..
        } => {
            let mut entry = json!({
                "identity": format!("{ENUM_IDENTITY}{domain}"),
                "kind": "enum",
                "spelling": domain,
                "displayName": display_name.as_deref().unwrap_or(domain),
            });
            if let Some(members) = members {
                entry["enum"] = enum_wire(domain, member_spellings(members));
            }
            entry
        }
        LookupHit::EnumMember {
            spelling,
            member,
            display_name,
            ..
        } => json!({
            "identity": format!("{ENUM_MEMBER_IDENTITY}{spelling}"),
            "kind": "enumMember",
            "spelling": spelling,
            "displayName": display_name.as_deref().unwrap_or(member),
        }),
        LookupHit::Setting {
            path,
            display_name,
            value,
            ..
        } => json!({
            "identity": format!("{SETTING_IDENTITY}{path}"),
            "kind": "setting",
            "spelling": if scoped { path_segment(path) } else { path },
            "displayName": display_name.as_deref().unwrap_or(path),
            "setting": setting_wire(value),
        }),
        LookupHit::Parameter {
            callable, param, ..
        } => {
            let mut fact = callable_param_wire(param);
            fact.as_object_mut()
                .expect("parameter object")
                .remove("name");
            json!({
                "identity": format!("{PARAM_IDENTITY}{callable}/{}", param.name),
                "kind": "parameter",
                "spelling": param.name,
                "displayName": param.name,
                "parameter": fact,
            })
        }
        LookupHit::SettingPath { path, .. } => json!({
            "identity": format!("{SETTING_PATH_IDENTITY}{path}"),
            "kind": "settingPath",
            "spelling": path_segment(path),
            "displayName": path,
        }),
        LookupHit::Event {
            spelling,
            accepts_filters,
            display_name,
            ..
        } => json!({
            "identity": format!("{EVENT_IDENTITY}{spelling}"),
            "kind": "event",
            "spelling": spelling,
            "displayName": display_name.as_deref().unwrap_or(spelling),
            "event": { "acceptsFilters": accepts_filters },
        }),
    }
}

fn callable_param_wire(param: &LookupParam) -> Value {
    let mut wire = json!({
        "name": param.name,
        "type": param.param_type.as_deref().unwrap_or("any"),
        "required": param.required,
    });
    if let Some(default) = &param.default {
        wire["default"] = json!(default);
    }
    if let Some(domain) = &param.domain {
        wire["enum"] = enum_wire(
            domain,
            param
                .members
                .as_ref()
                .map(|members| member_spellings(members))
                .unwrap_or_default(),
        );
    }
    wire
}

fn setting_wire(value: &SettingValueForm) -> Value {
    let mut wire = json!({ "type": value.kind.as_str() });
    if let Some(min) = value.min {
        wire["minimum"] = json!(min);
    }
    if let Some(max) = value.max {
        wire["maximum"] = json!(max);
    }
    if let Some(domain) = &value.domain {
        wire["enum"] = enum_wire(domain, value.members.clone());
    }
    wire
}

fn member_spellings(members: &[EnumMemberEntry]) -> Vec<String> {
    members
        .iter()
        .map(|member| member.spelling.clone())
        .collect()
}

fn enum_wire(domain: &str, members: Vec<String>) -> Value {
    json!({
        "domain": format!("{ENUM_IDENTITY}{domain}"),
        "members": members,
    })
}
