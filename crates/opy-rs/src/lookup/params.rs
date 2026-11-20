//! Parameter facts of a manifest function: names, catalog types, domains,
//! defaults, and catalog descriptions.

use super::{Index, LookupParam, default_text, lookup_kind, member_entry};
use crate::manifest::{Function, Param};

impl Index<'_> {
    pub(super) fn lookup_params(&self, function: &Function) -> Vec<LookupParam> {
        let catalog_entry = function
            .catalog_id
            .as_deref()
            .and_then(|catalog_id| self.catalog.entry(lookup_kind(function.kind), catalog_id));
        function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let catalog_index = self.catalog_param_index(catalog_entry, function, param, index);
                let (param_type, catalog_domain) = catalog_index
                    .and_then(|i| {
                        catalog_entry.map(|entry| {
                            (
                                entry.param_type(i).map(str::to_string),
                                entry.param_domain(i).map(str::to_string),
                            )
                        })
                    })
                    .unwrap_or((None, None));
                // The reported domain is the reference source spelling:
                // a catalog-declared `Clipping` slot renders as `Clip`.
                let domain = param
                    .domain
                    .clone()
                    .or(catalog_domain)
                    .map(|domain| crate::enums::opy_domain(&domain).to_string());
                let required = !param.optional && param.default.is_none();
                let members = domain
                    .as_deref()
                    .and_then(|domain| crate::enums::domain_members(domain, self.catalog))
                    .map(|members| members.iter().map(member_entry).collect::<Vec<_>>());
                let default = param
                    .default
                    .as_ref()
                    .map(|d| default_text(d, domain.as_deref(), self.catalog));
                let description = catalog_index
                    .zip(function.catalog_id.as_deref())
                    .and_then(|(position, catalog_id)| {
                        self.catalog.param_description(
                            lookup_kind(function.kind),
                            catalog_id,
                            position,
                            &crate::enums::en_us(),
                        )
                    })
                    .map(|description| description.text);
                LookupParam {
                    name: param.name.clone(),
                    alternate_names: param.alternate_names.clone(),
                    required,
                    members,
                    domain,
                    param_type,
                    default,
                    description,
                }
            })
            .collect()
    }

    fn catalog_param_index(
        &self,
        entry: Option<&workshop_rs::catalog::CatalogEntry>,
        function: &Function,
        param: &Param,
        index: usize,
    ) -> Option<usize> {
        let entry = entry?;
        let count = entry.param_count();
        let offset = usize::from(function.kind.is_member() && count == function.params.len() + 1);
        let folded = crate::matcher::fold(&param.name);
        let by_name = |i: usize| {
            entry
                .param_name(i)
                .is_some_and(|name| crate::matcher::fold(name) == folded)
        };
        if count == function.params.len() + offset {
            let candidate = index + offset;
            match (0..count).position(&by_name) {
                Some(i) => Some(i),
                None => (candidate < count).then_some(candidate),
            }
        } else {
            (0..count).find(|&i| by_name(i))
        }
    }
}
