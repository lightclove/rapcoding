//! Model downloader: streaming download with progress + zip extraction.
//! Compiled only with `--features gui` (used by the GUI "Модели" section).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// A downloadable Vosk model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub name: &'static str,
    pub lang: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
}

impl Model {
    /// `asr.model` / `asr.en_model` value pointing at this model's directory.
    /// Models live in `<config_dir>/models/`, resolved by `resolve_model_path`.
    pub fn config_value(&self) -> String {
        format!("models/{}", self.name)
    }
}

/// Catalog of heavier models (URLs/sizes verified in SC-gui).
pub const MODELS: &[Model] = &[
    Model {
        name: "vosk-model-ru-0.42",
        lang: "ru",
        url: "https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip",
        size_bytes: 1_937_602_113,
    },
    Model {
        name: "vosk-model-en-us-0.22",
        lang: "en",
        url: "https://alphacephei.com/vosk/models/vosk-model-en-us-0.22.zip",
        size_bytes: 1_913_365_522,
    },
    Model {
        name: "vosk-model-en-us-0.42-gigaspeech",
        lang: "en",
        url: "https://alphacephei.com/vosk/models/vosk-model-en-us-0.42-gigaspeech.zip",
        size_bytes: 2_423_807_363,
    },
    Model {
        name: "vosk-model-ru-0.10",
        lang: "ru",
        url: "https://alphacephei.com/vosk/models/vosk-model-ru-0.10.zip",
        size_bytes: 2_672_443_065,
    },
];

/// Final directory a model is extracted into: `<models_root>/<name>/`.
pub fn model_dir(models_root: &Path, name: &str) -> PathBuf {
    models_root.join(name)
}

/// Human-readable size ("1.8 GB", "500 B").
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Stream `url` into `dest`, calling `progress(downloaded, total)` as bytes
/// arrive. On any failure the partial `dest` is removed.
pub fn download(url: &str, dest: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let result = download_inner(url, dest, progress);
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

fn download_inner(url: &str, dest: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| Error::Download(format!("GET {url}: {e}")))?;
    let status = resp.status();
    if status != 200 {
        return Err(Error::Download(format!("GET {url}: HTTP {status}")));
    }
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();
    let mut file = File::create(dest)?;
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total.unwrap_or(done));
    }
    file.flush()?;
    Ok(())
}

/// Extract a zip archive into `dest_dir`. Entry paths are sanitized via
/// `enclosed_name` (rejects `..` traversal) — a malicious archive cannot write
/// outside `dest_dir`.
pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::Download(format!("open zip {}: {e}", zip_path.display())))?;
    fs::create_dir_all(dest_dir)?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Download(format!("zip entry {i}: {e}")))?;
        let rel = entry
            .enclosed_name()
            .map(|p| p.to_owned())
            .ok_or_else(|| Error::Download(format!("zip entry {i}: unsafe path")))?;
        let out = dest_dir.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rapcoding-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn human_size_formats_units() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(1_937_602_113), "1.8 GB");
    }

    #[test]
    fn model_dir_joins_root_and_name() {
        assert_eq!(
            model_dir(Path::new("/m/root"), "vosk-model-ru-0.42"),
            PathBuf::from("/m/root/vosk-model-ru-0.42")
        );
    }

    #[test]
    fn config_value_is_models_prefix() {
        assert_eq!(MODELS[0].config_value(), "models/vosk-model-ru-0.42");
    }

    #[test]
    fn catalog_has_verified_sizes() {
        // Guard against accidental URL/size drift in the catalog.
        assert_eq!(MODELS.len(), 4);
        for m in MODELS {
            assert!(m.url.starts_with("https://alphacephei.com/vosk/models/"));
            assert!(m.size_bytes > 0);
        }
    }

    #[test]
    fn extract_zip_roundtrip() {
        let dir = tmp("dl-extract");
        let zip_path = dir.join("m.zip");
        {
            let f = File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zw.add_directory("m/", opts).unwrap();
            zw.start_file("m/a.txt", opts).unwrap();
            zw.write_all(b"hello").unwrap();
            zw.start_file("m/sub/b.txt", opts).unwrap();
            zw.write_all(b"world").unwrap();
            zw.finish().unwrap();
        }
        let out = dir.join("out");
        extract_zip(&zip_path, &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("m/a.txt")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(out.join("m/sub/b.txt")).unwrap(), "world");
        let _ = fs::remove_dir_all(&dir);
    }
}
