use jwalk::{DirEntry, WalkDir};
use std::path::{Path, PathBuf};

use crate::domain::errors::ScanErrorKind;

/// Wrapper around a raw directory entry or traversal error from jwalk.
#[derive(Debug)]
pub enum WalkItem {
    File {
        path: PathBuf,
        file_size: u64,
        modified_at: Option<std::time::SystemTime>,
    },
    Directory {
        path: PathBuf,
    },
    Error {
        path: PathBuf,
        kind: ScanErrorKind,
        message: String,
    },
}

/// Filesystem walker adapter built on jwalk and rayon work-stealing parallelism.
pub struct ParallelWalker;

impl ParallelWalker {
    /// Creates a parallel walker iterator rooted at `root_path`.
    pub fn walk(
        root_path: &Path,
        skip_hidden: bool,
        follow_symlinks: bool,
    ) -> impl Iterator<Item = WalkItem> {
        WalkDir::new(root_path)
            .skip_hidden(skip_hidden)
            .follow_links(follow_symlinks)
            .into_iter()
            .map(Self::map_entry)
    }

    fn map_entry(entry_result: Result<DirEntry<((), ())>, jwalk::Error>) -> WalkItem {
        match entry_result {
            Ok(entry) => {
                let path = entry.path();
                let file_type = entry.file_type();

                if file_type.is_dir() {
                    return WalkItem::Directory { path };
                }

                if file_type.is_file() {
                    match entry.metadata() {
                        Ok(meta) => WalkItem::File {
                            path,
                            file_size: meta.len(),
                            modified_at: meta.modified().ok(),
                        },
                        Err(err) => {
                            let kind = if err.loop_ancestor().is_some() {
                                ScanErrorKind::CircularSymlink
                            } else if let Some(io) = err.io_error() {
                                match io.kind() {
                                    std::io::ErrorKind::PermissionDenied => {
                                        ScanErrorKind::PermissionDenied
                                    }
                                    std::io::ErrorKind::NotFound => ScanErrorKind::NotFound,
                                    _ => ScanErrorKind::IoError,
                                }
                            } else {
                                ScanErrorKind::IoError
                            };

                            WalkItem::Error {
                                path,
                                kind,
                                message: err.to_string(),
                            }
                        }
                    }
                } else {
                    WalkItem::Directory { path }
                }
            }
            Err(err) => {
                let path = err.path().map(Path::to_path_buf).unwrap_or_default();
                let (kind, msg) = if err.loop_ancestor().is_some() {
                    (
                        ScanErrorKind::CircularSymlink,
                        format!("Circular symlink detected at {}", path.display()),
                    )
                } else if let Some(io) = err.io_error() {
                    let k = match io.kind() {
                        std::io::ErrorKind::PermissionDenied => ScanErrorKind::PermissionDenied,
                        std::io::ErrorKind::NotFound => ScanErrorKind::NotFound,
                        _ => ScanErrorKind::IoError,
                    };
                    (k, io.to_string())
                } else {
                    (ScanErrorKind::IoError, err.to_string())
                };

                WalkItem::Error {
                    path,
                    kind,
                    message: msg,
                }
            }
        }
    }
}
