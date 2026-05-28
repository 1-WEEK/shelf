use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::IoContext;
use crate::source::validate_source_id;
use crate::{paths, Result, ShelfError};

pub fn credential_path(source_id: &str) -> Result<PathBuf> {
    validate_source_id(source_id)?;
    Ok(paths::credential_file_for(source_id))
}

pub fn write_credential(source_id: &str, username: &str, password: &str) -> Result<PathBuf> {
    validate_credential_value("username", username)?;
    validate_credential_value("password", password)?;
    let path = credential_path(source_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_path(parent)?;
    }
    let temp_path = path.with_extension("cred.tmp");
    let body = format!("username={username}\npassword={password}\n");
    std::fs::write(&temp_path, body).with_path(&temp_path)?;
    std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600))
        .with_path(&temp_path)?;
    std::fs::rename(&temp_path, &path).with_path(&path)?;
    Ok(path)
}

pub fn remove_credential(source_id: &str) -> Result<()> {
    let path = credential_path(source_id)?;
    if path.exists() {
        std::fs::remove_file(&path).with_path(path)?;
    }
    Ok(())
}

pub fn validate_credential_value(label: &str, value: &str) -> Result<()> {
    if value
        .bytes()
        .any(|b| matches!(b, b'\n' | b'\r' | 0) || b < 0x20)
    {
        return Err(ShelfError::Validation(format!(
            "{label} cannot contain control characters, newline, carriage return, or NUL"
        )));
    }
    Ok(())
}

pub fn read_password_from_stdin() -> Result<String> {
    let mut password = String::new();
    std::io::stdin()
        .read_to_string(&mut password)
        .with_path(Path::new("<stdin>"))?;
    while password.ends_with(['\n', '\r']) {
        password.pop();
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_line_injection_in_credentials() {
        assert!(validate_credential_value("username", "alice").is_ok());
        assert!(validate_credential_value("username", "alice\npassword=bad").is_err());
        assert!(validate_credential_value("password", "secret\r").is_err());
    }
}
