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
                write_fn(&mut file)?;
                file.sync_all()
                    .with_context(|| format!("failed to sync {}", label))?;
                drop(file);
                fs::rename(&tmp_path, path)
                    .with_context(|| format!("failed to rename {}", label))?;
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

fn sync_dir(dir: &Path) -> Result<()> {
    let dir_file = File::open(dir).with_context(|| "failed to open output dir")?;
    dir_file
        .sync_all()
        .with_context(|| "failed to sync output dir")?;
    Ok(())
}
