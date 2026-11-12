use chrono::NaiveDate;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::domain::dedup_models::{CanonicalPathStatus, DriveConfiguration, DriveDefinition};
use crate::domain::errors::DeduplicationError;

/// Loader and validator for drive configuration files (`drives.yaml`, `drives.toml`, `drives.json`).
pub struct DrivesConfigLoader;

impl DrivesConfigLoader {
    /// Interpolates environment variables into a raw configuration string.
    /// Supports `${VAR}` and `${VAR:-default}` syntax.
    pub fn interpolate_env_vars(content: &str) -> String {
        let mut result = String::with_capacity(content.len());
        let mut chars = content.char_indices().peekable();

        while let Some((_, c)) = chars.next() {
            if c == '$' {
                if let Some(&(_, '{')) = chars.peek() {
                    chars.next(); // consume '{'
                    let mut var_expr = String::new();
                    let mut closed = false;
                    for (_, ch) in chars.by_ref() {
                        if ch == '}' {
                            closed = true;
                            break;
                        }
                        var_expr.push(ch);
                    }

                    if closed {
                        let (var_name, default_val) = match var_expr.split_once(":-") {
                            Some((name, def)) => (name.trim(), Some(def)),
                            None => (var_expr.trim(), None),
                        };

                        let val = std::env::var(var_name).ok();
                        match val {
                            Some(ref v) if !v.is_empty() => result.push_str(v),
                            _ => {
                                if let Some(def) = default_val {
                                    result.push_str(def);
                                } else if let Some(ref v) = val {
                                    result.push_str(v);
                                }
                            }
                        }
                        continue;
                    } else {
                        result.push('$');
                        result.push('{');
                        result.push_str(&var_expr);
                        continue;
                    }
                }
            }
            result.push(c);
        }

        result
    }

    /// Loads and validates a drive configuration from a file path.
    pub fn load_from_path(path: &Path) -> Result<DriveConfiguration, DeduplicationError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            DeduplicationError::ConfigError(format!(
                "Failed to read drive config from '{}': {}",
                path.display(),
                e
            ))
        })?;

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("yaml")
            .to_lowercase();

        Self::load_from_str(&content, Some(&extension))
    }

    /// Loads and validates configuration from string content.
    /// Environment variables (${VAR} or ${VAR:-default}) are automatically interpolated.
    pub fn load_from_str(
        content: &str,
        format_hint: Option<&str>,
    ) -> Result<DriveConfiguration, DeduplicationError> {
        let interpolated = Self::interpolate_env_vars(content);
        let hint = format_hint.unwrap_or("yaml").to_lowercase();
        let config: DriveConfiguration = match hint.as_str() {
            "json" => serde_json::from_str(&interpolated).map_err(|e| {
                DeduplicationError::ConfigError(format!("Invalid JSON drive config: {}", e))
            })?,
            _ => serde_yaml::from_str(&interpolated).map_err(|e| {
                DeduplicationError::ConfigError(format!("Invalid YAML drive config: {}", e))
            })?,
        };

        Self::validate(&config)?;
        Ok(config)
    }

    /// Resolves the default master directory under a drive mount path.
    /// Prefers `<mount>/media/master` if that directory exists on disk,
    /// otherwise defaults to `<mount>/SPSA/media/master`.
    pub fn resolve_default_master_path(mount_path: &Path) -> PathBuf {
        let candidate_short = mount_path.join("media").join("master");
        if candidate_short.is_dir() {
            candidate_short
        } else {
            mount_path.join("SPSA").join("media").join("master")
        }
    }

    /// Synthesizes a DriveConfiguration directly from ambient environment variables (.env).
    /// Looks for:
    /// - `DRIVE_PRIMARY_MOUNT` (required)
    /// - `DRIVE_PRIMARY_MASTER` (optional, defaults to `<mount>/SPSA/media/master`)
    /// - `DRIVE_PRIMARY_ID` (optional, defaults to "primary_main")
    /// - `DRIVE_REPLICA<N>_MOUNT` for N in 1..=10 (optional)
    /// - `DRIVE_REPLICA<N>_MASTER` (optional)
    /// - `DRIVE_REPLICA<N>_ID` (optional, defaults to "replica_<N>")
    pub fn synthesize_from_env() -> Result<DriveConfiguration, DeduplicationError> {
        let primary_mount = std::env::var("DRIVE_PRIMARY_MOUNT")
            .or_else(|_| std::env::var("PRIMARY_MOUNT_PATH"))
            .map_err(|_| {
                DeduplicationError::ConfigError(
                    "Drive configuration not found. Please provide a configuration file (--config <PATH>) or define 'DRIVE_PRIMARY_MOUNT' in your environment / .env file. See drives.example.yaml.".to_string(),
                )
            })?;

        let primary_mount_trimmed = primary_mount.trim();
        if primary_mount_trimmed.is_empty() {
            return Err(DeduplicationError::ConfigError(
                "Environment variable 'DRIVE_PRIMARY_MOUNT' cannot be empty.".to_string(),
            ));
        }

        let primary_mount_path = PathBuf::from(primary_mount_trimmed);
        let primary_master_path = std::env::var("DRIVE_PRIMARY_MASTER")
            .or_else(|_| std::env::var("PRIMARY_MASTER_PATH"))
            .map(|s| PathBuf::from(s.trim()))
            .unwrap_or_else(|_| Self::resolve_default_master_path(&primary_mount_path));

        let primary_id =
            std::env::var("DRIVE_PRIMARY_ID").unwrap_or_else(|_| "primary_main".to_string());

        let mut groups = std::collections::HashMap::new();
        groups.insert(
            "primary".to_string(),
            crate::domain::dedup_models::DriveGroupSpec {
                priority: 100,
                description: Some("Primary master drive group".to_string()),
            },
        );

        let mut drives = vec![DriveDefinition {
            id: primary_id,
            group: "primary".to_string(),
            mount_path: primary_mount_path,
            master_path: primary_master_path,
        }];

        // Scan for replicas 1 through 10
        for n in 1..=10 {
            let replica_key = format!("DRIVE_REPLICA{}_MOUNT", n);
            let replica_alt_key = format!("REPLICA{}_MOUNT_PATH", n);
            if let Ok(mount_str) =
                std::env::var(&replica_key).or_else(|_| std::env::var(&replica_alt_key))
            {
                let trimmed = mount_str.trim();
                if !trimmed.is_empty() {
                    let mount_path = PathBuf::from(trimmed);
                    let master_key = format!("DRIVE_REPLICA{}_MASTER", n);
                    let master_alt_key = format!("REPLICA{}_MASTER_PATH", n);
                    let master_path = std::env::var(&master_key)
                        .or_else(|_| std::env::var(&master_alt_key))
                        .map(|s| PathBuf::from(s.trim()))
                        .unwrap_or_else(|_| Self::resolve_default_master_path(&mount_path));

                    let group_name = format!("replica{}", n);
                    let priority = 80u32.saturating_sub((n - 1) * 10);
                    groups.insert(
                        group_name.clone(),
                        crate::domain::dedup_models::DriveGroupSpec {
                            priority,
                            description: Some(format!("Replica {} group", n)),
                        },
                    );

                    let drive_id = std::env::var(format!("DRIVE_REPLICA{}_ID", n))
                        .unwrap_or_else(|_| format!("replica_{}", n));

                    drives.push(DriveDefinition {
                        id: drive_id,
                        group: group_name,
                        mount_path,
                        master_path,
                    });
                }
            }
        }

        let config = DriveConfiguration {
            version: "1.0".to_string(),
            groups,
            drives,
            rules: crate::domain::dedup_models::DriveRules::default(),
        };

        Self::validate(&config)?;
        Ok(config)
    }

    /// Loads drive configuration with fallback order:
    /// 1. If explicit `path` is passed and exists, load and interpolate that file.
    /// 2. If `path` is `None` or points to a non-existent default `drives.yaml`,
    ///    attempt to synthesize the configuration from environment variables (.env).
    /// 3. If an explicit path was passed that does NOT exist (and is not default "drives.yaml"),
    ///    return an error.
    pub fn load_from_env_or_path(
        path: Option<&Path>,
    ) -> Result<DriveConfiguration, DeduplicationError> {
        if let Some(p) = path {
            if p.exists() {
                return Self::load_from_path(p);
            }
            if p != Path::new("drives.yaml") {
                return Err(DeduplicationError::ConfigError(format!(
                    "Drive configuration file not found at: {}",
                    p.display()
                )));
            }
        } else if Path::new("drives.yaml").exists() {
            return Self::load_from_path(Path::new("drives.yaml"));
        }

        Self::synthesize_from_env()
    }

    /// Performs validation of drive groups and definitions.
    pub fn validate(config: &DriveConfiguration) -> Result<(), DeduplicationError> {
        if config.groups.is_empty() {
            return Err(DeduplicationError::ConfigError(
                "Drive configuration must define at least one group in 'groups'.".to_string(),
            ));
        }

        let mut seen_ids = HashSet::new();
        for drive in &config.drives {
            if drive.id.trim().is_empty() {
                return Err(DeduplicationError::ConfigError(
                    "Drive definition cannot have an empty 'id'.".to_string(),
                ));
            }
            if !seen_ids.insert(drive.id.clone()) {
                return Err(DeduplicationError::ConfigError(format!(
                    "Duplicate drive id '{}' found in configuration.",
                    drive.id
                )));
            }
            if !config.groups.contains_key(&drive.group) {
                return Err(DeduplicationError::ConfigError(format!(
                    "Drive '{}' references unknown group '{}'. Defined groups are: {:?}",
                    drive.id,
                    drive.group,
                    config.groups.keys().collect::<Vec<_>>()
                )));
            }
        }

        Ok(())
    }
}

/// Matches file paths against configured drive master directory structures.
#[derive(Debug, Clone)]
pub struct CanonicalPathMatcher {
    config: DriveConfiguration,
}

impl CanonicalPathMatcher {
    pub fn new(config: DriveConfiguration) -> Self {
        Self { config }
    }

    /// Access the underlying configuration.
    pub fn config(&self) -> &DriveConfiguration {
        &self.config
    }

    /// Identifies which drive definition owns the given path.
    /// Selects the drive with the longest matching mount_path or master_path prefix.
    pub fn find_drive_for_path<'a>(&'a self, path: &Path) -> Option<&'a DriveDefinition> {
        let mut best_match: Option<(&'a DriveDefinition, usize)> = None;

        for drive in &self.config.drives {
            let m_str = drive.mount_path.to_string_lossy();
            let p_str = path.to_string_lossy();

            if p_str.starts_with(m_str.as_ref()) {
                let len = m_str.len();
                if best_match
                    .as_ref()
                    .is_none_or(|(_, best_len)| len > *best_len)
                {
                    best_match = Some((drive, len));
                }
            }
        }

        best_match.map(|(d, _)| d)
    }

    /// Evaluates whether a file path conforms to `<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>/...`
    pub fn evaluate_path(&self, path: &Path) -> CanonicalPathStatus {
        let drive = match self.find_drive_for_path(path) {
            Some(d) => d,
            None => return CanonicalPathStatus::UnknownDrive,
        };

        let master_root = &drive.master_path;
        let rel_path = match path.strip_prefix(master_root) {
            Ok(rel) => rel,
            Err(_) => return CanonicalPathStatus::OutsideMaster,
        };

        let components: Vec<_> = rel_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        // Must have at least: <Year>/<DateFolder>/<CategoryFolder>/<filename> (>= 4 components)
        if components.len() < 4 {
            return CanonicalPathStatus::OutsideMaster;
        }

        let year_str = &components[0];
        let date_folder = &components[1];
        let category_folder = &components[2];

        // 1. Verify Year (4-digit number)
        let year: i32 = match year_str.parse() {
            Ok(y) if (1900..=2100).contains(&y) => y,
            _ => return CanonicalPathStatus::OutsideMaster,
        };

        // 2. Verify Date Folder matches one of the accepted patterns or English month format
        if !Self::matches_date_pattern(date_folder, year) {
            return CanonicalPathStatus::OutsideMaster;
        }

        // 3. Verify Category Folder is in configured allowed folders
        let cat_allowed = self
            .config
            .rules
            .category_folders
            .iter()
            .any(|c| c.eq_ignore_ascii_case(category_folder));

        if !cat_allowed {
            return CanonicalPathStatus::OutsideMaster;
        }

        CanonicalPathStatus::InMaster {
            year,
            date_folder: date_folder.clone(),
            category_folder: category_folder.clone(),
        }
    }

    /// Verifies if a folder name matches `DD-Mon-YYYY` (e.g. `30-Nov-2021`) or `YYYY-MM-DD`.
    fn matches_date_pattern(folder_name: &str, expected_year: i32) -> bool {
        // Try standard chrono parse for DD-Mon-YYYY (e.g. "30-Nov-2021")
        if let Ok(d) = NaiveDate::parse_from_str(folder_name, "%d-%b-%Y") {
            use chrono::Datelike;
            return d.year() == expected_year;
        }

        // Try YYYY-MM-DD
        if let Ok(d) = NaiveDate::parse_from_str(folder_name, "%Y-%m-%d") {
            use chrono::Datelike;
            return d.year() == expected_year;
        }

        // Custom tolerant check for DD-Mon-YYYY with case-insensitive month
        let parts: Vec<&str> = folder_name.split('-').collect();
        if parts.len() == 3 {
            if let (Ok(day), Ok(year)) = (parts[0].parse::<u32>(), parts[2].parse::<i32>()) {
                if year == expected_year && (1..=31).contains(&day) {
                    let month = parts[1].to_lowercase();
                    let months = [
                        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct",
                        "nov", "dec",
                    ];
                    if months.contains(&month.as_str()) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Determines if a path belongs to the highest priority group ("primary") within the canonical master path.
    pub fn is_primary_master(&self, path: &Path) -> bool {
        let drive = match self.find_drive_for_path(path) {
            Some(d) => d,
            None => return false,
        };

        let group_spec = match self.config.groups.get(&drive.group) {
            Some(g) => g,
            None => return false,
        };

        // Check if this group has the maximum priority among all groups
        let max_priority = self
            .config
            .groups
            .values()
            .map(|g| g.priority)
            .max()
            .unwrap_or(0);

        if group_spec.priority != max_priority {
            return false;
        }

        matches!(
            self.evaluate_path(path),
            CanonicalPathStatus::InMaster { .. }
        )
    }

    /// Returns whether a path is inside a canonical master folder structure.
    pub fn is_in_canonical_master(&self, path: &Path) -> bool {
        matches!(
            self.evaluate_path(path),
            CanonicalPathStatus::InMaster { .. }
        )
    }

    /// Returns the priority of a group name, or 0 if not found.
    pub fn group_priority(&self, group: &str) -> u32 {
        self.config
            .groups
            .get(group)
            .map(|g| g.priority)
            .unwrap_or(0)
    }

    /// Returns the group priority of the drive containing the path, or 0 if unknown.
    pub fn get_path_priority(&self, path: &Path) -> u32 {
        self.find_drive_for_path(path)
            .and_then(|d| self.config.groups.get(&d.group))
            .map(|g| g.priority)
            .unwrap_or(0)
    }
}
