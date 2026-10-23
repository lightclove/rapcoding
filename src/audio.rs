use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::error::{Error, Result};

struct Shared {
    recording: AtomicBool,
    tx: Sender<Vec<f32>>, // lock-free: callback pushes chunks, main thread drains
    rx: Receiver<Vec<f32>>,
    start: Mutex<Option<Instant>>,
}

/// A persistent input stream that collects mono samples only while recording.
/// Held on the main thread; the audio callback pushes into `Shared`.
pub struct AudioCapture {
    shared: Arc<Shared>,
    device_rate: u32,
    channels: u16,
    device_name: String,
    _stream: cpal::Stream,
}

impl AudioCapture {
    pub fn new(device_filter: &str) -> Result<Self> {
        let device = resolve_input_device(device_filter)?;
        let device_name = device.to_string();
        let supported = device
            .default_input_config()
            .map_err(|e| Error::Audio(format!("default input config: {e}")))?;
        let sample_format = supported.sample_format();
        let channels = supported.channels();
        let device_rate = supported.sample_rate();
        let config = supported.config();

        let (tx, rx) = unbounded();
        let shared = Arc::new(Shared {
            recording: AtomicBool::new(false),
            tx,
            rx,
            start: Mutex::new(None),
        });

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, config, shared.clone(), channels, downmix_f32)?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, config, shared.clone(), channels, downmix_i16)?
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, config, shared.clone(), channels, downmix_u16)?
            }
            other => {
                return Err(Error::Audio(format!("unsupported sample format {other:?}")));
            }
        };
        stream
            .play()
            .map_err(|e| Error::Audio(format!("start stream: {e}")))?;

        Ok(AudioCapture {
            shared,
            device_rate,
            channels,
            device_name,
            _stream: stream,
        })
    }

    pub fn start(&self) {
        // Drain any stale chunks from a previous (unfinished) recording.
        while self.shared.rx.try_recv().is_ok() {}
        self.shared.recording.store(true, Ordering::Relaxed);
        *crate::sync::lock_recover(&self.shared.start) = Some(Instant::now());
    }

    pub fn stop(&self) -> Vec<f32> {
        self.shared.recording.store(false, Ordering::Relaxed);
        *crate::sync::lock_recover(&self.shared.start) = None;
        let mut out = Vec::new();
        while let Ok(chunk) = self.shared.rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    pub fn elapsed(&self) -> Option<Duration> {
        crate::sync::lock_recover(&self.shared.start)
            .as_ref()
            .map(|i| i.elapsed())
    }

    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }
}

/// A selectable audio endpoint, as shown in the GUI and `rapcoding devices`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    /// Native default config (None when the endpoint refuses to report one).
    pub rate: Option<u32>,
    pub channels: Option<u16>,
}

impl DeviceInfo {
    /// Human summary of the native format, e.g. "48 кГц · стерео".
    #[cfg(feature = "gui")]
    pub fn summary(&self) -> String {
        match (self.rate, self.channels) {
            (Some(r), Some(c)) => {
                let ch = match c {
                    1 => "моно".to_string(),
                    2 => "стерео".to_string(),
                    _ => format!("{c} кан."),
                };
                format!("{} кГц · {ch}", r / 1000)
            }
            _ => String::new(),
        }
    }
}

/// Enumerate input (capture) endpoints; the system default is marked.
pub fn list_input_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().map(|d| d.to_string());
    let mut out = Vec::new();
    for d in host
        .input_devices()
        .map_err(|e| Error::Audio(format!("enumerate input devices: {e}")))?
    {
        let name = d.to_string();
        let (rate, channels) = match d.default_input_config() {
            Ok(c) => (Some(c.sample_rate()), Some(c.channels())),
            Err(_) => (None, None),
        };
        out.push(DeviceInfo {
            is_default: Some(&name) == default_name.as_ref(),
            name,
            rate,
            channels,
        });
    }
    Ok(out)
}

/// Enumerate output (render) endpoints; the system default is marked.
pub fn list_output_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().map(|d| d.to_string());
    let mut out = Vec::new();
    for d in host
        .output_devices()
        .map_err(|e| Error::Audio(format!("enumerate output devices: {e}")))?
    {
        let name = d.to_string();
        let (rate, channels) = match d.default_output_config() {
            Ok(c) => (Some(c.sample_rate()), Some(c.channels())),
            Err(_) => (None, None),
        };
        out.push(DeviceInfo {
            is_default: Some(&name) == default_name.as_ref(),
            name,
            rate,
            channels,
        });
    }
    Ok(out)
}

/// Index of the device chosen for `filter` among `names`. An exact
/// (case-insensitive) match wins over a substring match so that picking
/// "AirPods Pro" cannot accidentally select "AirPods Pro Hands-Free".
/// Empty filter = "use the system default" (caller treats None accordingly).
pub fn pick_index(filter: &str, names: &[String]) -> Option<usize> {
    if filter.is_empty() {
        return None;
    }
    let f = filter.to_lowercase();
    names
        .iter()
        .position(|n| n.to_lowercase() == f)
        .or_else(|| names.iter().position(|n| n.to_lowercase().contains(&f)))
}

/// Resolve the configured `audio.device` filter to a concrete capture device.
/// Empty filter = system default. A filter matching nothing is an error: the
/// daemon must fail loudly instead of recording from the wrong microphone.
pub fn resolve_input_device(filter: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if filter.is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| Error::Audio("no default input device".into()));
    }
    let mut devices: Vec<(cpal::Device, String)> = Vec::new();
    for d in host
        .input_devices()
        .map_err(|e| Error::Audio(format!("enumerate input devices: {e}")))?
    {
        let name = d.to_string();
        devices.push((d, name));
    }
    let names: Vec<String> = devices.iter().map(|(_, n)| n.clone()).collect();
    match pick_index(filter, &names) {
        Some(i) => Ok(devices[i].0.clone()),
        None => Err(Error::Audio(format!(
            "no input device matching '{filter}'"
        ))),
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: Arc<Shared>,
    channels: u16,
    convert: fn(&[T], usize) -> Vec<f32>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Send + 'static,
{
    let s = shared.clone();
    let ch = channels as usize;
    // Rate-limit stream errors: a WASAPI buffer underrun/overrun is non-fatal
    // and can fire in bursts, but one line per ~2 s is enough signal.
    let last_err = Arc::new(AtomicU64::new(0));
    let err = {
        let last_err = last_err.clone();
        move |e: cpal::Error| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let last = last_err.load(Ordering::Relaxed);
            if now.saturating_sub(last) >= 2 {
                last_err.store(now, Ordering::Relaxed);
                // Fallible write: a broken stderr pipe must not panic the audio thread.
                crate::log::write_line(
                    &mut std::io::stderr(),
                    &format!("audio stream error: {}", crate::log::sanitize(&e.to_string())),
                );
            }
        }
    };
    let cb = move |data: &[T], _info: &cpal::InputCallbackInfo| {
        if !s.recording.load(Ordering::Relaxed) {
            return;
        }
        let mono = convert(data, ch);
        let _ = s.tx.send(mono);
    };
    device
        .build_input_stream(config, cb, err, None)
        .map_err(|e| Error::Audio(format!("build input stream: {e}")))
}

fn downmix_f32(data: &[f32], channels: usize) -> Vec<f32> {
    downmix(data, channels, |x| x)
}

fn downmix_i16(data: &[i16], channels: usize) -> Vec<f32> {
    downmix(data, channels, |x| x as f32 / 32768.0)
}

fn downmix_u16(data: &[u16], channels: usize) -> Vec<f32> {
    downmix(data, channels, |x| (x as f32 - 32768.0) / 32768.0)
}

fn downmix<T: Copy>(data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) -> Vec<f32> {
    let ch = channels.max(1);
    let mut out = Vec::with_capacity(data.len() / ch);
    for frame in data.chunks(ch) {
        let s: f32 = frame.iter().copied().map(&to_f32).sum::<f32>() / ch as f32;
        out.push(s);
    }
    out
}

/// Resample mono audio to the target rate. A light moving-average low-pass is
/// applied before decimation to reduce aliasing; otherwise linear interpolation.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || to == 0 {
        return Vec::new();
    }
    if from == to {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let filtered;
    let src: &[f32] = if from > to {
        let w = ratio.round() as usize;
        if w > 1 {
            filtered = moving_average(input, w);
            &filtered
        } else {
            input
        }
    } else {
        input
    };
    let out_len = ((src.len() as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len.max(1));
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let frac = (pos - i0 as f64) as f32;
        let a = src[i0.min(src.len() - 1)];
        let b = src[(i0 + 1).min(src.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

fn moving_average(input: &[f32], window: usize) -> Vec<f32> {
    if window <= 1 {
        return input.to_vec();
    }
    let n = input.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let start = i.saturating_sub(window / 2);
        let end = (i + window / 2).min(n);
        let mut sum = 0f64;
        let mut cnt = 0usize;
        for &x in &input[start..end] {
            sum += x as f64;
            cnt += 1;
        }
        out.push((sum / cnt as f64) as f32);
    }
    out
}

/// First and last index whose absolute value exceeds `threshold`.
pub fn speech_range(input: &[f32], threshold: f32) -> Option<(usize, usize)> {
    let th = threshold.abs();
    let first = input.iter().position(|&x| x.abs() > th)?;
    let last = input.iter().rposition(|&x| x.abs() > th)?;
    Some((first, last))
}

/// Trim leading/trailing silence, keeping `padding_samples` around the speech.
pub fn trim_silence(input: &[f32], threshold: f32, padding_samples: usize) -> Vec<f32> {
    match speech_range(input, threshold) {
        None => Vec::new(),
        Some((a, b)) => {
            let start = a.saturating_sub(padding_samples);
            let end = (b + 1 + padding_samples).min(input.len());
            input[start..end].to_vec()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_exact_beats_substring() {
        let names: Vec<String> = [
            "Головной телефон (AirPods Pro Hands-Free)",
            "Головной телефон (2- AirPods Pro)",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(pick_index("Головной телефон (2- AirPods Pro)", &names), Some(1));
    }

    #[test]
    fn pick_is_case_insensitive() {
        let names: Vec<String> = vec!["Микрофон AirPods Pro".to_string()];
        assert_eq!(pick_index("airpods", &names), Some(0));
    }

    #[test]
    fn pick_empty_filter_means_default() {
        let names: Vec<String> = vec!["X".to_string()];
        assert_eq!(pick_index("", &names), None);
    }

    #[test]
    fn pick_no_match_is_none() {
        let names: Vec<String> = vec!["A".to_string(), "B".to_string()];
        assert_eq!(pick_index("zzz", &names), None);
    }

    #[test]
    fn pick_substring_backward_compat() {
        let names: Vec<String> = vec!["Набор микрофонов (Intel)".to_string()];
        assert_eq!(pick_index("микрофонов", &names), Some(0));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn device_summary_stereo() {
        let d = DeviceInfo {
            name: "x".into(),
            is_default: false,
            rate: Some(48_000),
            channels: Some(2),
        };
        assert_eq!(d.summary(), "48 кГц · стерео");
    }

    #[cfg(feature = "gui")]
    #[test]
    fn device_summary_mono() {
        let d = DeviceInfo {
            name: "x".into(),
            is_default: false,
            rate: Some(16_000),
            channels: Some(1),
        };
        assert_eq!(d.summary(), "16 кГц · моно");
    }

    #[cfg(feature = "gui")]
    #[test]
    fn device_summary_unknown_format_is_empty() {
        let d = DeviceInfo {
            name: "x".into(),
            is_default: false,
            rate: None,
            channels: None,
        };
        assert_eq!(d.summary(), "");
    }

    #[test]
    fn resample_48k_to_16k_length() {
        let input = vec![0.5f32; 48_000];
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn resample_preserves_constant() {
        let input = vec![0.25f32; 24_000];
        let out = resample(&input, 48_000, 16_000);
        for &x in &out {
            assert!((x - 0.25).abs() < 1e-4);
        }
    }

    #[test]
    fn resample_noop_same_rate() {
        let input = vec![0.1, 0.2, 0.3];
        let out = resample(&input, 16_000, 16_000);
        assert_eq!(out, input);
    }

    #[test]
    fn downmix_stereo_averages() {
        let stereo = vec![0.0f32, 1.0, 0.0, 1.0];
        let mono = downmix_f32(&stereo, 2);
        assert_eq!(mono, vec![0.5, 0.5]);
    }

    #[test]
    fn speech_range_finds_edges() {
        let input = vec![0.0, 0.0, 0.5, 0.6, 0.0];
        let (a, b) = speech_range(&input, 0.1).unwrap();
        assert_eq!((a, b), (2, 3));
    }

    #[test]
    fn trim_silence_pads() {
        let input = vec![0.0, 0.0, 0.9, 0.0, 0.0, 0.0];
        let out = trim_silence(&input, 0.1, 1);
        assert_eq!(out, vec![0.0, 0.9, 0.0]);
    }

    #[test]
    fn trim_all_silent_is_empty() {
        let input = vec![0.0f32; 100];
        assert!(trim_silence(&input, 0.1, 10).is_empty());
    }
}
