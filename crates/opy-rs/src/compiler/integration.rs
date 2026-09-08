//! Manifest/catalog validation and the released Workshop integration contract.

use super::*;

/// Results of the manifest-to-catalog cross-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkReport {
    pub catalog_ids_checked: usize,
    pub domains_checked: usize,
}

/// Cross-check every OPY manifest `catalogId` and domain identity against the
/// canonical Workshop catalog. No local catalog copy or spelling allowlist is
/// involved.
pub(crate) fn cross_check_manifest(
    manifest: &Manifest,
    catalog: &Catalog,
) -> Result<LinkReport, IntegrationError> {
    let mut catalog_ids_checked = 0;
    let mut domains_checked = 0;

    for function in &manifest.functions {
        if let Some(catalog_id) = &function.catalog_id {
            let kind = match function.kind {
                FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
            };
            catalog_ids_checked += 1;
            if catalog.entry(kind, catalog_id).is_none() {
                return Err(IntegrationError::new(
                    "catalog-link-missing",
                    format!(
                        "manifest function '{}' links to missing {:?} catalog id '{}'",
                        function.id, kind, catalog_id
                    ),
                    None,
                ));
            }
        }

        for parameter in &function.params {
            let Some(domain) = &parameter.domain else {
                continue;
            };
            if crate::lower::policy::is_contextual_domain(domain) {
                continue;
            }
            domains_checked += 1;
            if catalog.enum_domain(domain).is_none() {
                return Err(IntegrationError::new(
                    "domain-link-missing",
                    format!(
                        "manifest function '{}' parameter '{}' links to missing enum domain '{}',",
                        function.id, parameter.name, domain
                    ),
                    None,
                ));
            }
        }

        if let Some(contextual) = crate::lower::policy::contextual_domain(&function.id) {
            for option in contextual.options {
                domains_checked += 1;
                if catalog.enum_domain(option.domain).is_none() {
                    return Err(IntegrationError::new(
                        "domain-link-missing",
                        format!(
                            "manifest function '{}' contextual option links to missing enum domain '{}'",
                            function.id, option.domain
                        ),
                        None,
                    ));
                }
            }
        }
    }

    Ok(LinkReport {
        catalog_ids_checked,
        domains_checked,
    })
}

pub(crate) fn load_compiler_contract()
-> Result<(Catalog, &'static Manifest, LinkReport), IntegrationError> {
    let catalog = Catalog::builtin()
        .map_err(|error| IntegrationError::new("catalog-load", error.to_string(), None))?;
    let manifest = Manifest::builtin()
        .map_err(|error| IntegrationError::new("manifest-load", error.to_string(), None))?;
    let links = cross_check_manifest(manifest, &catalog)?;
    let identity = catalog.identity();
    if identity.implementation_version != WORKSHOP_RS_VERSION {
        return Err(IntegrationError::new(
            "workshop-contract-version",
            format!(
                "expected workshop-rs {}, loaded {}",
                WORKSHOP_RS_VERSION, identity.implementation_version
            ),
            None,
        ));
    }
    Ok((catalog, manifest, links))
}
