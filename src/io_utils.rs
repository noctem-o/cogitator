use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn write_atomic<F>(path: &Path, label: &str, write_fn: F) -> Result<()>
where
    F: FnOnce(&mut File) -> Result<()>,
{
    let dir = path
        .parent()
        .with_context(|| format!("failed to locate parent dir for {}", label))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("failed to locate file name for {}", label))?
        .to_string_lossy()
        .to_string();

    let mut attempts = 0;
    loop {
        let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(temp_name(&file_name, counter));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(mut file) => {
                // If writing fails, best-effort cleanup of the temp file.
                if let Err(err) = write_fn(&mut file) {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(err);
                }

                // Ensure user-space buffers are flushed before syncing.
                file.flush()
                    .with_context(|| format!("failed to flush {}", label))?;

                file.sync_all()
                    .with_context(|| format!("failed to sync {}", label))?;
                drop(file);

                // Windows rename doesn't overwrite; on Unix it does. Handle both.
                replace_file(&tmp_path, path)
                    .with_context(|| format!("failed to replace {}", label))?;

                // Directory fsync is best-effort and Unix-only.
                sync_dir(dir)?;

                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                attempts += 1;
                if attempts > 10 {
                    return Err(error)
                        .with_context(|| format!("failed to create temp file for {}", label));
                }
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create temp file for {}", label))
            }
        }
    }
}

pub fn write_atomic_bytes(path: &Path, label: &str, bytes: &[u8]) -> Result<()> {
    write_atomic(path, label, |file| {
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", label))?;
        Ok(())
    })
}

pub fn write_atomic_string(path: &Path, label: &str, contents: &str) -> Result<()> {
    write_atomic_bytes(path, label, contents.as_bytes())
}

fn temp_name(base: &str, counter: usize) -> PathBuf {
    PathBuf::from(format!(".{}.tmp.{}", base, counter))
}

/// Replace the destination file with the temporary file.
///
/// - On Unix, `rename` overwrites atomically.
/// - On Windows, `rename` fails if the destination exists, so we remove it first.
///   (Not perfectly atomic, but it's the standard portability compromise.)
fn replace_file(tmp_path: &Path, dst_path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        if dst_path.exists() {
            // Best-effort; if it fails we'll surface the later rename error anyway.
            let _ = fs::remove_file(dst_path);
        }
        fs::rename(tmp_path, dst_path)
    }

    #[cfg(not(windows))]
    {
        fs::rename(tmp_path, dst_path)
    }
}

/// Best-effort directory fsync for crash-safety.
/// On Windows, opening/syncing directories is not generally supported via std APIs.
fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let dir_file = File::open(dir).with_context(|| "failed to open output dir")?;
        dir_file
            .sync_all()
            .with_context(|| "failed to sync output dir")?;
    }
    Ok(())
}
