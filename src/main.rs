use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use metadata_extractor::application::drive_pruner::{DrivePrunerOptions, DrivePruningService};
use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::application::ingest_service::MasterIngestService;
use metadata_extractor::application::pipeline::ExtractionPipeline;
use metadata_extractor::application::MetadataExtractionEngine;
use metadata_extractor::domain::dedup_models::{
    DeduplicationConfig, DeduplicationMetadata, DeduplicationSummary, DuplicateCluster,
    FingerprintConfig, IngestOptions, MediaFingerprint,
};
use metadata_extractor::domain::metadata_models::{
    AppProfile, DatabaseSinkConfig, MediaMetadataDocument,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, DiscoveryBatch, MediaCategory};
use metadata_extractor::domain::ports::{
    BatchMetadataExtractor, DeduplicationEngine, DirectoryScanner, MediaClassifier,
    MediaFingerprinter, MediaMetadataExtractor, MetadataStorageSink, ScanConfig,
};
use metadata_extractor::domain::progress_models::{ExtractionMode, ExtractionPerformanceProfile};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;
use metadata_extractor::infrastructure::classifier::PhfMediaClassifier;
use metadata_extractor::infrastructure::drives_config::{CanonicalPathMatcher, DrivesConfigLoader};
use metadata_extractor::infrastructure::mongodb_sink::MongoDbStorageSink;
use metadata_extractor::infrastructure::terminal_progress::IndicatifTerminalProgress;
use metadata_extractor::MediaDeduplicationEngine;

#[derive(Parser, Debug)]
#[command(
    name = "metadata-extractor",
    version,
    about = "High-throughput asynchronous media metadata extractor and scanner"
)]
struct Cli {
    /// Target media directory or file to recursively scan and extract rich metadata
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Output file path to write extracted metadata records (defaults to stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Format output with pretty indentation (defaults to false for directories, true for single files)
    #[arg(short = 'p', long)]
    pretty: Option<bool>,

    /// Batch size for discovered media items when scanning directories
    #[arg(short = 'b', long, default_value_t = 500)]
    batch_size: usize,

    /// Channel capacity for batch buffer backpressure
    #[arg(short = 'c', long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = true)]
    include_hidden: bool,

    /// Follow directory symlinks with cycle detection
    #[arg(long, default_value_t = false)]
    follow_symlinks: bool,

    /// Suppress live scan telemetry heartbeats on stderr
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,

    /// Operational profile ('local' for development defaults, 'production' for strict security checks)
    #[arg(long, value_enum, default_value_t = AppProfile::Local, env = "APP_PROFILE")]
    profile: AppProfile,

    /// MongoDB connection URI to persist extracted metadata records
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name within the MongoDB database (defaults to 'media_assets')
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,

    /// Enable high-speed metadata-only extraction; bypass full-file bitwise and perceptual hashing
    #[arg(short = 'f', long, default_value_t = false, env = "MEDIA_FAST_MODE")]
    fast: bool,

    /// Explicit extraction performance mode ('fast', 'balanced', 'full')
    #[arg(long, value_enum, env = "MEDIA_EXTRACTION_MODE")]
    mode: Option<ExtractionMode>,

    /// Skip full-file bitwise hashing for files exceeding this size threshold in MB
    #[arg(long, default_value_t = 100, env = "MEDIA_MAX_HASH_SIZE_MB")]
    max_hash_size: u64,

    /// Number of concurrent batch extraction workers
    #[arg(long, env = "MEDIA_WORKERS")]
    workers: Option<usize>,

    /// Emit progress telemetry as discrete JSON lines instead of ANSI progress bar
    #[arg(long, default_value_t = false, env = "MEDIA_JSON_TELEMETRY")]
    json_telemetry: bool,

    /// Force complete re-extraction of all files, bypassing database incremental change pre-filtering
    #[arg(
        long,
        visible_alias = "reindex",
        default_value_t = false,
        env = "MEDIA_FORCE_REINDEX"
    )]
    force: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan directories recursively for photos and videos
    Scan(ScanArgs),

    /// Extract rich metadata from a single photo or video file
    Extract(ExtractArgs),

    /// Stream scanned media batches from stdin and emit MongoDB-ready NDJSON to stdout
    ExtractStream(ExtractStreamArgs),

    /// Deduplicate media assets in directories or files
    Dedup(DedupArgs),

    /// Stream media metadata from stdin and emit duplicate clusters NDJSON to stdout
    DedupStream(DedupStreamArgs),

    /// Ingest unmastered media assets from a secondary drive into the primary master structure
    Ingest(IngestArgs),

    /// Synchronize canonical master assets from primary master to configured replica drives
    #[command(alias = "replicate", alias = "replica-sync")]
    SyncReplicas(SyncReplicasArgs),
}

#[derive(Parser, Debug)]
struct SyncReplicasArgs {
    /// Target specific replica drive ID (defaults to all mounted replica drives if omitted)
    #[arg(short = 'r', long)]
    replica: Option<String>,

    /// Target primary master drive ID (defaults to configured primary drive)
    #[arg(short = 'p', long)]
    primary: Option<String>,

    /// Path to drive configuration YAML file
    #[arg(
        short = 'c',
        long,
        default_value = "drives.yaml",
        env = "MEDIA_DRIVES_CONFIG"
    )]
    config: PathBuf,

    /// Preview planned replica transfers without copying or modifying DB
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Skip BLAKE3 cryptographic hash verification of copied destination files
    #[arg(long, default_value_t = false)]
    no_verify_hash: bool,

    /// MongoDB connection URI override
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name override
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Suppress non-essential progress telemetry
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,
}

#[derive(Parser, Debug)]
struct IngestArgs {
    /// Source drive ID, external drive mount path, or local directory path holding unmastered files
    #[arg(short = 's', long, required = true)]
    source_drive: String,

    /// Target primary master drive ID (defaults to configured primary drive)
    #[arg(short = 't', long)]
    target_drive: Option<String>,

    /// Path to drive configuration YAML file
    #[arg(
        short = 'c',
        long,
        default_value = "drives.yaml",
        env = "MEDIA_DRIVES_CONFIG"
    )]
    config: PathBuf,

    /// Preview planned file transfers without copying or modifying DB
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Automatically fan out newly ingested canonical masters to all mounted replica drives
    #[arg(short = 'r', long, default_value_t = false)]
    replicate: bool,

    /// Physically delete source files after verified copy (defaults to deferred MarkForRemoval)
    #[arg(long, default_value_t = false)]
    delete_source: bool,

    /// Skip BLAKE3 cryptographic hash verification of copied destination files
    #[arg(long, default_value_t = false)]
    no_verify_hash: bool,

    /// MongoDB connection URI override
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name override
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Suppress non-essential progress telemetry
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
enum DedupMode {
    #[default]
    All,
    Exact,
    Perceptual,
}

#[derive(Subcommand, Debug)]
enum DedupSubcommands {
    /// Clean / prune marked duplicates on a specific mounted drive
    Prune(PruneArgs),
}

#[derive(Parser, Debug)]
struct PruneArgs {
    /// Drive ID or mount path to prune (e.g. wd_14tb_primary or /Volumes/SP-1)
    #[arg(short = 'd', long, required = true)]
    drive: String,

    /// Path to drive configuration YAML/TOML file
    #[arg(
        short = 'c',
        long,
        default_value = "drives.yaml",
        env = "MEDIA_DRIVES_CONFIG"
    )]
    config: PathBuf,

    /// Actually delete files on disk (defaults to safe dry-run preview if omitted)
    #[arg(long, default_value_t = false)]
    execute: bool,

    /// Require canonical master drive to be mounted and verified on disk before deletion
    #[arg(long, default_value_t = false)]
    require_master_disk: bool,

    /// Save removal audit manifest to JSON file
    #[arg(short = 'o', long)]
    manifest_out: Option<PathBuf>,

    /// MongoDB connection URI override
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name override
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,
}

#[derive(Parser, Debug)]
struct DedupArgs {
    /// Subcommand (e.g. prune)
    #[command(subcommand)]
    subcommand: Option<DedupSubcommands>,

    /// Directory path or media file to scan and deduplicate (optional if using --config)
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Path to drive configuration YAML/TOML file for database-driven deduplication
    #[arg(short = 'c', long, env = "MEDIA_DRIVES_CONFIG")]
    config: Option<PathBuf>,

    /// Evaluate and preview classifications without updating MongoDB
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Perform universal classification across all assets in the catalog (both unique and duplicates)
    #[arg(long, default_value_t = false)]
    all: bool,

    /// MongoDB connection URI override
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name override
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,

    /// Deduplication mode ('exact' for bitwise only, 'perceptual' for visual only, 'all' for 4-tier pipeline)
    #[arg(long, value_enum, default_value_t = DedupMode::All)]
    mode: DedupMode,

    /// Maximum Hamming distance to consider near-duplicate (0..=64)
    #[arg(long, default_value_t = 10)]
    similarity_threshold: u32,

    /// Disable Tier 4 perceptual visual deduplication (exact bitwise only)
    #[arg(long, default_value_t = false)]
    no_perceptual: bool,

    /// Disable rotation invariance (0° canonical orientation only)
    #[arg(long, default_value_t = false)]
    no_rotation: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Batch size for discovered media items
    #[arg(short = 'b', long, default_value_t = 500)]
    batch_size: usize,

    /// Channel capacity for batch buffer backpressure
    #[arg(long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = true)]
    include_hidden: bool,

    /// Follow directory symlinks with cycle detection
    #[arg(long, default_value_t = false)]
    follow_symlinks: bool,

    /// Suppress live telemetry heartbeats on stderr
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,
}

#[derive(Parser, Debug)]
struct DedupStreamArgs {
    /// Path to drive configuration YAML/TOML file for replica-aware deduplication
    #[arg(short = 'c', long, env = "MEDIA_DRIVES_CONFIG")]
    config: Option<PathBuf>,

    /// Maximum Hamming distance to consider near-duplicate (0..=64)
    #[arg(long, default_value_t = 10)]
    similarity_threshold: u32,

    /// Disable Tier 4 perceptual visual deduplication (exact bitwise only)
    #[arg(long, default_value_t = false)]
    no_perceptual: bool,

    /// Disable rotation invariance (0° canonical orientation only)
    #[arg(long, default_value_t = false)]
    no_rotation: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

#[derive(Parser, Debug)]
struct ExtractArgs {
    /// Path to a media file or directory to scan and extract
    #[arg(required = true)]
    path: PathBuf,

    /// Output file path to write extracted metadata records (defaults to stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Format output with pretty indentation (defaults to false for directories, true for single files)
    #[arg(short = 'p', long)]
    pretty: Option<bool>,

    /// Batch size for discovered media items when scanning directories
    #[arg(short = 'b', long, default_value_t = 500)]
    batch_size: usize,

    /// Channel capacity for batch buffer backpressure
    #[arg(short = 'c', long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = true)]
    include_hidden: bool,

    /// Follow directory symlinks with cycle detection
    #[arg(long, default_value_t = false)]
    follow_symlinks: bool,

    /// Suppress live scan telemetry heartbeats on stderr
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,

    /// Operational profile ('local' for development defaults, 'production' for strict security checks)
    #[arg(long, value_enum, default_value_t = AppProfile::Local, env = "APP_PROFILE")]
    profile: AppProfile,

    /// MongoDB connection URI to persist extracted metadata records
    #[arg(long, visible_alias = "db-uri", env = "MONGODB_URI")]
    mongodb_uri: Option<String>,

    /// Target collection name within the MongoDB database (defaults to 'media_assets')
    #[arg(long, visible_alias = "db-collection", env = "MONGODB_COLLECTION")]
    collection: Option<String>,

    /// Enable high-speed metadata-only extraction; bypass full-file bitwise and perceptual hashing
    #[arg(short = 'f', long, default_value_t = false, env = "MEDIA_FAST_MODE")]
    fast: bool,

    /// Explicit extraction performance mode ('fast', 'balanced', 'full')
    #[arg(long, value_enum, env = "MEDIA_EXTRACTION_MODE")]
    mode: Option<ExtractionMode>,

    /// Skip full-file bitwise hashing for files exceeding this size threshold in MB
    #[arg(long, default_value_t = 100, env = "MEDIA_MAX_HASH_SIZE_MB")]
    max_hash_size: u64,

    /// Number of concurrent batch extraction workers
    #[arg(long, env = "MEDIA_WORKERS")]
    workers: Option<usize>,

    /// Emit progress telemetry as discrete JSON lines instead of ANSI progress bar
    #[arg(long, default_value_t = false, env = "MEDIA_JSON_TELEMETRY")]
    json_telemetry: bool,

    /// Force complete re-extraction of all files, bypassing database incremental change pre-filtering
    #[arg(
        long,
        visible_alias = "reindex",
        default_value_t = false,
        env = "MEDIA_FORCE_REINDEX"
    )]
    force: bool,
}

#[derive(Parser, Debug)]
struct ExtractStreamArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

#[derive(Parser, Debug)]
struct ScanArgs {
    /// One or more directory paths to scan
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Batch size for discovered media items
    #[arg(short = 'b', long, default_value_t = 500)]
    batch_size: usize,

    /// Channel capacity for batch buffer backpressure
    #[arg(long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = true)]
    include_hidden: bool,

    /// Follow directory symlinks with cycle detection
    #[arg(long, default_value_t = false)]
    follow_symlinks: bool,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Suppress live telemetry heartbeats on stderr
    #[arg(short = 'q', long, default_value_t = false)]
    quiet: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment files: .env.{profile} then .env
    if let Ok(profile_str) =
        std::env::var("APP_PROFILE").or_else(|_| std::env::var("MEDIA_PROFILE"))
    {
        let _ = dotenvy::from_filename(format!(".env.{}", profile_str.to_lowercase()));
    }
    let _ = dotenvy::dotenv();

    // Check MEDIA_PROFILE fallback if APP_PROFILE was not set
    if std::env::var("APP_PROFILE").is_err() {
        if let Ok(media_prof) = std::env::var("MEDIA_PROFILE") {
            std::env::set_var("APP_PROFILE", media_prof);
        }
    }

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Scan(args) => run_scan(args).await?,
            Commands::Extract(args) => run_extract(args).await?,
            Commands::ExtractStream(args) => run_extract_stream(args).await?,
            Commands::Dedup(args) => run_dedup(args).await?,
            Commands::DedupStream(args) => run_dedup_stream(args).await?,
            Commands::Ingest(args) => run_ingest(args).await?,
            Commands::SyncReplicas(args) => run_sync_replicas(args).await?,
        }
    } else if let Some(path) = cli.path {
        let args = ExtractArgs {
            path,
            output: cli.output,
            pretty: cli.pretty,
            batch_size: cli.batch_size,
            channel_capacity: cli.channel_capacity,
            include_hidden: cli.include_hidden,
            follow_symlinks: cli.follow_symlinks,
            quiet: cli.quiet,
            profile: cli.profile,
            mongodb_uri: cli.mongodb_uri,
            collection: cli.collection,
            fast: cli.fast,
            mode: cli.mode,
            max_hash_size: cli.max_hash_size,
            workers: cli.workers,
            json_telemetry: cli.json_telemetry,
            force: cli.force,
        };
        run_extract(args).await?;
    } else {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
    }

    Ok(())
}

async fn run_scan(args: ScanArgs) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();
    let engine = ScanEngine::new();

    let config = ScanConfig {
        root_paths: args.paths,
        batch_size: args.batch_size,
        channel_capacity: args.channel_capacity,
        skip_hidden: !args.include_hidden,
        follow_symlinks: args.follow_symlinks,
        ..Default::default()
    };

    let (session, mut stream) = engine.start_scan(config).await?;

    let is_json = args.format == OutputFormat::Json;
    let quiet = args.quiet;

    // Handle telemetry on stderr
    let telemetry_task = tokio::spawn(async move {
        if quiet {
            return;
        }
        let mut stderr = tokio::io::stderr();
        while let Some(metric) = stream.telemetry.recv().await {
            if is_json {
                let json = serde_json::to_string(&serde_json::json!({
                    "type": "telemetry",
                    "session_id": metric.session_id,
                    "elapsed_seconds": metric.elapsed_duration.as_secs_f64(),
                    "directories_scanned": metric.directories_scanned,
                    "files_inspected": metric.files_inspected,
                    "media_files_discovered": metric.media_files_discovered,
                    "throughput_fps": metric.items_per_sec,
                    "queue_depth": metric.queue_depth,
                    "errors_encountered": metric.errors_encountered,
                }))
                .unwrap_or_default();
                let _ = stderr.write_all(format!("{}\n", json).as_bytes()).await;
            } else {
                let line = format!(
                    "\r[Telemetry] Visited: {} | Found: {} | Rate: {:.1} fps | Errors: {}",
                    metric.directories_scanned,
                    metric.media_files_discovered,
                    metric.items_per_sec,
                    metric.errors_encountered,
                );
                let _ = stderr.write_all(line.as_bytes()).await;
                let _ = stderr.flush().await;
            }
        }
    });

    // Handle errors on stderr
    let error_task = tokio::spawn(async move {
        let mut stderr = tokio::io::stderr();
        while let Some(err) = stream.errors.recv().await {
            if is_json {
                let json = serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "path": err.path,
                    "kind": err.kind,
                    "message": err.message,
                    "occurred_at": err.occurred_at,
                }))
                .unwrap_or_default();
                let _ = stderr.write_all(format!("{}\n", json).as_bytes()).await;
            } else {
                let line = format!("[SKIP/ERROR] {:?} on {}\n", err.kind, err.path.display());
                let _ = stderr.write_all(line.as_bytes()).await;
            }
        }
    });

    // Handle batch streaming on stdout
    let mut stdout = tokio::io::stdout();
    let mut total_discovered = 0usize;
    let mut total_batches = 0usize;

    while let Some(batch) = stream.batches.recv().await {
        total_batches += 1;
        total_discovered += batch.count;

        if is_json {
            let json = serde_json::to_string(&serde_json::json!({
                "type": "batch",
                "session_id": batch.session_id,
                "batch_sequence": batch.batch_sequence,
                "count": batch.count,
                "created_at": batch.created_at,
                "items": batch.items,
            }))?;
            stdout.write_all(format!("{}\n", json).as_bytes()).await?;
        } else {
            for item in &batch.items {
                let line = format!(
                    "[{}] {} ({} bytes)\n",
                    item.category,
                    item.path.display(),
                    item.file_size_bytes
                );
                stdout.write_all(line.as_bytes()).await?;
            }
        }
    }

    let _ = telemetry_task.await;
    let _ = error_task.await;

    if !is_json && !quiet {
        eprintln!(
            "\nScan completed in {:.2}s. Total batches: {}, Total media files: {}. Session: {}",
            start_time.elapsed().as_secs_f64(),
            total_batches,
            total_discovered,
            session.session_id
        );
    }

    Ok(())
}

async fn run_extract(args: ExtractArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mode = if args.fast {
        ExtractionMode::Fast
    } else {
        args.mode.unwrap_or(ExtractionMode::Balanced)
    };

    let max_hash_size_bytes = if mode == ExtractionMode::Fast {
        None
    } else {
        Some(args.max_hash_size * 1024 * 1024)
    };

    let profile = ExtractionPerformanceProfile {
        mode,
        max_hash_size_bytes,
        enable_perceptual: mode != ExtractionMode::Fast,
        enable_cryptographic_hash: mode != ExtractionMode::Fast,
        worker_concurrency: args.workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(8)
        }),
        batch_size: args.batch_size,
        channel_capacity: args.channel_capacity,
        update_interval_ms: 250,
        force_reindex: args.force,
    };

    let engine = MetadataExtractionEngine::with_performance_profile(profile.clone());
    let quiet = args.quiet;

    // Initialize MongoDB sink if URI is provided (fails fast if connection/auth fails)
    let mongo_sink =
        if let Some(raw_uri) = args.mongodb_uri.as_deref().filter(|s| !s.trim().is_empty()) {
            let env_password = std::env::var("MONGODB_PASSWORD").ok();
            let interpolated_uri =
                DatabaseSinkConfig::interpolate_secret(raw_uri, env_password.as_deref());
            let config = DatabaseSinkConfig::new(interpolated_uri, args.collection.clone());

            // Fail fast if profile validation fails (exit code 2 per CLI contract)
            if let Err(err) = config.validate_for_profile(args.profile) {
                eprintln!("Configuration error ({} profile): {}", args.profile, err);
                std::process::exit(2);
            }

            if !quiet {
                eprintln!("Connecting to MongoDB at {}...", config.sanitized_uri());
            }

            let sink = match MongoDbStorageSink::connect(config).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Database error: {}", e);
                    std::process::exit(3);
                }
            };

            if let Err(e) = sink.ensure_indexes().await {
                eprintln!("Database index initialization error: {}", e);
                std::process::exit(3);
            }

            Some(sink)
        } else {
            None
        };

    // If path is a directory, utilize the media directory walker and scan all files recursively
    if args.path.is_dir() {
        let scan_engine = std::sync::Arc::new(ScanEngine::new());
        let metadata_engine = std::sync::Arc::new(engine);

        let progress_sink: std::sync::Arc<dyn metadata_extractor::domain::ports::ProgressSink> =
            std::sync::Arc::new(IndicatifTerminalProgress::new(
                args.json_telemetry,
                args.quiet,
            ));

        let has_mongo = mongo_sink.is_some();
        let storage_sink_opt: Option<std::sync::Arc<dyn MetadataStorageSink>> =
            mongo_sink.map(|s| std::sync::Arc::new(s) as std::sync::Arc<dyn MetadataStorageSink>);

        let pipeline = ExtractionPipeline::new(
            scan_engine,
            metadata_engine,
            profile,
            storage_sink_opt,
            progress_sink,
        );

        // Setup file/stdout writer (defaults to stdout if output file not specified)
        let writer_opt: Option<
            std::sync::Arc<tokio::sync::Mutex<Box<dyn tokio::io::AsyncWrite + Unpin + Send>>>,
        > = match &args.output {
            Some(out_path) => {
                let file = tokio::fs::File::create(out_path).await?;
                Some(std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(
                    tokio::io::BufWriter::new(file),
                ))))
            }
            None => Some(std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(
                tokio::io::BufWriter::new(tokio::io::stdout()),
            )))),
        };

        // Cancellation signal handling (Ctrl+C / SIGINT)
        let cancel_token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_for_signal = cancel_token.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });

        let is_pretty = args.pretty.unwrap_or(false);
        let summary = pipeline
            .execute(args.path.clone(), writer_opt, cancel_token, is_pretty)
            .await?;

        if has_mongo && !quiet {
            eprintln!(
                "Scan & Ingest completed for {}. Total: {}, DB Ingested: {} (Upserted: {}, Modified: {}, Matched: {}), Rate: {:.1} fps. Session: {}",
                args.path.display(),
                summary.total_processed,
                summary.db_upserted + summary.db_matched,
                summary.db_upserted,
                summary.db_modified,
                summary.db_matched,
                summary.average_items_per_sec,
                summary.session_id
            );
        }

        if summary.was_cancelled {
            std::process::exit(130);
        }

        return Ok(());
    }

    // Single file extraction
    let classifier = PhfMediaClassifier::new();
    let category = classifier
        .classify(&args.path)
        .unwrap_or(MediaCategory::Photo);
    let size = std::fs::metadata(&args.path).map(|m| m.len()).unwrap_or(0);
    let extension = args
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let item = DiscoveredMediaItem {
        path: args.path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category,
        extension,
    };

    let doc = engine.extract_file(&item, args.path.parent())?;

    // Ingest single document to MongoDB if configured
    if let Some(ref sink) = mongo_sink {
        let stats = sink.save_batch(std::slice::from_ref(&doc)).await?;
        if !quiet {
            eprintln!(
                "Persisted document to MongoDB: {} (Upserted: {}, Modified: {})",
                args.path.display(),
                stats.upserted_count,
                stats.modified_count
            );
        }
    }

    let is_pretty = args.pretty.unwrap_or(args.output.is_none());
    let output_str = if is_pretty {
        serde_json::to_string_pretty(&doc)?
    } else {
        serde_json::to_string(&doc)?
    };

    if let Some(ref out_path) = args.output {
        let mut file = tokio::fs::File::create(out_path).await?;
        file.write_all(format!("{}\n", output_str).as_bytes())
            .await?;
        file.flush().await?;
    } else {
        println!("{}", output_str);
    }
    Ok(())
}

async fn run_extract_stream(_args: ExtractStreamArgs) -> Result<(), Box<dyn std::error::Error>> {
    let engine = MetadataExtractionEngine::new();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    while stdin.read_line(&mut line).await? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        // Handle scanned batch JSON object
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if val.get("type").and_then(|t| t.as_str()) == Some("batch") {
                if let Some(items_val) = val.get("items") {
                    if let Ok(items) =
                        serde_json::from_value::<Vec<DiscoveredMediaItem>>(items_val.clone())
                    {
                        let disc_batch = DiscoveryBatch {
                            session_id: uuid::Uuid::new_v4(),
                            batch_sequence: 0,
                            items,
                            count: 0,
                            created_at: std::time::SystemTime::now(),
                        };
                        let docs = engine.extract_batch(disc_batch, None).await;
                        for doc in docs {
                            let json = serde_json::to_string(&doc)?;
                            stdout.write_all(format!("{}\n", json).as_bytes()).await?;
                        }
                    }
                }
            } else if let Ok(item) = serde_json::from_value::<DiscoveredMediaItem>(val) {
                let doc = engine.extract_file(&item, None)?;
                let json = serde_json::to_string(&doc)?;
                stdout.write_all(format!("{}\n", json).as_bytes()).await?;
            }
        }
        stdout.flush().await?;
        line.clear();
    }
    Ok(())
}

async fn run_dedup(args: DedupArgs) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(sub) = args.subcommand {
        match sub {
            DedupSubcommands::Prune(prune_args) => return run_dedup_prune(prune_args).await,
        }
    }

    if let Some(ref path) = args.path {
        if args.config.is_none() {
            return run_local_dedup_cli(path.clone(), args).await;
        }
    }

    run_database_dedup_cli(args.config.clone(), args).await
}

fn resolve_db_config(
    cli_uri: Option<String>,
    cli_collection: Option<String>,
) -> DatabaseSinkConfig {
    let raw_uri = cli_uri
        .or_else(|| std::env::var("MONGODB_URI").ok())
        .unwrap_or_else(|| "mongodb://localhost:27017/media_index_db".to_string());
    let env_password = std::env::var("MONGODB_PASSWORD").ok();
    let uri = DatabaseSinkConfig::interpolate_secret(&raw_uri, env_password.as_deref());

    let collection = cli_collection
        .or_else(|| std::env::var("MONGODB_COLLECTION").ok())
        .unwrap_or_else(|| "media_assets".to_string());

    DatabaseSinkConfig::new(uri, Some(collection))
}

async fn run_database_dedup_cli(
    cfg_path: Option<PathBuf>,
    args: DedupArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let drive_config = DrivesConfigLoader::load_from_env_or_path(cfg_path.as_deref())?;

    let db_config = resolve_db_config(args.mongodb_uri, args.collection);
    if !args.quiet {
        eprintln!("Connecting to MongoDB at {}...", db_config.sanitized_uri());
    }

    let sink = match MongoDbStorageSink::connect(db_config.clone()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Database error: {}", e);
            std::process::exit(3);
        }
    };

    let _ = sink.ensure_indexes().await;

    if !args.quiet {
        eprintln!(
            "Running database-driven deduplication across configured drives (dry_run: {})...",
            args.dry_run
        );
        if let Ok(unhashed) = sink.count_unfingerprinted_records().await {
            if unhashed > 0 {
                eprintln!(
                    "Notice: Found {} record(s) in MongoDB without cryptographic fingerprints (e.g. extracted in fast mode '-f').",
                    unhashed
                );
                eprintln!(
                    "        Deduplication requires cryptographic fingerprints. Re-run extraction without '-f' (using '--force') to compute them.\n"
                );
            }
        }
    }

    let engine = MediaDeduplicationEngine::new();
    let summary = if args.all {
        if !args.quiet {
            eprintln!(
                "Running universal classification for both unique assets and duplicate clusters..."
            );
        }
        engine
            .run_universal_classification(&sink, &drive_config, args.dry_run)
            .await?
    } else {
        engine
            .run_database_dedup(&sink, &drive_config, args.dry_run)
            .await?
    };

    if args.format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "================================================================================"
        );
        println!("MEDIA DEDUPLICATION & REPLICA AUDIT SUMMARY");
        println!(
            "================================================================================"
        );
        println!(
            "Configuration:             {} ({} groups, {} drives)",
            cfg_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ".env (Environment Synthesized)".to_string()),
            drive_config.groups.len(),
            drive_config.drives.len()
        );
        println!(
            "Database:                  {} ({})",
            db_config.sanitized_uri(),
            db_config.collection_name
        );
        println!("Mode:                      Database-Driven Exact Bitwise");
        println!("Total Records Evaluated:   {}", summary.total_files_scanned);
        println!(
            "Duplicate Clusters Found:  {}",
            summary.duplicate_clusters_count
        );
        println!(
            "Total Redundant Files:     {}",
            summary.exact_duplicate_files
        );
        println!(
            "Reclaimable Storage:       {} bytes ({:.2} GB)",
            summary.total_reclaimable_bytes,
            summary.total_reclaimable_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!("Elapsed Time:              {:.3}s", summary.elapsed_seconds);
        println!(
            "--------------------------------------------------------------------------------"
        );
        if args.dry_run {
            println!("Database Update:           [DRY-RUN] No records were modified in MongoDB.");
        } else {
            println!(
                "Database Update:           Successfully marked duplicate metadata in MongoDB."
            );
        }
        println!(
            "================================================================================"
        );
    }

    Ok(())
}

async fn run_dedup_prune(args: PruneArgs) -> Result<(), Box<dyn std::error::Error>> {
    let drive_config = DrivesConfigLoader::load_from_env_or_path(Some(&args.config))?;

    let db_config = resolve_db_config(args.mongodb_uri, args.collection);
    let sink = match MongoDbStorageSink::connect(db_config.clone()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Database error: {}", e);
            std::process::exit(3);
        }
    };

    let service = DrivePruningService::new();
    let options = DrivePrunerOptions {
        drive_identifier: args.drive,
        execute: args.execute,
        require_master_disk: args.require_master_disk,
        manifest_out: args.manifest_out,
    };

    let report = service.prune_drive(&sink, &drive_config, options).await?;

    if report.is_dry_run {
        println!(
            "[DRY-RUN] Pruning preview for drive: {} ({})",
            report.drive_id,
            report.mount_path.display()
        );
        println!("Candidates Marked for Removal: {}", report.total_candidates);
        println!("Verified Canonical Masters:    {}", report.verified_masters);
        println!(
            "Unverified Masters (Skipped):  {}",
            report.unverified_masters
        );
        println!(
            "Reclaimable Space:             {} bytes ({:.2} GB)",
            report.bytes_reclaimed,
            report.bytes_reclaimed as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        println!();
        println!("Zero files were deleted. Re-run with `--execute` to delete redundant copies.");
    } else {
        println!(
            "[EXECUTED] Pruned redundant duplicates on drive: {} ({})",
            report.drive_id,
            report.mount_path.display()
        );
        println!("Files Deleted:                 {}", report.files_deleted);
        println!(
            "Storage Space Reclaimed:       {} bytes ({:.2} GB)",
            report.bytes_reclaimed,
            report.bytes_reclaimed as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        if !report.errors.is_empty() {
            println!("Errors / Warnings encountered: {}", report.errors.len());
            for err in &report.errors {
                println!("  - {}", err);
            }
        }
    }

    Ok(())
}

async fn run_local_dedup_cli(
    target_path: PathBuf,
    args: DedupArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let enable_perceptual = args.mode != DedupMode::Exact && !args.no_perceptual;
    let fp_config = FingerprintConfig {
        enable_perceptual,
        enable_rotation: !args.no_rotation,
        ..Default::default()
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: args.similarity_threshold,
        enable_perceptual,
        min_cluster_size: 2,
    };

    let hasher = Blake3MediaHasher::new();
    let engine = MediaDeduplicationEngine::new();

    let mut items = Vec::new();

    if target_path.is_file() {
        let size = std::fs::metadata(&target_path)?.len();
        let classifier = PhfMediaClassifier::new();
        let category = classifier
            .classify(&target_path)
            .unwrap_or(MediaCategory::Photo);
        let ext = target_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        items.push(DiscoveredMediaItem {
            path: target_path,
            file_size_bytes: size,
            modified_at: None,
            category,
            extension: ext,
        });
    } else {
        let scan_config = ScanConfig {
            root_paths: vec![target_path],
            batch_size: args.batch_size,
            channel_capacity: args.channel_capacity,
            skip_hidden: !args.include_hidden,
            follow_symlinks: args.follow_symlinks,
            telemetry_interval: if args.quiet {
                std::time::Duration::from_secs(3600)
            } else {
                std::time::Duration::from_millis(500)
            },
        };

        let scanner = ScanEngine::new();
        let (_session, mut stream) = scanner.start_scan(scan_config).await?;

        while let Some(batch) = stream.batches.recv().await {
            items.extend(batch.items);
        }
    }

    if !args.quiet && args.format == OutputFormat::Text {
        eprintln!("Computing fingerprints for {} media assets...", items.len());
    }

    let fingerprints: Vec<MediaFingerprint> = tokio::task::spawn_blocking(move || {
        items
            .par_iter()
            .filter_map(|item| hasher.fingerprint_file(item, None, &fp_config).ok())
            .collect()
    })
    .await?;

    let (clusters, summary) = engine.find_duplicates(fingerprints, dedup_config).await?;

    match args.format {
        OutputFormat::Json => {
            #[derive(serde::Serialize)]
            struct DedupResult {
                clusters: Vec<DuplicateCluster>,
                summary: DeduplicationSummary,
            }
            let res = DedupResult { clusters, summary };
            println!("{}", serde_json::to_string_pretty(&res)?);
        }
        OutputFormat::Text => {
            println!(
                "================================================================================"
            );
            println!("MEDIA DEDUPLICATION REPORT");
            println!(
                "================================================================================"
            );
            println!("Total Files Scanned:       {}", summary.total_files_scanned);
            println!("Unique Media Assets:       {}", summary.unique_media_count);
            println!(
                "Duplicate Clusters:        {}",
                summary.duplicate_clusters_count
            );
            println!(
                "Exact Bitwise Duplicates:  {}",
                summary.exact_duplicate_files
            );
            println!(
                "Perceptual Near-Duplicates:{}",
                summary.near_duplicate_files
            );
            println!(
                "Total Reclaimable Space:   {} bytes ({:.2} MB)",
                summary.total_reclaimable_bytes,
                summary.total_reclaimable_bytes as f64 / (1024.0 * 1024.0)
            );
            println!(
                "Tiers Pruned:              {:.1}%",
                summary.tier_pruned_percentage
            );
            println!("Elapsed Time:              {:.3}s", summary.elapsed_seconds);
            println!(
                "================================================================================"
            );

            for (idx, cluster) in clusters.iter().enumerate() {
                println!();
                println!(
                    "Cluster #{} [{:?}] ({} files, {} bytes reclaimable):",
                    idx + 1,
                    cluster.relationship_type,
                    cluster.total_files,
                    cluster.total_reclaimable_bytes
                );
                println!(
                    "  [Master]  {} ({} bytes, score: {:.1})",
                    cluster.canonical_asset.file_path.display(),
                    cluster.canonical_asset.file_size_bytes,
                    cluster.canonical_asset.quality_score
                );
                for variant in &cluster.variants {
                    println!(
                        "  [Variant] {} ({} bytes, rel: {:?}, sim: {:.3}, dist: {:?})",
                        variant.file_path.display(),
                        variant.file_size_bytes,
                        variant.relationship,
                        variant.similarity_score,
                        variant.hamming_distance
                    );
                }
            }
        }
    }

    Ok(())
}

async fn run_dedup_stream(args: DedupStreamArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    // If --config is supplied, perform replica-aware deduplication on streaming MediaMetadataDocument items
    if let Some(ref cfg_path) = args.config {
        let drive_config = DrivesConfigLoader::load_from_env_or_path(Some(cfg_path))?;
        let matcher = CanonicalPathMatcher::new(drive_config);

        let mut doc_groups: HashMap<String, Vec<MediaMetadataDocument>> = HashMap::new();

        while stdin.read_line(&mut line).await? > 0 {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                line.clear();
                continue;
            }

            if let Ok(doc) = serde_json::from_str::<MediaMetadataDocument>(trimmed) {
                let hash = doc
                    .fingerprint
                    .as_ref()
                    .map(|f| f.cryptographic_hash.clone())
                    .unwrap_or_default();
                doc_groups.entry(hash).or_default().push(doc);
            }
            line.clear();
        }

        for (_hash, docs) in doc_groups {
            if docs.len() > 1 {
                let evaluated = MediaDeduplicationEngine::evaluate_replica_cluster(&docs, &matcher);
                let meta_map: HashMap<uuid::Uuid, DeduplicationMetadata> =
                    evaluated.into_iter().collect();
                for mut doc in docs {
                    if let Some(meta) = meta_map.get(&doc.id) {
                        doc.dedup = Some(meta.clone());
                    }
                    let json = serde_json::to_string(&doc)?;
                    stdout.write_all(format!("{}\n", json).as_bytes()).await?;
                }
            } else {
                for doc in docs {
                    let json = serde_json::to_string(&doc)?;
                    stdout.write_all(format!("{}\n", json).as_bytes()).await?;
                }
            }
        }
        stdout.flush().await?;
        return Ok(());
    }

    let fp_config = FingerprintConfig {
        enable_perceptual: !args.no_perceptual,
        enable_rotation: !args.no_rotation,
        ..Default::default()
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: args.similarity_threshold,
        enable_perceptual: !args.no_perceptual,
        min_cluster_size: 2,
    };

    let hasher = Blake3MediaHasher::new();
    let engine = MediaDeduplicationEngine::new();

    let mut items: Vec<(DiscoveredMediaItem, Option<MediaMetadataDocument>)> = Vec::new();
    let mut direct_fps = Vec::new();

    while stdin.read_line(&mut line).await? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        // Could be a MediaFingerprint, MediaMetadataDocument, or DiscoveredMediaItem
        if let Ok(fp) = serde_json::from_str::<MediaFingerprint>(trimmed) {
            direct_fps.push(fp);
        } else if let Ok(doc) = serde_json::from_str::<MediaMetadataDocument>(trimmed) {
            let item = DiscoveredMediaItem {
                path: doc.file_context.file_path.clone(),
                file_size_bytes: doc.file_context.file_size_bytes,
                modified_at: None,
                category: doc.file_context.media_category,
                extension: doc
                    .file_context
                    .file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase(),
            };
            items.push((item, Some(doc)));
        } else if let Ok(item) = serde_json::from_str::<DiscoveredMediaItem>(trimmed) {
            items.push((item, None));
        }

        line.clear();
    }

    let mut fingerprints = direct_fps;
    if !items.is_empty() {
        let computed_fps: Vec<MediaFingerprint> = tokio::task::spawn_blocking(move || {
            items
                .par_iter()
                .filter_map(|(item, doc)| {
                    hasher.fingerprint_file(item, doc.as_ref(), &fp_config).ok()
                })
                .collect()
        })
        .await?;
        fingerprints.extend(computed_fps);
    }

    let (clusters, summary) = engine.find_duplicates(fingerprints, dedup_config).await?;

    for cluster in clusters {
        let json = serde_json::to_string(&cluster)?;
        stdout.write_all(format!("{}\n", json).as_bytes()).await?;
    }
    stdout.flush().await?;

    eprintln!(
        "Deduplication complete: {} clusters found across {} files ({:.2} MB reclaimable)",
        summary.duplicate_clusters_count,
        summary.total_files_scanned,
        summary.total_reclaimable_bytes as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

async fn run_ingest(args: IngestArgs) -> Result<(), Box<dyn std::error::Error>> {
    let drive_config = DrivesConfigLoader::load_from_env_or_path(Some(&args.config))?;

    let db_config = resolve_db_config(args.mongodb_uri, args.collection);
    if !args.quiet {
        eprintln!("Connecting to MongoDB at {}...", db_config.sanitized_uri());
    }

    let sink = match MongoDbStorageSink::connect(db_config.clone()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Database error: {}", e);
            std::process::exit(3);
        }
    };

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: args.source_drive,
        target_drive: args.target_drive,
        dry_run: args.dry_run,
        delete_source: args.delete_source,
        verify_hash: !args.no_verify_hash,
    };

    if args.dry_run {
        let plan = service.compile_plan(&sink, &drive_config, &options).await?;
        if args.format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!(
                "================================================================================"
            );
            println!("MASTER INGESTION PLAN (DRY-RUN)");
            println!(
                "================================================================================"
            );
            println!("Configuration:         {}", args.config.display());
            println!(
                "Source Drive:          {} ({})",
                plan.source_drive_id,
                plan.source_mount.display()
            );
            println!(
                "Target Primary Master: {} ({})",
                plan.target_drive_id,
                plan.target_master_root.display()
            );
            println!(
                "Unmastered Files:      {} files ({:.2} MB)",
                plan.tasks.len(),
                plan.total_bytes as f64 / (1024.0 * 1024.0)
            );
            println!(
                "--------------------------------------------------------------------------------"
            );
            println!("Planned Transfer Operations:");
            for task in &plan.tasks {
                println!("  [{:?}]", task.category);
                println!(
                    "    From: {} ({:.1} MB)",
                    task.source_path.display(),
                    task.file_size_bytes as f64 / (1024.0 * 1024.0)
                );
                println!("    To:   {}", task.target_path.display());
            }
            println!(
                "--------------------------------------------------------------------------------"
            );
            println!("Zero bytes transferred. Re-run without `--dry-run` to execute ingestion.");
            println!(
                "================================================================================"
            );
        }
        return Ok(());
    }

    let report = service
        .execute_ingest(&sink, &drive_config, options)
        .await?;

    if args.format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "================================================================================"
        );
        if args.delete_source {
            println!("MASTER INGESTION & PRUNE SUMMARY");
        } else {
            println!("MASTER INGESTION SUMMARY");
        }
        println!(
            "================================================================================"
        );
        println!("Source Drive:          {}", report.source_drive_id);
        println!("Target Primary Master: {}", report.target_drive_id);
        println!("Status:                SUCCESS (100% Verified)");
        println!("Files Transferred:     {}", report.files_transferred);
        println!(
            "Files Skipped:         {} (Identical already in master)",
            report.files_skipped_identical
        );
        println!("Files Failed:          {}", report.files_failed);
        println!(
            "Volume Ingested:       {} bytes ({:.2} MB)",
            report.bytes_transferred,
            report.bytes_transferred as f64 / (1024.0 * 1024.0)
        );
        println!("Elapsed Time:          {:.3}s", report.elapsed_seconds);
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!("Database Updates:");
        println!(
            "  - Canonical Masters Created: {} (Registered on {})",
            report.files_transferred, report.target_drive_id
        );
        println!(
            "  - Source Files Marked:       {} (Updated to 'MarkForRemoval' on {})",
            report.files_transferred + report.files_skipped_identical,
            report.source_drive_id
        );
        if args.delete_source {
            println!(
                "Source Files on Disk:  Deleted {} source files from disk.",
                report.source_files_deleted
            );
        } else {
            println!(
                "Source Files on Disk:  Preserved on source drive. Run `dedup prune --drive {}` to delete.",
                report.source_drive_id
            );
        }
        if !report.errors.is_empty() {
            println!("Errors / Warnings encountered: {}", report.errors.len());
            for err in &report.errors {
                println!("  - {}", err);
            }
        }
        println!(
            "================================================================================"
        );
    }

    if args.replicate && !args.dry_run && report.files_transferred > 0 {
        if !args.quiet {
            eprintln!(
                "\nMaster ingestion complete. Fanning out to all mounted replica backup drives..."
            );
        }

        let rep_service =
            metadata_extractor::application::replica_service::ReplicaSyncService::new();
        let rep_options = metadata_extractor::application::replica_service::ReplicaSyncOptions {
            target_replica_id: None,
            primary_drive_id: Some(report.target_drive_id.clone()),
            dry_run: false,
            verify_hash: !args.no_verify_hash,
        };

        let rep_report = rep_service
            .execute_sync(&sink, &drive_config, rep_options)
            .await?;

        if args.format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&rep_report)?);
        } else {
            println!(
                "================================================================================"
            );
            println!("AUTOMATED REPLICA FANOUT SUMMARY");
            println!(
                "================================================================================"
            );
            println!("Primary Master Drive:  {}", rep_report.primary_drive_id);
            println!(
                "Replicas Synchronized: {}",
                if rep_report.synchronized_replicas.is_empty() {
                    "None".to_string()
                } else {
                    rep_report.synchronized_replicas.join(", ")
                }
            );
            if !rep_report.offline_replicas.is_empty() {
                println!(
                    "Offline Replicas:      {} (Skipped - drive unmounted)",
                    rep_report.offline_replicas.join(", ")
                );
            }
            println!("Files Replicated:      {}", rep_report.files_replicated);
            println!(
                "Volume Replicated:     {} bytes ({:.2} MB)",
                rep_report.bytes_transferred,
                rep_report.bytes_transferred as f64 / (1024.0 * 1024.0)
            );
            println!("Elapsed Time:          {:.3}s", rep_report.elapsed_seconds);
            println!(
                "================================================================================"
            );
        }
    }

    Ok(())
}

async fn run_sync_replicas(args: SyncReplicasArgs) -> Result<(), Box<dyn std::error::Error>> {
    let drive_config = DrivesConfigLoader::load_from_env_or_path(Some(&args.config))?;

    let db_config = resolve_db_config(args.mongodb_uri, args.collection);
    if !args.quiet {
        eprintln!("Connecting to MongoDB at {}...", db_config.sanitized_uri());
    }

    let sink = match MongoDbStorageSink::connect(db_config.clone()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Database error: {}", e);
            std::process::exit(3);
        }
    };

    let service = metadata_extractor::application::replica_service::ReplicaSyncService::new();
    let options = metadata_extractor::application::replica_service::ReplicaSyncOptions {
        target_replica_id: args.replica,
        primary_drive_id: args.primary,
        dry_run: args.dry_run,
        verify_hash: !args.no_verify_hash,
    };

    if args.dry_run {
        let plan = service.compile_plan(&sink, &drive_config, &options).await?;
        if args.format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!(
                "================================================================================"
            );
            println!("REPLICA SYNCHRONIZATION PLAN (DRY-RUN)");
            println!(
                "================================================================================"
            );
            println!("Configuration:         {}", args.config.display());
            println!("Primary Drive:         {}", plan.primary_drive_id);
            println!(
                "Active Replicas:       {}",
                if plan.active_replica_ids.is_empty() {
                    "None".to_string()
                } else {
                    plan.active_replica_ids.join(", ")
                }
            );
            if !plan.offline_replica_ids.is_empty() {
                println!(
                    "Offline Replicas:      {} (Skipped - drive unmounted)",
                    plan.offline_replica_ids.join(", ")
                );
            }
            println!(
                "Replication Tasks:     {} files ({:.2} MB)",
                plan.tasks.len(),
                plan.total_bytes as f64 / (1024.0 * 1024.0)
            );
            println!(
                "--------------------------------------------------------------------------------"
            );
            for task in &plan.tasks {
                println!(
                    "  [{}] -> [{}]",
                    task.source_path.display(),
                    task.replica_drive_id
                );
                println!("    To: {}", task.target_path.display());
            }
            println!(
                "--------------------------------------------------------------------------------"
            );
            println!("Zero bytes transferred. Re-run without `--dry-run` to execute replica sync.");
            println!(
                "================================================================================"
            );
        }
        return Ok(());
    }

    let report = service.execute_sync(&sink, &drive_config, options).await?;

    if args.format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "================================================================================"
        );
        println!("REPLICA SYNCHRONIZATION SUMMARY");
        println!(
            "================================================================================"
        );
        println!("Primary Master Drive:  {}", report.primary_drive_id);
        println!(
            "Replicas Synchronized: {}",
            if report.synchronized_replicas.is_empty() {
                "None".to_string()
            } else {
                report.synchronized_replicas.join(", ")
            }
        );
        if !report.offline_replicas.is_empty() {
            println!(
                "Offline Replicas:      {} (Skipped - drive unmounted)",
                report.offline_replicas.join(", ")
            );
        }
        println!("Status:                SUCCESS (100% Verified)");
        println!("Files Replicated:      {}", report.files_replicated);
        println!(
            "Files Skipped:         {} (Identical already on replica)",
            report.files_skipped_identical
        );
        println!("Files Failed:          {}", report.files_failed);
        println!(
            "Volume Transferred:    {} bytes ({:.2} MB)",
            report.bytes_transferred,
            report.bytes_transferred as f64 / (1024.0 * 1024.0)
        );
        println!("Elapsed Time:          {:.3}s", report.elapsed_seconds);
        if !report.errors.is_empty() {
            println!("Errors / Warnings encountered: {}", report.errors.len());
            for err in &report.errors {
                println!("  - {}", err);
            }
        }
        println!(
            "================================================================================"
        );
    }

    Ok(())
}
