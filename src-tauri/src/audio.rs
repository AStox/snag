use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::error::{Result, SnagError};

pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    started: Instant,
    stopped: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    _stream: Option<cpal::Stream>,
}

impl Recorder {
    pub fn start() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            macos::start()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {
                samples: Arc::new(Mutex::new(Vec::new())),
                sample_rate: 16000,
                started: Instant::now(),
                stopped: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    pub fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn recent_rms(&self, window_ms: u32) -> f32 {
        let guard = self.samples.lock().expect("samples");
        if guard.is_empty() {
            return 0.0;
        }
        let n = ((self.sample_rate as u64 * window_ms as u64) / 1000) as usize;
        let start = guard.len().saturating_sub(n.max(1));
        let slice = &guard[start..];
        if slice.is_empty() {
            return 0.0;
        }
        let sum: f32 = slice.iter().map(|s| s * s).sum();
        (sum / slice.len() as f32).sqrt()
    }

    pub fn has_voice(&self) -> bool {
        self.recent_rms(400) > 0.02
    }

    pub fn should_autostop(&self) -> bool {
        if self.elapsed_ms() < 900 {
            return false;
        }
        // Need some energy at some point, then a pause.
        let long = self.recent_rms(1600);
        let pause = self.recent_rms(900);
        self.elapsed_ms() > 700 && long > 0.018 && pause < 0.012 && self.elapsed_ms() > 1400
    }

    pub fn stop_wav(self) -> Result<Vec<u8>> {
        self.stopped.store(true, Ordering::SeqCst);
        let samples = self.samples.lock().expect("samples").clone();
        encode_wav(&samples, self.sample_rate)
    }
}

fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::<u8>::new());
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| SnagError::from(e.to_string()))?;
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            w.write_sample(v).map_err(|e| SnagError::from(e.to_string()))?;
        }
        w.finalize().map_err(|e| SnagError::from(e.to_string()))?;
    }
    Ok(cursor.into_inner())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    pub fn start() -> Result<Recorder> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| SnagError::from("No microphone found"))?;
        let config = device
            .default_input_config()
            .map_err(|e| SnagError::from(e.to_string()))?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let samples_cb = samples.clone();
        let stopped_cb = stopped.clone();

        let err_fn = |e| log::error!("mic stream: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| write_samples(data, channels, &samples_cb, &stopped_cb),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let f: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                    write_samples(&f, channels, &samples_cb, &stopped_cb)
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    let f: Vec<f32> = data
                        .iter()
                        .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    write_samples(&f, channels, &samples_cb, &stopped_cb)
                },
                err_fn,
                None,
            ),
            other => {
                return Err(SnagError::from(format!(
                    "Unsupported mic sample format: {other:?}"
                )))
            }
        }
        .map_err(|e| SnagError::from(e.to_string()))?;
        stream.play().map_err(|e| SnagError::from(e.to_string()))?;
        Ok(Recorder {
            samples,
            sample_rate,
            started: Instant::now(),
            stopped,
            _stream: Some(stream),
        })
    }

    fn write_samples(
        data: &[f32],
        channels: usize,
        dest: &Arc<Mutex<Vec<f32>>>,
        stopped: &Arc<AtomicBool>,
    ) {
        if stopped.load(Ordering::Relaxed) {
            return;
        }
        let mut g = dest.lock().expect("samples");
        if channels <= 1 {
            g.extend_from_slice(data);
            return;
        }
        for frame in data.chunks(channels) {
            let avg = frame.iter().sum::<f32>() / channels as f32;
            g.push(avg);
        }
    }
}
