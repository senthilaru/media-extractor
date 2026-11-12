# Contract: Drive Group Configuration Schema (`drives.yaml`)

**Feature**: `009-database-replica-dedup`  
**Contract Type**: YAML Configuration File Schema  
**Status**: Approved  

---

## 1. Specification & Syntax

The configuration file defines drive group hierarchies, mount points, and canonical master paths.

### Sample `drives.yaml`

```yaml
version: "1.0"

# Logical groups ranked by priority (highest priority = master authoritative tier)
groups:
  primary:
    priority: 100
    description: "Primary home media server master storage"
  replica1:
    priority: 80
    description: "First complete mirror backup drive group"
  replica2:
    priority: 60
    description: "Secondary cold off-site backup drive group"

# Physical hard drives and their assigned roles
drives:
  - id: "wd_14tb_primary"
    group: "primary"
    mount_path: "/Volumes/SP-1"
    master_path: "/Volumes/SP-1/SPSA/media/master"

  - id: "seagate_14tb_rep1"
    group: "replica1"
    mount_path: "/Volumes/SP-2"
    master_path: "/Volumes/SP-2/SPSA/media/master"

  - id: "toshiba_8tb_scratch"
    group: "replica1"
    mount_path: "/Volumes/SP-3"
    master_path: "/Volumes/SP-3/SPSA/media/master"

# Master folder structure conventions
rules:
  # Supported media category folder names inside date directories
  category_folders:
    - "Videos"
    - "Photos"
    - "PhotosRaw"
  # Accepted date directory patterns:
  # - DD-Mon-YYYY (e.g. 30-Nov-2021)
  # - YYYY-MM-DD  (e.g. 2021-11-30)
  date_patterns:
    - "%d-%b-%Y"
    - "%Y-%m-%d"
```

---

## 2. Validation Rules

1. **`groups`**:
   - Must contain at least one group designated with the highest priority (e.g. `primary`).
   - Group priorities must be distinct positive integers.
2. **`drives`**:
   - `id` must be non-empty, unique across all drive definitions.
   - `group` must match an existing key defined in `groups`.
   - `mount_path` must be an absolute path.
   - `master_path` must either be equal to or a subpath of `mount_path`.
3. **Format**:
   - System auto-detects YAML (`.yaml`, `.yml`), TOML (`.toml`), or JSON (`.json`) by extension.
