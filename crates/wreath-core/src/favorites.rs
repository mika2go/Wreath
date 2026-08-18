use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    clips: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorites {
    file: PathBuf,
    root: PathBuf,
    keys: BTreeSet<String>,
}

impl Favorites {
    pub fn load(file: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let keys = fs::read_to_string(&file)
            .ok()
            .and_then(|contents| serde_json::from_str::<Stored>(&contents).ok())
            .map(|stored| stored.clips.into_iter().collect())
            .unwrap_or_default();
        Self {
            file,
            root: root.into(),
            keys,
        }
    }

    pub fn set_root(&mut self, root: impl Into<PathBuf>) {
        self.root = root.into();
    }

    pub fn contains(&self, clip: &Path) -> bool {
        self.key(clip).is_some_and(|key| self.keys.contains(&key))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn toggle(&mut self, clip: &Path) -> bool {
        let Some(key) = self.key(clip) else {
            return false;
        };
        if !self.keys.remove(&key) {
            self.keys.insert(key);
            return true;
        }
        false
    }

    pub fn remove(&mut self, clip: &Path) {
        if let Some(key) = self.key(clip) {
            self.keys.remove(&key);
        }
    }

    pub fn relocate(&mut self, from: &Path, to: &Path) {
        let Some(previous) = self.key(from) else {
            return;
        };
        if !self.keys.remove(&previous) {
            return;
        }
        if let Some(next) = self.key(to) {
            self.keys.insert(next);
        }
    }

    pub fn retain_existing(&mut self, clips: &[PathBuf]) {
        let existing = clips
            .iter()
            .filter_map(|clip| self.key(clip))
            .collect::<BTreeSet<_>>();
        self.keys.retain(|key| existing.contains(key));
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let stored = Stored {
            clips: self.keys.iter().cloned().collect(),
        };
        let contents = serde_json::to_string_pretty(&stored)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let scratch = self.file.with_extension("json.tmp");
        fs::write(&scratch, contents)?;
        fs::rename(&scratch, &self.file)
    }

    fn key(&self, clip: &Path) -> Option<String> {
        let relative = clip.strip_prefix(&self.root).unwrap_or(clip);
        let mut key = String::new();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            if !key.is_empty() {
                key.push('/');
            }
            key.push_str(&part.to_string_lossy());
        }
        (!key.is_empty()).then_some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("wreath-favorites-{name}"));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        directory
    }

    #[test]
    fn a_toggled_clip_survives_a_reload() {
        let directory = scratch("reload");
        let root = directory.join("clips");
        let file = directory.join("favorites.json");
        let clip = root.join("Night Drive.mp4");

        let mut favorites = Favorites::load(&file, &root);
        assert!(favorites.toggle(&clip));
        assert!(favorites.contains(&clip));
        favorites.save().expect("favorites are written");

        let reloaded = Favorites::load(&file, &root);
        assert!(reloaded.contains(&clip));

        let mut reloaded = reloaded;
        assert!(!reloaded.toggle(&clip));
        assert!(!reloaded.contains(&clip));
    }

    #[test]
    fn keys_stay_relative_so_a_moved_clip_directory_keeps_its_favorites() {
        let directory = scratch("relative");
        let file = directory.join("favorites.json");
        let clip = directory.join("clips").join("Sammlung").join("Ace.mp4");

        let mut favorites = Favorites::load(&file, directory.join("clips"));
        favorites.toggle(&clip);
        favorites.save().expect("favorites are written");

        let contents = fs::read_to_string(&file).expect("favorites file");
        assert!(contents.contains("Sammlung/Ace.mp4"));

        let moved_root = directory.join("videos");
        let moved = Favorites::load(&file, &moved_root);
        assert!(moved.contains(&moved_root.join("Sammlung").join("Ace.mp4")));
    }

    #[test]
    fn a_renamed_clip_keeps_its_star_and_a_deleted_clip_loses_it() {
        let directory = scratch("relocate");
        let root = directory.join("clips");
        let file = directory.join("favorites.json");
        let before = root.join("Clip.mp4");
        let after = root.join("Mountain Clutch.mp4");

        let mut favorites = Favorites::load(&file, &root);
        favorites.toggle(&before);
        favorites.relocate(&before, &after);
        assert!(!favorites.contains(&before));
        assert!(favorites.contains(&after));

        favorites.remove(&after);
        assert!(favorites.is_empty());
    }

    #[test]
    fn clips_that_no_longer_exist_are_dropped() {
        let directory = scratch("retain");
        let root = directory.join("clips");
        let file = directory.join("favorites.json");
        let kept = root.join("Kept.mp4");
        let gone = root.join("Gone.mp4");

        let mut favorites = Favorites::load(&file, &root);
        favorites.toggle(&kept);
        favorites.toggle(&gone);
        favorites.retain_existing(std::slice::from_ref(&kept));

        assert!(favorites.contains(&kept));
        assert_eq!(favorites.len(), 1);
    }

    #[test]
    fn a_missing_or_broken_file_starts_empty() {
        let directory = scratch("broken");
        let root = directory.join("clips");
        let file = directory.join("favorites.json");
        fs::write(&file, "not json").expect("write");

        assert!(Favorites::load(&file, &root).is_empty());
        assert!(Favorites::load(directory.join("absent.json"), &root).is_empty());
    }
}
