use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::application::MetadataExtractionEngine;
use metadata_extractor::domain::dedup_models::{
    DeduplicationConfig, DeduplicationSummary, DuplicateCluster, FingerprintConfig,
    MediaFingerprint,
};
use metadata_extractor::domain::metadata_models::{
    AppProfile, DatabaseSinkConfig, MediaMetadataDocument,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, DiscoveryBatch, MediaCategory};
use metadata_extractor::domain::ports::{
    BatchMetadataExtractor, DeduplicationEngine, DirectoryScanner, MediaClassifier,
    MediaFingerprinter, MediaMetadataExtractor, MetadataStorageSink, ScanConfig,
};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;
use metadata_extractor::infrastructure::classifier::PhfMediaClassifier;
use metadata_extractor::infrastructure::mongodb_sink::MongoDbStorageSink;
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
    #[arg(long, default_value_t = false)]
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
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
enum DedupMode {
    #[default]
    All,
    Exact,
    Perceptual,
}

#[derive(Parser, Debug)]
struct DedupArgs {
    /// Directory path or media file to scan and deduplicate
    #[arg(required = true)]
    path: PathBuf,

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
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,

    /// Batch size for discovered media items
    #[arg(short = 'b', long, default_value_t = 500)]
    batch_size: usize,

    /// Channel capacity for batch buffer backpressure
    #[arg(short = 'c', long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = false)]
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
    #[arg(long, default_value_t = false)]
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
    #[arg(short = 'c', long, default_value_t = 100)]
    channel_capacity: usize,

    /// Include hidden files and directories (starting with '.')
    #[arg(long, default_value_t = false)]
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
    let start_time = Instant::now();
    let engine = MetadataExtractionEngine::new();
    let quiet = args.quiet;

    // Initialize MongoDB sink if URI is provided (fails fast if connection/auth fails)
    let mongo_sink = if let Some(ref raw_uri) = args.mongodb_uri {
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
        let scan_engine = ScanEngine::new();
        let config = ScanConfig {
            root_paths: vec![args.path.clone()],
            batch_size: args.batch_size,
            channel_capacity: args.channel_capacity,
            skip_hidden: !args.include_hidden,
            follow_symlinks: args.follow_symlinks,
            ..Default::default()
        };

        let (session, mut stream) = scan_engine.start_scan(config).await?;
        let root = args.path.clone();
        let is_pretty = args.pretty.unwrap_or(false);

        // Database write telemetry counters
        let db_upserted = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let db_matched = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let db_modified = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let db_failed = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        let db_upserted_telemetry = db_upserted.clone();
        let db_matched_telemetry = db_matched.clone();
        let db_failed_telemetry = db_failed.clone();
        let has_mongo = mongo_sink.is_some();

        // Live telemetry heartbeat on stderr
        let telemetry_task = tokio::spawn(async move {
            if quiet {
                return;
            }
            let mut stderr = tokio::io::stderr();
            while let Some(metric) = stream.telemetry.recv().await {
                let line = if has_mongo {
                    let total_ingested = db_upserted_telemetry
                        .load(std::sync::atomic::Ordering::Relaxed)
                        + db_matched_telemetry.load(std::sync::atomic::Ordering::Relaxed);
                    format!(
                        "\r[Scan & Ingest] Visited: {} | Found: {} | DB Ingested: {} | Rate: {:.1} fps | DB Errors: {}",
                        metric.directories_scanned,
                        metric.media_files_discovered,
                        total_ingested,
                        metric.items_per_sec,
                        db_failed_telemetry.load(std::sync::atomic::Ordering::Relaxed),
                    )
                } else {
                    format!(
                        "\r[Scan & Extract] Visited: {} | Found: {} | Rate: {:.1} fps | Errors: {}",
                        metric.directories_scanned,
                        metric.media_files_discovered,
                        metric.items_per_sec,
                        metric.errors_encountered,
                    )
                };
                let _ = stderr.write_all(line.as_bytes()).await;
                let _ = stderr.flush().await;
            }
        });

        // Error notifications on stderr
        let error_task = tokio::spawn(async move {
            let mut stderr = tokio::io::stderr();
            while let Some(err) = stream.errors.recv().await {
                let line = format!("[SKIP/ERROR] {:?} on {}\n", err.kind, err.path.display());
                let _ = stderr.write_all(line.as_bytes()).await;
            }
        });

        // Setup file/stdout writer if requested or when not writing solely to MongoDB
        let mut writer_opt: Option<Box<dyn tokio::io::AsyncWrite + Unpin + Send>> =
            match &args.output {
                Some(out_path) => {
                    let file = tokio::fs::File::create(out_path).await?;
                    Some(Box::new(tokio::io::BufWriter::new(file)))
                }
                None => {
                    if mongo_sink.is_none() {
                        Some(Box::new(tokio::io::BufWriter::new(tokio::io::stdout())))
                    } else {
                        None
                    }
                }
            };

        while let Some(batch) = stream.batches.recv().await {
            let docs = engine.extract_batch(batch, Some(&root)).await;

            // Ingest to MongoDB if configured
            if let Some(ref sink) = mongo_sink {
                match sink.save_batch(&docs).await {
                    Ok(stats) => {
                        db_upserted
                            .fetch_add(stats.upserted_count, std::sync::atomic::Ordering::Relaxed);
                        db_matched
                            .fetch_add(stats.matched_count, std::sync::atomic::Ordering::Relaxed);
                        db_modified
                            .fetch_add(stats.modified_count, std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(err) => {
                        db_failed
                            .fetch_add(docs.len() as u64, std::sync::atomic::Ordering::Relaxed);
                        tracing::error!(error = %err, "Failed to persist batch to MongoDB");
                    }
                }
            }

            // Write to file or stdout if enabled
            if let Some(ref mut writer) = writer_opt {
                for doc in &docs {
                    if is_pretty {
                        let pretty_json = serde_json::to_string_pretty(doc)?;
                        writer
                            .write_all(format!("{}\n", pretty_json).as_bytes())
                            .await?;
                    } else {
                        let json = serde_json::to_string(doc)?;
                        writer.write_all(format!("{}\n", json).as_bytes()).await?;
                    }
                }
                writer.flush().await?;
            }
        }

        let _ = telemetry_task.await;
        let _ = error_task.await;

        if !quiet {
            let metrics = engine.metrics(start_time);
            if mongo_sink.is_some() {
                let total_ingested = db_upserted.load(std::sync::atomic::Ordering::Relaxed)
                    + db_matched.load(std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "\rScan & Ingest completed for {}. Total: {}, DB Ingested: {} (Upserted: {}, Modified: {}, Matched: {}), Rate: {:.1} fps. Session: {}",
                    root.display(),
                    metrics.total_processed,
                    total_ingested,
                    db_upserted.load(std::sync::atomic::Ordering::Relaxed),
                    db_modified.load(std::sync::atomic::Ordering::Relaxed),
                    db_matched.load(std::sync::atomic::Ordering::Relaxed),
                    metrics.throughput_fps,
                    session.session_id
                );
            } else {
                eprintln!(
                    "\rScan & Extract completed for {}. Total: {}, Success: {}, Partial: {}, Failed: {}, Rate: {:.1} fps. Session: {}",
                    root.display(),
                    metrics.total_processed,
                    metrics.success_count,
                    metrics.partial_count,
                    metrics.failed_count,
                    metrics.throughput_fps,
                    session.session_id
                );
            }
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
    } else if args.mongodb_uri.is_none() {
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
    let enable_perceptual = args.mode != DedupMode::Exact && !args.no_perceptual;
    let fp_config = FingerprintConfig {
        enable_perceptual,
        enable_rotation: !args.no_rotation,
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: args.similarity_threshold,
        enable_perceptual,
        min_cluster_size: 2,
    };

    let hasher = Blake3MediaHasher::new();
    let engine = MediaDeduplicationEngine::new();

    let mut items = Vec::new();

    if args.path.is_file() {
        let size = std::fs::metadata(&args.path)?.len();
        let classifier = PhfMediaClassifier::new();
        let category = classifier
            .classify(&args.path)
            .unwrap_or(MediaCategory::Photo);
        let ext = args
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        items.push(DiscoveredMediaItem {
            path: args.path.clone(),
            file_size_bytes: size,
            modified_at: None,
            category,
            extension: ext,
        });
    } else {
        let scan_config = ScanConfig {
            root_paths: vec![args.path.clone()],
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
    let fp_config = FingerprintConfig {
        enable_perceptual: !args.no_perceptual,
        enable_rotation: !args.no_rotation,
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: args.similarity_threshold,
        enable_perceptual: !args.no_perceptual,
        min_cluster_size: 2,
    };

    let hasher = Blake3MediaHasher::new();
    let engine = MediaDeduplicationEngine::new();

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

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
