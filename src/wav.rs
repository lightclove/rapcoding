use std::path::Path;

use crate::error::{Error, Result};

/// Write mono 16 kHz 16-bit PCM WAV. Used by tests (the live pipeline feeds
/// vosk samples directly, no WAV round-trip).
#[cfg(test)]
pub fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let n = samples.len();
    let data_len = (n * 2) as u32;
    let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);

    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&crate::asr::ASR_RATE.to_le_bytes())?; // sample rate
    w.write_all(&(crate::asr::ASR_RATE * 2).to_le_bytes())?; // byte rate = rate * block_align
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample

    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_all(&v.to_le_bytes())?;
    }
    w.flush()
}

/// Read a PCM WAV file into mono f32 samples at 16 kHz.
/// Handles 16-bit PCM, any channel count (downmixed), any sample rate.
pub fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Audio(format!(
            "{}: not a RIFF/WAVE file",
            path.display()
        )));
    }
    let mut pos = 12usize;
    let mut channels = 0usize;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut format = 0u16;
    let mut data: &[u8] = &[];
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if id == b"data" && pos + 8 + size > bytes.len() {
            return Err(Error::Audio(format!(
                "{}: truncated data chunk",
                path.display()
            )));
        }
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        match id {
            b"fmt " => {
                if body.len() >= 16 {
                    format = u16::from_le_bytes(body[0..2].try_into().unwrap());
                    channels = u16::from_le_bytes(body[2..4].try_into().unwrap()) as usize;
                    rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                    bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
                }
            }
            b"data" => {
                data = body;
                break;
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    if data.is_empty() || rate == 0 || channels == 0 {
        return Err(Error::Audio(format!(
            "{}: no PCM data found",
            path.display()
        )));
    }
    if format != 1 {
        return Err(Error::Audio(format!(
            "{}: unsupported audio format {format} (need PCM)",
            path.display()
        )));
    }
    if bits != 16 {
        return Err(Error::Audio(format!(
            "{}: unsupported bit depth {bits} (need 16)",
            path.display()
        )));
    }
    if channels > 32 {
        return Err(Error::Audio(format!(
            "{}: unsupported channel count {channels}",
            path.display()
        )));
    }
    let mut mono: Vec<f32> = Vec::with_capacity(data.len() / (2 * channels));
    for frame in data.chunks_exact(2 * channels) {
        let mut sum = 0f32;
        for b in frame.chunks_exact(2) {
            sum += i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0;
        }
        mono.push(sum / channels as f32);
    }
    Ok(crate::audio::resample(&mono, rate, crate::asr::ASR_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_valid_header() {
        let p = std::env::temp_dir().join("rapcoding-wav-test.wav");
        let samples = vec![0.0f32; 1600];
        write_wav(&p, &samples).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        let channels = u16::from_le_bytes(bytes[22..24].try_into().unwrap());
        assert_eq!(channels, 1);
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(rate, 16000);
        let bits = u16::from_le_bytes(bytes[34..36].try_into().unwrap());
        assert_eq!(bits, 16);
        assert_eq!(bytes.len(), 44 + samples.len() * 2);
    }

    #[test]
    fn clamps_and_converts() {
        let p = std::env::temp_dir().join("rapcoding-wav-clamp-test.wav");
        write_wav(&p, &[1.5f32, -1.5, 0.0]).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        let a = i16::from_le_bytes(bytes[44..46].try_into().unwrap());
        let b = i16::from_le_bytes(bytes[46..48].try_into().unwrap());
        assert_eq!(a, 32767);
        assert_eq!(b, -32767);
    }

    #[test]
    fn read_back_roundtrip() {
        let p = std::env::temp_dir().join("rapcoding-wav-roundtrip.wav");
        let samples: Vec<f32> = (0..1600).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
        write_wav(&p, &samples).unwrap();
        let back = read_wav(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(back.len(), 1600);
        for (a, b) in samples.iter().zip(back.iter()) {
            assert!((a - b).abs() < 0.01, "{a} vs {b}");
        }
    }

    fn bytes_of(samples: &[f32]) -> Vec<u8> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("rapc-wav-bytes-{}-{n}.wav", std::process::id()));
        write_wav(&p, samples).unwrap();
        let b = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        b
    }

    #[test]
    fn rejects_non_pcm_format() {
        let mut b = bytes_of(&[0.0f32; 100]);
        b[20] = 2; // audio_format = Microsoft ADPCM (not PCM)
        b[21] = 0;
        let p = std::env::temp_dir().join("rapc-nonpcm.wav");
        std::fs::write(&p, &b).unwrap();
        assert!(read_wav(&p).is_err(), "non-PCM must be rejected");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_truncated_data_chunk() {
        let mut b = bytes_of(&[0.0f32; 100]);
        let declared = (b.len() as u32 + 1000).to_le_bytes();
        b[40..44].copy_from_slice(&declared);
        let p = std::env::temp_dir().join("rapc-truncated.wav");
        std::fs::write(&p, &b).unwrap();
        assert!(read_wav(&p).is_err(), "truncated data chunk must be rejected");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rejects_absurd_channels() {
        let mut b = bytes_of(&[0.0f32; 100]);
        b[22] = 200; // channels = 200 (low byte)
        b[23] = 0;
        let p = std::env::temp_dir().join("rapc-channels.wav");
        std::fs::write(&p, &b).unwrap();
        assert!(read_wav(&p).is_err(), "absurd channel count must be rejected");
        let _ = std::fs::remove_file(&p);
    }
}
