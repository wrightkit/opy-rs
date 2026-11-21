use opy_rs::manifest::Manifest;
use serde::Deserialize;
use serde_json::{Value, json};
use workshop_rs::catalog::Catalog;
use workshop_rs::program::MappedText;

use crate::{HandlerError, MAPPED_ARTIFACT_FORMAT, WORKSHOP_ARTIFACT_FORMAT, invalid_artifact};

#[derive(Debug, Deserialize)]
struct ReconstructParams {
    artifact: ReconstructArtifact,
}

#[derive(Debug, Deserialize)]
struct ReconstructArtifact {
    format: String,
    content: String,
}

pub(crate) fn reconstruct(value: Value) -> Result<Value, HandlerError> {
    let params: ReconstructParams =
        serde_json::from_value(value).map_err(|_| HandlerError::invalid_params())?;
    let text = match params.artifact.format.as_str() {
        WORKSHOP_ARTIFACT_FORMAT => params.artifact.content,
        MAPPED_ARTIFACT_FORMAT => {
            MappedText::from_json(&params.artifact.content)
                .map_err(|error| invalid_artifact(error.to_string()))?
                .text
        }
        format => {
            return Err(HandlerError::refusal(
                "reconstruct.artifactFormatUnsupported",
                json!({ "format": format }),
                format!("artifact format is not supported for reconstruction: {format}"),
            ));
        }
    };
    let catalog = Catalog::builtin_ref().map_err(|error| HandlerError::Lpp {
        kind: "providerFailure",
        details: json!({ "code": "catalog-init" }),
        message: format!("cannot load the Workshop catalog: {error}"),
    })?;
    let locale = workshop_rs::detect::resolve_locale(&text, catalog, None)
        .map_err(|error| invalid_artifact(error.to_string()))?;
    let program = workshop_rs::parser::parse(&text, catalog, &locale)
        .map_err(|error| invalid_artifact(error.to_string()))?;
    let manifest = Manifest::builtin().map_err(|error| HandlerError::Lpp {
        kind: "providerFailure",
        details: json!({ "code": "manifest-error" }),
        message: format!("cannot load the OPY semantic compatibility manifest: {error}"),
    })?;
    match opy_rs::reconstruct::reconstruct_with(&program, manifest, catalog, &locale) {
        Ok(source) => Ok(json!({ "source": source })),
        Err(error) => {
            if error
                .issues
                .iter()
                .all(|issue| issue.code == "catalog-error")
            {
                return Err(HandlerError::Lpp {
                    kind: "providerFailure",
                    details: json!({
                        "code": issue_code(&error).unwrap_or("environment")
                    }),
                    message: error.to_string(),
                });
            }
            Err(HandlerError::refusal(
                issue_code(&error).unwrap_or("reconstruct.unsupported"),
                json!({}),
                format!("reconstruct refused: {error}"),
            ))
        }
    }
}

fn issue_code(error: &opy_rs::reconstruct::ReconstructError) -> Option<&'static str> {
    error.issues.first().map(|issue| issue.code)
}
