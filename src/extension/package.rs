// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

//! Extension packages: Chrome's CRX3 (a header, then a zip), Firefox's XPI (a zip) and a
//! plain zip. Unpacked into a folder of the profile with the same rule EON Zip keeps:
//! nothing is written outside that folder, whatever the names inside say.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use super::manifest::Manifest;

/// What kind of file it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Chrome Web Store, Edge Add-ons, Opera add-ons.
    Crx3,
    /// Firefox add-ons (signed zips).
    Xpi,
    Zip,
}

/// An unpacked extension.
#[derive(Debug, Clone)]
pub struct Unpacked {
    pub format: Format,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub files: usize,
}

const MAX_TOTAL: u64 = 256 * 1024 * 1024;

/// The zip inside a package, and its format.
pub fn zip_part(bytes: &[u8]) -> Result<(Format, &[u8]), String> {
    if bytes.starts_with(b"Cr24") {
        if bytes.len() < 12 {
            return Err("a CRX file shorter than its header".into());
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let header = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if version != 3 {
            return Err(format!("CRX version {version}: only CRX3 is read"));
        }
        let start = 12usize
            .checked_add(header)
            .ok_or("a CRX header too large")?;
        let zip = bytes
            .get(start..)
            .ok_or("a CRX header longer than the file")?;
        if !zip.starts_with(b"PK") {
            return Err("no zip after the CRX header".into());
        }
        return Ok((Format::Crx3, zip));
    }
    if bytes.starts_with(b"PK") {
        // A Firefox add-on carries its signature in META-INF/mozilla.rsa.
        let signed = find(bytes, b"META-INF/mozilla.rsa").is_some();
        return Ok((if signed { Format::Xpi } else { Format::Zip }, bytes));
    }
    Err("not an extension package (no CRX or zip header)".into())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Unpack `bytes` into `dir` (created empty). Refuses names that climb out of it, absolute
/// names, and archives that would expand past 256 MiB.
pub fn unpack(bytes: &[u8], dir: &Path) -> Result<Unpacked, String> {
    let (format, zip) = zip_part(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| e.to_string())?;
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    let mut files = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        let Some(rel) = safe_relative(&name) else {
            return Err(format!(
                "refused: `{name}` would be written outside the folder"
            ));
        };
        let out = dir.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        total += entry.size();
        if total > MAX_TOTAL {
            return Err("the package expands past 256 MiB".into());
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
        fs::write(&out, data).map_err(|e| e.to_string())?;
        files += 1;
    }
    let text = fs::read_to_string(dir.join("manifest.json")).map_err(|_| "no manifest.json")?;
    let mut manifest = Manifest::parse(&text)?;
    if let Some(lang) = manifest.default_locale.clone() {
        if let Ok(messages) =
            fs::read_to_string(dir.join("_locales").join(lang).join("messages.json"))
        {
            manifest.localize(&messages);
        }
    }
    Ok(Unpacked {
        format,
        dir: dir.to_path_buf(),
        manifest,
        files,
    })
}

/// A zip entry's name as a path inside the folder, or `None` if it would leave it.
fn safe_relative(name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    let mut out = PathBuf::new();
    for c in Path::new(&name).components() {
        match c {
            Component::Normal(p) => {
                let s = p.to_string_lossy();
                // Windows drops trailing dots and spaces; "x. " would become "x".
                if s.trim_end_matches(['.', ' ']).is_empty() {
                    return None;
                }
                out.push(p);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, body) in files {
                let _ = z.start_file(*name, zip::write::SimpleFileOptions::default());
                let _ = z.write_all(body.as_bytes());
            }
            let _ = z.finish();
        }
        buf.into_inner()
    }

    const MANIFEST: &str =
        r#"{"manifest_version":3,"name":"__MSG_n__","version":"1.0","default_locale":"tr"}"#;

    #[test]
    fn crx3_xpi_and_zip() -> Result<(), String> {
        let inner = zip_of(&[
            ("manifest.json", MANIFEST),
            (
                "_locales/tr/messages.json",
                r#"{"n":{"message":"Yardımcı"}}"#,
            ),
            ("js/c.js", "1"),
        ]);
        let mut crx = b"Cr24".to_vec();
        crx.extend(3u32.to_le_bytes());
        crx.extend(5u32.to_le_bytes());
        crx.extend(b"HDR!!");
        crx.extend(&inner);
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let u = unpack(&crx, &dir.path().join("ext"))?;
        assert_eq!(u.format, Format::Crx3);
        assert_eq!(u.manifest.name, "Yardımcı");
        assert_eq!(u.files, 3);
        assert!(dir.path().join("ext/js/c.js").exists());

        let xpi = zip_of(&[("manifest.json", MANIFEST), ("META-INF/mozilla.rsa", "sig")]);
        assert_eq!(unpack(&xpi, &dir.path().join("x"))?.format, Format::Xpi);
        let plain = zip_of(&[("manifest.json", MANIFEST)]);
        assert_eq!(unpack(&plain, &dir.path().join("z"))?.format, Format::Zip);
        Ok(())
    }

    #[test]
    fn refuses_names_that_climb_out() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        for bad in [
            "../evil.js",
            "/etc/x",
            "C:/x",
            "a/../../x",
            "..\\x",
            "a/.../b",
        ] {
            let z = zip_of(&[("manifest.json", MANIFEST), (bad, "x")]);
            let r = unpack(&z, &dir.path().join("ext"));
            if bad == "a/.../b" {
                assert!(r.is_err(), "`...` is refused");
            } else {
                assert!(r.is_err(), "{bad}");
            }
        }
        assert!(!dir.path().join("evil.js").exists());
        Ok(())
    }

    #[test]
    fn not_a_package() {
        assert!(zip_part(b"hello").is_err());
        let mut crx2 = b"Cr24".to_vec();
        crx2.extend(2u32.to_le_bytes());
        crx2.extend(0u32.to_le_bytes());
        assert!(zip_part(&crx2).is_err());
    }
}
