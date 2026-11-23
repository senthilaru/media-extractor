# CLI Contract: Visual Semantic Recognition, Face Search & Smart Albums

This contract defines the command-line interfaces exposed by `metadata-extractor` for visual enrichment, face management, semantic search, and smart album generation.

---

## 1. Subcommand: `enrich`

Enriches cataloged media assets with visual semantic features (objects, faces, OCR text, occasions).

```bash
metadata-extractor enrich [OPTIONS]
```

### Options

| Flag / Option | Short | Type | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `--source <PATH>` | `-s` | `PathBuf` | `None` | Restrict enrichment to files matching path prefix (or all un-enriched if omitted) |
| `--config <PATH>` | `-c` | `PathBuf` | `drives.yaml` | Path to drive configuration |
| `--enable-objects` | | `bool` | `true` | Extract object and scene labels |
| `--enable-faces` | | `bool` | `true` | Detect faces, expressions, and compute embeddings |
| `--enable-ocr` | | `bool` | `true` | Extract visible text snippets |
| `--min-confidence <FLOAT>` | | `f32` | `0.60` | Minimum confidence threshold for recording detected features |
| `--batch-size <NUM>` | `-b` | `usize` | `50` | Inference batch size for hardware acceleration |
| `--force` | | `bool` | `false` | Force re-analysis of already enriched media assets |
| `--dry-run` | | `bool` | `false` | Run inference and preview results without updating database |

### Example Invocation
```bash
metadata-extractor enrich --source /Volumes/SP-1/SPSA/media/master/2024 --enable-objects --enable-faces
```

---

## 2. Subcommand: `faces`

Manages detected person clusters, lists unnamed clusters, and assigns names to recurring identities.

### Commands

#### `faces list`
Lists recognized person clusters across the library.
```bash
metadata-extractor faces list [--unnamed-only] [--min-photos <N>]
```

#### `faces show` (aliases: `photos`, `files`)
Inspects the top matching photos for a specific cluster ID to visually identify the person before naming.
```bash
# Display top 10 matching files in table format with face confidence & expression
metadata-extractor faces show --cluster-id <UUID> [--limit <N>] [--format table|paths|json]

# Stream paths directly into image viewers on macOS (e.g. Preview)
metadata-extractor faces show --cluster-id <UUID> --format paths | xargs open
```

#### `faces name`
Assigns a person's name to a specific cluster ID.
```bash
metadata-extractor faces name --cluster-id <UUID> --name "Maya"
```

#### `faces merge`
Merges two duplicate person clusters into a single unified identity.
```bash
metadata-extractor faces merge --source <UUID> --target <UUID>
```

---

## 3. Subcommand: `search`

Performs high-performance multi-modal search across visual features, people, expressions, objects, text, and calendar facets.

```bash
metadata-extractor search [QUERY] [OPTIONS]
```

### Options

| Option | Type | Example | Description |
| :--- | :--- | :--- | :--- |
| `[QUERY]` | `String` | `"bicycle"` | Free-text keyword search across tags, objects, and OCR words |
| `--person <NAME>` | `String` | `--person "Maya"` | Filter by recognized person name |
| `--cluster-id <UUID>` | `Uuid` | `--cluster-id <UUID>` | Filter by recognized person cluster UUID |
| `--expression <EMOTION>`| `String` | `--expression "smile"` | Filter by facial expression (`smile`, `happy`, `crying`) |
| `--object <LABEL>` | `String` | `--object "teddy_bear"` | Filter by detected physical object |
| `--occasion <TAG>` | `String` | `--occasion "birthday"` | Filter by scene or occasion context |
| `--text <WORD>` | `String` | `--text "Anniversary"` | Filter by in-image OCR text |
| `--year <YYYY>` | `u32` | `--year 2024` | Filter by calendar year |
| `--format <FMT>` | `String` | `table` (or `json`, `paths`) | Output formatting |

### Example Invocation
```bash
metadata-extractor search "beach" --person "Maya" --expression "smile" --object "bicycle" --year 2024
```

---

## 4. Subcommand: `albums`

Manages and generates virtual smart albums without duplicating physical files.

### Commands

#### `albums generate`
Analyzes catalog metadata, clusters events, and synthesizes virtual smart albums.
```bash
metadata-extractor albums generate [--year <YYYY>] [--min-photos <N>] [--dry-run]
```

#### `albums list`
Lists all synthesized and custom virtual albums with photo counts and date spans.
```bash
metadata-extractor albums list
```

#### `albums export`
Exports album manifests as JSON, M3U playlist, or directory of symbolic references.
```bash
metadata-extractor albums export --album-id <UUID> --output <DIR>
```
