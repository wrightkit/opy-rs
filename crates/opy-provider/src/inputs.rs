use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::resolved_path;
use crate::uri::path_to_file_uri;

/// The declared inputs of an entry-based compile (LPP `compileInputs`):
/// every path the compile accessed — the file registry under `display_root`
/// plus the accessed-paths record — in the form each was accessed, every
/// symbolic link followed while resolving it, and each link's resolved path.
pub(crate) fn compile_inputs(
    display_root: &Path,
    paths: &[String],
    accessed: impl IntoIterator<Item = PathBuf>,
) -> Vec<String> {
    let mut declared = BTreeSet::new();
    for path in paths
        .iter()
        .map(|path| resolved_path(display_root, path))
        .chain(accessed)
    {
        if path.is_absolute() {
            let resolved = resolve(&path, &mut declared);
            declared.insert(resolved);
        }
        declared.insert(path);
    }
    let mut uris: Vec<String> = declared.iter().map(|path| path_to_file_uri(path)).collect();
    uris.sort();
    uris.dedup();
    uris
}

/// Follow `path` component by component like the reference mock provider's
/// `resolve` and record every symbolic link on the way — including links in
/// a chain and linked directory components — plus the path each link
/// resolves to.
fn resolve(path: &Path, declared: &mut BTreeSet<PathBuf>) -> PathBuf {
    let mut hops = 0;
    resolve_inner(path, declared, &mut hops)
}

const MAX_LINK_HOPS: u32 = 40;

fn resolve_inner(path: &Path, declared: &mut BTreeSet<PathBuf>, hops: &mut u32) -> PathBuf {
    let mut resolved = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let next = resolved.join(name);
                match std::fs::read_link(&next) {
                    Ok(target) if *hops < MAX_LINK_HOPS => {
                        *hops += 1;
                        declared.insert(next);
                        let target = resolved.join(target);
                        declared.insert(normalized(&target));
                        resolved = resolve_inner(&target, declared, hops);
                    }
                    _ => resolved = next,
                }
            }
            Component::ParentDir => {
                resolved.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    resolved
}

fn normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
