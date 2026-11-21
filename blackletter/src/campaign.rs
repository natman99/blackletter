//! The campaign folder: the unit the app opens.
//!
//! `<root>/characters/` + `<root>/dungeons/` + a small `campaign.json` meta
//! file. Everything the app owns lives under one user-chosen directory, so a
//! campaign is copyable, syncable (Nextcloud), and git-diffable.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Campaign {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CampaignMeta {
    pub name: String,
    /// The edition this campaign was created for. 2014 is the default and the
    /// only corpus wired end-to-end (plan ruling 12).
    pub edition: String,
}

impl Default for CampaignMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            edition: "2014".into(),
        }
    }
}

impl Campaign {
    pub fn characters_dir(&self) -> PathBuf {
        self.root.join("characters")
    }

    pub fn dungeons_dir(&self) -> PathBuf {
        self.root.join("dungeons")
    }

    /// A folder is a campaign when both document directories exist.
    pub fn is_campaign(path: &Path) -> bool {
        path.join("characters").is_dir() && path.join("dungeons").is_dir()
    }

    /// Create the skeleton in `root` (idempotent) and write the meta file if
    /// absent, named after the folder.
    pub fn create(root: PathBuf) -> io::Result<Self> {
        let campaign = Self { root };
        fs::create_dir_all(campaign.characters_dir())?;
        fs::create_dir_all(campaign.dungeons_dir())?;
        let meta_path = campaign.root.join("campaign.json");
        if !meta_path.exists() {
            let name = campaign
                .root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Campaign".into());
            let meta = CampaignMeta {
                name,
                ..Default::default()
            };
            let json = serde_json::to_string_pretty(&meta)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            fs::write(&meta_path, json)?;
        }
        Ok(campaign)
    }

    pub fn meta(&self) -> CampaignMeta {
        fs::read_to_string(self.root.join("campaign.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Document files in a subdirectory, sorted, skipping backups and litter.
    fn list_docs(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("json")
                    && !p.to_string_lossy().ends_with(".bak")
                    && !p.to_string_lossy().contains(".tmp")
            })
            .collect();
        files.sort();
        files
    }

    pub fn list_characters(&self) -> Vec<PathBuf> {
        Self::list_docs(&self.characters_dir())
    }

    pub fn list_dungeons(&self) -> Vec<PathBuf> {
        Self::list_docs(&self.dungeons_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bl_campaign_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn create_makes_skeleton_and_meta() {
        let dir = tmp("create");
        let c = Campaign::create(dir.clone()).unwrap();
        assert!(c.characters_dir().is_dir());
        assert!(c.dungeons_dir().is_dir());
        assert!(Campaign::is_campaign(&dir));
        assert_eq!(c.meta().edition, "2014");
        assert_eq!(c.meta().name, dir.file_name().unwrap().to_string_lossy());
        // Idempotent: a second create keeps the existing meta.
        Campaign::create(dir.clone()).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_folder_is_not_a_campaign() {
        let dir = tmp("plain");
        fs::create_dir_all(&dir).unwrap();
        assert!(!Campaign::is_campaign(&dir));
        fs::create_dir_all(dir.join("characters")).unwrap();
        assert!(!Campaign::is_campaign(&dir)); // both dirs required
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_skips_backups_and_litter() {
        let dir = tmp("listing");
        let c = Campaign::create(dir.clone()).unwrap();
        fs::write(c.dungeons_dir().join("b.json"), "{}").unwrap();
        fs::write(c.dungeons_dir().join("a.json"), "{}").unwrap();
        fs::write(c.dungeons_dir().join("a.json.bak"), "{}").unwrap();
        fs::write(c.dungeons_dir().join(".a.json.tmp"), "{}").unwrap();
        fs::write(c.dungeons_dir().join("notes.txt"), "x").unwrap();
        let listed = c.list_dungeons();
        assert_eq!(
            listed,
            vec![
                c.dungeons_dir().join("a.json"),
                c.dungeons_dir().join("b.json")
            ]
        );
        assert!(c.list_characters().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
