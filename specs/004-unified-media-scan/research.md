# Phase 0: Research & Technical Architecture

**Feature**: `004-unified-media-scan`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. CLI Ergonomics: Single-Command vs Subcommands

### Research Question
How should the CLI accept a single mandatory path argument without requiring the user to specify subcommands like `extract` or `scan`, while maintaining backwards compatibility for existing subcommands?

### Options Evaluated
1. **Remove subcommands entirely**:
   - *Pros*: Extremely simple CLI (`metadata-extractor <PATH>`).
   - *Cons*: Breaks existing automation or tests relying on `scan` or `extract`.
2. **Clap Optional Subcommand with Default Positional Argument**:
   - In `clap 4.5`, `Cli` can define `path: Option<PathBuf>` as a positional argument alongside `command: Option<Commands>`.
   - If `path` is present (e.g. `metadata-extractor /Volumes/SP-1` or `metadata-extractor photo.jpg`), the tool directly executes the unified scan & extraction pipeline.
   - If a subcommand is explicitly passed (e.g. `metadata-extractor extract <PATH>` or `metadata-extractor scan <PATH>`), the tool routes to that subcommand.
   - If neither is provided, `clap` prints the help banner with usage instructions.
3. **Dedicated `run` or `extract` as default**:
   - *Pros*: Clear subcommand naming.
   - *Cons*: Still requires typing an extra keyword unless defaulted.

### Decision
**Adopt Option 2 (Top-Level Positional Argument with Optional Subcommands)**:
`metadata-extractor <PATH> [OPTIONS]` is the primary interface. Specifying only the path argument is the default user journey, while `metadata-extractor extract <PATH>` acts as a transparent alias.

---

## 2. Unified Scan & Extraction Workflow

### Research Question
How to execute directory discovery, rich EXIF/video parsing, and perceptual/cryptographic fingerprinting with maximum throughput and strictly bounded memory?

### Pipeline Architecture
```text
┌─────────────────────────────────┐
│ Input Path (Dir or File)        │
└──────────────┬──────────────────┘
               │
       Is Directory?
       ├── YES ──> ScanEngine (jwalk + rayon)
       │             └── Bounded channel (Capacity 100 batches)
       │                   └── MetadataExtractionEngine
       │                         ├── EXIF / Video atom parser
       │                         ├── Temporal calendar faceter
       │                         └── Blake3MediaHasher (pHash + crypto)
       │                               └── NDJSON / JSON Output (stdout or file)
       └── NO  ───> MetadataExtractionEngine (single file)
                     └── Pretty JSON Output (stdout or file)
```

### Performance & Memory Guarantees
- **Asynchronous Walking**: Tokio async channels receive batches from `ScanEngine` with backpressure.
- **Rayon Worker Threads**: Extraction and hashing are computed in parallel across Rayon worker threads without starving Tokio async executor threads.
- **Memory Bounded**: Memory stays $< 100\text{ MB}$ even when scanning $> 100,000$ files because batches are streamed, processed, written, and immediately dropped without holding the full catalog in RAM.
- **Fault-Isolated**: Corrupted or non-standard media files trigger diagnostic fallback documents with warning flags in `extraction_status`; the scan never crashes or terminates prematurely.

---

## 3. Output Sinks (stdout vs `--output <FILE>`)

### Research Question
How should the unified command deliver output when invoked in interactive terminals vs automated scripts?

### Decision
1. **Default (`stdout`)**:
   - When path is a directory: emits streaming NDJSON to `stdout` (1 complete JSON document per line).
   - When path is a single file: emits pretty-printed JSON to `stdout`.
   - Real-time progress and telemetry are written to `stderr` so that stdout redirection (`> output.ndjson`) remains 100% clean JSON.
2. **File Output (`-o, --output <PATH>`)**:
   - If `--output <FILE>` is supplied, output is written directly to the target file with a buffered writer (`BufWriter`), eliminating the need for shell pipe redirection.
