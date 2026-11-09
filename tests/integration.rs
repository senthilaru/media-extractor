#[path = "integration/traversal_tests.rs"]
mod traversal_tests;

#[path = "integration/backpressure_tests.rs"]
mod backpressure_tests;

#[path = "integration/resilience_tests.rs"]
mod resilience_tests;

#[path = "integration/metadata_pipeline_tests.rs"]
mod metadata_pipeline_tests;

#[path = "integration/dedup_pipeline_tests.rs"]
mod dedup_pipeline_tests;

#[path = "integration/unified_scan_tests.rs"]
mod unified_scan_tests;

#[path = "integration/mongodb_sink_tests.rs"]
mod mongodb_sink_tests;
