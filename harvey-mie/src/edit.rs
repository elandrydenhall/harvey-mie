use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SaveError {
    InvalidJson(String),
    Io(io::Error),
    RoundTrip,
    NotUtf8,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::InvalidJson(msg) => write!(f, "Invalid JSON: {msg}"),
            SaveError::Io(err) => write!(f, "{err}"),
            SaveError::RoundTrip => write!(
                f,
                "The trailer read back did not match the editor. The JPEG was restored."
            ),
            SaveError::NotUtf8 => write!(f, "Pin is not UTF-8"),
        }
    }
}

pub struct Loaded {
    pub text: String,
    pub has_pin: bool,
}

pub fn json_error(text: &str) -> Option<String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => None,
        Err(err) => Some(err.to_string()),
    }
}

pub fn list_jpegs(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".jpg") {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub fn load_pin(jpeg: &[u8]) -> Result<Loaded, SaveError> {
    match mie::pin(jpeg) {
        None => Ok(Loaded {
            text: String::new(),
            has_pin: false,
        }),
        Some(bytes) => {
            let text = String::from_utf8(bytes.to_vec()).map_err(|_| SaveError::NotUtf8)?;
            Ok(Loaded {
                text,
                has_pin: true,
            })
        }
    }
}

/// `photo.jpg` -> `photo.jpg.json`
pub fn sibling_json(jpeg: &Path) -> PathBuf {
    let mut name = jpeg.file_name().unwrap_or_default().to_os_string();
    name.push(".json");
    jpeg.with_file_name(name)
}

/// Write `editor_text` into the JPEG trailer and the sibling `.jpg.json`.
///
/// The bytes stored are the editor text. Invalid JSON is refused before either
/// file changes. A read-back mismatch restores the JPEG and leaves the JSON alone.
pub fn save_file(jpeg: &Path, editor_text: &str) -> Result<(), SaveError> {
    if let Some(err) = json_error(editor_text) {
        return Err(SaveError::InvalidJson(err));
    }
    let original = fs::read(jpeg).map_err(SaveError::Io)?;
    let new_jpeg = mie::embed(&original, editor_text.as_bytes());
    if mie::pin(&new_jpeg).as_deref() != Some(editor_text.as_bytes()) {
        return Err(SaveError::RoundTrip);
    }
    commit_verified(jpeg, &original, &new_jpeg, editor_text.as_bytes())?;

    let json_path = sibling_json(jpeg);
    if let Err(err) = replace_with(&json_path, editor_text.as_bytes()) {
        let _ = replace_with(jpeg, &original);
        return Err(SaveError::Io(err));
    }
    Ok(())
}

fn commit_verified(
    path: &Path,
    original: &[u8],
    candidate: &[u8],
    expected_pin: &[u8],
) -> Result<(), SaveError> {
    replace_with(path, candidate).map_err(SaveError::Io)?;
    let on_disk = fs::read(path).unwrap_or_default();
    if mie::pin(&on_disk).as_deref() != Some(expected_pin) {
        replace_with(path, original).map_err(SaveError::Io)?;
        return Err(SaveError::RoundTrip);
    }
    Ok(())
}

/// Write `bytes` to a temp file in the same directory, then replace `path`.
fn replace_with(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = writing_path(path);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    let from = wide(&tmp);
    let to = wide(path);
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        let err = io::Error::last_os_error();
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn writing_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".writing");
    path.with_file_name(name)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[link(name = "kernel32")]
extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &[u8] = b"\xff\xd8HOST\xff\xd9";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pin-edit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn valid_edit_round_trips_and_keeps_the_jpeg_host() {
        let dir = scratch("round");
        let jpeg = dir.join("a.jpg");
        fs::write(&jpeg, HOST).unwrap();
        let text = "{\n  \"image_hash\": \"37d5152f9165b2f3\",\n  \"b\": 1, \"a\": 2\n}\n";
        save_file(&jpeg, text).unwrap();
        let saved = fs::read(&jpeg).unwrap();
        assert_eq!(mie::strip(&saved), HOST);
        assert_eq!(mie::pin(&saved), Some(text.as_bytes()));
        assert_eq!(fs::read(sibling_json(&jpeg)).unwrap(), text.as_bytes());

        let text2 = "{\"a\":2}\n";
        save_file(&jpeg, text2).unwrap();
        let saved = fs::read(&jpeg).unwrap();
        assert_eq!(mie::strip(&saved), HOST);
        assert_eq!(mie::pin(&saved), Some(text2.as_bytes()));
        assert_eq!(fs::read(sibling_json(&jpeg)).unwrap(), text2.as_bytes());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_json_changes_nothing() {
        let dir = scratch("invalid");
        let jpeg = dir.join("a.jpg");
        let original = mie::embed(HOST, br#"{"ok":true}"#);
        fs::write(&jpeg, &original).unwrap();
        fs::write(sibling_json(&jpeg), br#"{"ok":true}"#).unwrap();
        let err = save_file(&jpeg, "{").unwrap_err();
        assert!(matches!(err, SaveError::InvalidJson(_)));
        assert_eq!(fs::read(&jpeg).unwrap(), original);
        assert_eq!(fs::read(sibling_json(&jpeg)).unwrap(), br#"{"ok":true}"#);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jpeg_without_a_trailer_gains_one() {
        let dir = scratch("create");
        let jpeg = dir.join("a.jpg");
        fs::write(&jpeg, HOST).unwrap();
        assert!(mie::pin(HOST).is_none());
        let text = "{\"n\":1}";
        save_file(&jpeg, text).unwrap();
        let saved = fs::read(&jpeg).unwrap();
        assert_eq!(mie::strip(&saved), HOST);
        assert_eq!(mie::pin(&saved), Some(text.as_bytes()));
        assert_eq!(fs::read(sibling_json(&jpeg)).unwrap(), text.as_bytes());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_read_back_restores_the_jpeg() {
        let dir = scratch("restore");
        let jpeg = dir.join("a.jpg");
        let original = b"\xff\xd8ORIGINAL\xff\xd9";
        fs::write(&jpeg, original).unwrap();
        let err = commit_verified(&jpeg, original, b"not-a-trailer", b"{}").unwrap_err();
        assert!(matches!(err, SaveError::RoundTrip));
        assert_eq!(fs::read(&jpeg).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_jpegs_skips_json_sidecars() {
        let dir = scratch("list");
        fs::write(dir.join("b.jpg"), b"x").unwrap();
        fs::write(dir.join("a.jpg"), b"x").unwrap();
        fs::write(dir.join("a.jpg.json"), b"{}").unwrap();
        fs::write(dir.join("note.txt"), b"x").unwrap();
        assert_eq!(
            list_jpegs(&dir).unwrap(),
            vec!["a.jpg".to_string(), "b.jpg".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
