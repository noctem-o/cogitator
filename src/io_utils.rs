use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn resolve_bundle_relative_path(witness_dir: &Path, raw: &str) -> Result<PathBuf> {
    let root = std::fs::canonicalize(witness_dir).with_context(|| {
        format!(
            "failed to canonicalize witness dir {}",
            witness_dir.display()
        )
    })?;
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        anyhow::bail!("absolute manifest path is forbidden: {}", raw);
    }
    let joined = root.join(&candidate);
    let joined = if joined.exists() {
        joined
    } else {
        root.join(
            candidate
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("invalid empty manifest path: {}", raw))?,
        )
    };
    let canon = std::fs::canonicalize(&joined).with_context(|| {
        format!(
            "failed to canonicalize manifest artifact path {}",
            joined.display()
        )
    })?;
    if !canon.starts_with(&root) {
        anyhow::bail!(
            "manifest artifact escapes witness dir: {} -> {}",
            raw,
            canon.display()
        );
    }
    Ok(canon)
}

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
                if let Err(err) = write_fn(&mut file) {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(err);
                }

                file.flush()
                    .with_context(|| format!("failed to flush {}", label))?;
                file.sync_all()
                    .with_context(|| format!("failed to sync {}", label))?;
                drop(file);

                replace_file(&tmp_path, path)
                    .with_context(|| format!("failed to replace {}", label))?;

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

pub fn write_report_json<T: serde::Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    write_atomic(path, label, |file| {
        serde_json::to_writer(&mut *file, value)
            .with_context(|| format!("failed to serialize {}", label))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write newline for {}", label))?;
        Ok(())
    })
}

fn temp_name(base: &str, counter: usize) -> PathBuf {
    PathBuf::from(format!(".{}.tmp.{}", base, counter))
}

/// Replace destination with temp.
///
/// Unix rename overwrites atomically.
/// Windows rename fails if destination exists, so we remove it first.
fn replace_file(tmp: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        if dst.exists() {
            let _ = fs::remove_file(dst);
        }
        fs::rename(tmp, dst)
    }

    #[cfg(not(windows))]
    {
        fs::rename(tmp, dst)
    }
}

/// Best-effort directory fsync.
/// On Windows, opening/syncing directories like files is not generally supported.
fn sync_dir(_dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let dir_file = File::open(_dir).with_context(|| "failed to open output dir")?;
        dir_file
            .sync_all()
            .with_context(|| "failed to sync output dir")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct ReportRow {
        score: f64,
    }

    #[test]
    fn write_report_json_writes_standard_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report.json");
        let value = ReportRow { score: 0.25 };

        write_report_json(&path, &value, "report.json").expect("write report json");
        let bytes = std::fs::read(&path).expect("read report json");
        assert_eq!(
            std::str::from_utf8(&bytes).expect("utf8"),
            "{\"score\":0.25}\n"
        );
    }
}
