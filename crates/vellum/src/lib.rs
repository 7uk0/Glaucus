//! # vellum — the resource layer
//!
//! The parchment everything else is written on. Mirrors RobustToolbox's
//! `IResourceManager`: a stack of *resource roots* (content `Resources/`
//! directories) addressed by rooted, forward-slash [`ResPath`]s such as
//! `/Textures/Tiles/steel.png` or `/Prototypes/Entities/Mobs/NPCs/carp.yml`.
//!
//! Later roots override earlier ones, which is how a fork (Frontier) layers
//! on top of upstream (SS14) and engine (RobustToolbox) resources.
//!
//! Also home to the RSI ("Robust Station Image") sprite-sheet format in
//! [`rsi`] and a minimal Fluent reader in [`locale`].

pub mod locale;
pub mod rsi;

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VellumError {
    #[error("resource not found: {0}")]
    NotFound(ResPath),
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("bad RSI {path}: {msg}")]
    Rsi { path: ResPath, msg: String },
}

/// A rooted resource path (`/Textures/foo.rsi`). Always starts with `/`,
/// always uses `/` separators, never contains `..`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResPath(String);

impl ResPath {
    pub fn new(p: impl AsRef<str>) -> Self {
        let p = p.as_ref().replace('\\', "/");
        let mut out: Vec<&str> = Vec::new();
        for seg in p.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    out.pop();
                }
                s => out.push(s),
            }
        }
        ResPath(format!("/{}", out.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Join a relative path onto this one.
    pub fn join(&self, rel: impl AsRef<str>) -> ResPath {
        let rel = rel.as_ref();
        if rel.starts_with('/') {
            return ResPath::new(rel);
        }
        ResPath::new(format!("{}/{}", self.0, rel))
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rfind('.').map(|i| &name[i + 1..])
    }

    /// Path without the leading `/`, suitable for joining onto a root dir.
    pub fn relative(&self) -> &str {
        &self.0[1..]
    }

    /// SS14 sprite paths in prototypes are relative to `/Textures/` unless rooted.
    pub fn texture(p: &str) -> ResPath {
        if p.starts_with('/') {
            ResPath::new(p)
        } else {
            ResPath::new(format!("/Textures/{p}"))
        }
    }
}

impl fmt::Display for ResPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ResPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResPath({})", self.0)
    }
}

/// An ordered stack of resource roots. Lookups search from the *last* root
/// added to the first, so later roots override.
#[derive(Clone, Debug, Default)]
pub struct Resources {
    roots: Vec<PathBuf>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.add_root(root);
        self
    }

    pub fn add_root(&mut self, root: impl Into<PathBuf>) {
        self.roots.push(root.into());
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Locate a resource on disk.
    pub fn resolve(&self, path: &ResPath) -> Option<PathBuf> {
        self.roots
            .iter()
            .rev()
            .map(|r| r.join(path.relative()))
            .find(|p| p.exists())
    }

    pub fn exists(&self, path: &ResPath) -> bool {
        self.resolve(path).is_some()
    }

    pub fn read(&self, path: &ResPath) -> Result<Vec<u8>, VellumError> {
        let disk = self
            .resolve(path)
            .ok_or_else(|| VellumError::NotFound(path.clone()))?;
        std::fs::read(&disk).map_err(|source| VellumError::Io { path: disk, source })
    }

    pub fn read_string(&self, path: &ResPath) -> Result<String, VellumError> {
        let bytes = self.read(path)?;
        // Strip a UTF-8 BOM; several upstream files carry one.
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Recursively list every file under `dir` (across all roots) whose
    /// extension matches `ext`. Later roots shadow identical paths from
    /// earlier ones. Results are sorted for determinism.
    pub fn find_files(&self, dir: &ResPath, ext: &str) -> Vec<ResPath> {
        let mut found = std::collections::BTreeSet::new();
        for root in &self.roots {
            let base = root.join(dir.relative());
            if !base.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&base).follow_links(true) {
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_file() {
                    continue;
                }
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some(ext) {
                    continue;
                }
                // Skip dotfiles, like Robust does.
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'))
                {
                    continue;
                }
                if let Ok(rel) = p.strip_prefix(root) {
                    found.insert(ResPath::new(rel.to_string_lossy()));
                }
            }
        }
        found.into_iter().collect()
    }

    /// Convenience: the default layout produced by `tools/summon-upstream.sh`.
    /// Engine resources first, then the content fork on top.
    pub fn from_upstream_dir(upstream: &Path) -> Self {
        let mut res = Resources::new();
        let engine = upstream.join("RobustToolbox/Resources");
        if engine.is_dir() {
            res.add_root(engine);
        }
        let content = upstream.join("frontier/Resources");
        if content.is_dir() {
            res.add_root(content);
        }
        res
    }
}

/// Find the upstream checkout: `$CAPUT_UPSTREAM`, else `./upstream`, else
/// walk up from the current directory looking for an `upstream/` dir.
pub fn locate_upstream() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CAPUT_UPSTREAM") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("upstream");
        if candidate.join("frontier/Resources").is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respath_normalizes() {
        assert_eq!(ResPath::new("Textures//a/./b/../c.png").as_str(), "/Textures/a/c.png");
        assert_eq!(ResPath::new("/").as_str(), "/");
        assert_eq!(ResPath::texture("Mobs/x.rsi").as_str(), "/Textures/Mobs/x.rsi");
        assert_eq!(ResPath::texture("/Textures/y.rsi").as_str(), "/Textures/y.rsi");
        assert_eq!(ResPath::new("/a/b.rsi").join("c.png").as_str(), "/a/b.rsi/c.png");
        assert_eq!(ResPath::new("/a/b.yml").extension(), Some("yml"));
    }
}
