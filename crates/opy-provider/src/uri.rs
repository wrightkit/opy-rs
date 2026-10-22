use std::path::{Path, PathBuf};

pub(crate) fn filesystem_path(value: &str) -> Option<PathBuf> {
    if value.starts_with("file://") {
        return file_uri_path(value);
    }
    // A `scheme:` prefix makes `value` a URI, not a filesystem path — a
    // document URI like `untitled:Untitled-1` must not silently resolve
    // against the provider's working directory. A single letter before ':'
    // is a Windows drive (`C:`), not a scheme.
    if let Some(end) = value.find(':') {
        let scheme = &value[..end];
        if scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
    }
    Some(PathBuf::from(value))
}

pub(crate) fn file_uri_path(value: &str) -> Option<PathBuf> {
    let path = value.strip_prefix("file://")?;
    if !path.starts_with('/') {
        return None;
    }
    let path = percent_decode(path)?;
    #[cfg(windows)]
    let path = path
        .strip_prefix('/')
        .filter(|path| {
            let bytes = path.as_bytes();
            bytes.first().is_some_and(u8::is_ascii_alphabetic)
                && bytes.get(1) == Some(&b':')
                && bytes.get(2) == Some(&b'/')
        })
        .unwrap_or(&path);
    Some(PathBuf::from(path))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|byte| (*byte as char).to_digit(16))?;
            let low = bytes
                .get(index + 2)
                .and_then(|byte| (*byte as char).to_digit(16))?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub(crate) fn path_to_file_uri(path: &Path) -> String {
    // Windows paths arrive as `E:\dir\f` or the verbatim `\\?\E:\dir\f`; the
    // URI form is `file:///E:/dir/f`.
    let raw = path.to_string_lossy().into_owned();
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(&raw).replace('\\', "/");
    let drive = raw.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && raw.as_bytes().get(1) == Some(&b':');
    let mut uri = String::from(if drive { "file:///" } else { "file://" });
    if drive {
        uri.push_str(&raw[..2]);
    }
    for byte in raw[if drive { 2 } else { 0 }..].bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push('%');
            uri.push(
                char::from_digit(u32::from(byte >> 4), 16)
                    .expect("hex digit")
                    .to_ascii_uppercase(),
            );
            uri.push(
                char::from_digit(u32::from(byte & 0x0f), 16)
                    .expect("hex digit")
                    .to_ascii_uppercase(),
            );
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_round_trips_spaces_and_unicode() {
        let path = Path::new("/project/中文 file.opy");
        assert_eq!(
            filesystem_path(&path_to_file_uri(path)),
            Some(path.to_path_buf())
        );
    }

    #[test]
    fn file_uri_encodes_windows_paths_with_a_drive_and_no_host() {
        for raw in [
            r"E:\Work\Bastion\src\main.opy",
            r"\\?\E:\Work\Bastion\src\main.opy",
            "E:/Work/Bastion/src/main.opy",
        ] {
            assert_eq!(
                path_to_file_uri(Path::new(raw)),
                "file:///E:/Work/Bastion/src/main.opy"
            );
        }
    }

    #[test]
    fn file_uri_path_handles_windows_drive_letter_uris() {
        let path = file_uri_path("file:///D:/path/to/project.opy").expect("file URI path");
        let expected = if cfg!(windows) {
            PathBuf::from("D:/path/to/project.opy")
        } else {
            PathBuf::from("/D:/path/to/project.opy")
        };
        assert_eq!(path, expected);
    }
}
