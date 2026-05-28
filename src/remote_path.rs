use std::path::{Path, PathBuf};

use crate::{Result, ShelfError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePath {
    raw: String,
    top_level: String,
    remainder: Vec<String>,
}

impl RemotePath {
    pub fn parse(input: &str) -> Result<Self> {
        if !input.starts_with('/') {
            return Err(ShelfError::Validation(
                "remote path must start with /, for example /media/movies".into(),
            ));
        }
        if input
            .bytes()
            .any(|b| matches!(b, b'\\' | b'\n' | b'\r' | b'%' | 0) || b < 0x20)
        {
            return Err(ShelfError::Validation(
                "remote path cannot contain backslash, control characters, NUL, or '%'".into(),
            ));
        }

        let mut segments = Vec::new();
        for segment in input.split('/').skip(1) {
            if segment.is_empty() || matches!(segment, "." | "..") {
                return Err(ShelfError::Validation(
                    "remote path cannot contain empty, '.', or '..' segments".into(),
                ));
            }
            segments.push(segment.to_string());
        }

        if segments.is_empty() {
            return Err(ShelfError::Validation(
                "remote path must include at least one path segment".into(),
            ));
        }

        let top_level = segments[0].clone();
        let remainder = segments[1..].to_vec();
        Ok(Self {
            raw: input.to_string(),
            top_level,
            remainder,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn top_level(&self) -> &str {
        &self.top_level
    }

    pub fn remainder(&self) -> &[String] {
        &self.remainder
    }

    pub fn bind_source_under(&self, mount_root: &Path) -> PathBuf {
        let mut out = mount_root.to_path_buf();
        for segment in &self.remainder {
            out.push(segment);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_path_without_exposing_smb_share() {
        let parsed = RemotePath::parse("/media/movies/2026").unwrap();
        assert_eq!(parsed.top_level(), "media");
        assert_eq!(
            parsed.remainder(),
            &["movies".to_string(), "2026".to_string()]
        );
        assert_eq!(
            parsed.bind_source_under(Path::new("/mnt/shelf/sources/home/media")),
            PathBuf::from("/mnt/shelf/sources/home/media/movies/2026")
        );
    }

    #[test]
    fn rejects_unsafe_remote_path() {
        assert!(RemotePath::parse("media/movies").is_err());
        assert!(RemotePath::parse("/").is_err());
        assert!(RemotePath::parse("/media//movies").is_err());
        assert!(RemotePath::parse("/media/../movies").is_err());
        assert!(RemotePath::parse("/media\\movies").is_err());
        assert!(RemotePath::parse("/media/%n").is_err());
    }
}
