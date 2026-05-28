use crate::{Result, ShelfError};

pub fn validate_address(address: &str) -> Result<()> {
    if address.trim().is_empty() {
        return Err(ShelfError::Validation(
            "source address cannot be empty".into(),
        ));
    }
    if address
        .bytes()
        .any(|b| matches!(b, b'/' | b'\\' | b'\n' | b'\r' | b'%' | 0) || b < 0x20)
    {
        return Err(ShelfError::Validation(
            "source address cannot contain slash, backslash, control characters, NUL, or '%'"
                .into(),
        ));
    }
    Ok(())
}

pub fn validate_source_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(ShelfError::Validation("source name cannot be empty".into()));
    }
    if id
        .bytes()
        .any(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')))
    {
        return Err(ShelfError::Validation(
            "source name may only contain letters, numbers, dash, underscore, and dot".into(),
        ));
    }
    Ok(())
}

pub fn generate_source_id(address: &str, username: &str) -> String {
    let raw = format!("{address}-{username}");
    let mut id = String::with_capacity(raw.len());
    let mut previous_dash = false;

    for ch in raw.chars().flat_map(char::to_lowercase) {
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.');
        let out = if safe { ch } else { '-' };
        if out == '-' {
            if !previous_dash {
                id.push(out);
            }
            previous_dash = true;
        } else {
            id.push(out);
            previous_dash = false;
        }
    }

    id.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_address() {
        assert!(validate_address("192.168.1.10").is_ok());
        assert!(validate_address("nas/home").is_err());
        assert!(validate_address("nas\nhome").is_err());
    }

    #[test]
    fn generates_stable_source_id() {
        assert_eq!(
            generate_source_id("192.168.1.10", "Alice"),
            "192.168.1.10-alice"
        );
        assert_eq!(generate_source_id("nas home", "a/b"), "nas-home-a-b");
    }
}
