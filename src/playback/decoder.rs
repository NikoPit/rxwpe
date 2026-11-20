use std::{
    collections::VecDeque,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;

const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct VideoInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) frames_per_second: f64,
    pub(crate) duration: Option<Duration>,
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Playback time, including loops, rather than the timestamp in the file.
    pub(crate) timestamp: Duration,
    pub(crate) rgba: Arc<[u8]>,
}

/// A looping decoder with at most two completed frames waiting in memory.
///
/// `frame_at` advances the stream to a playback time. Large jumps and backwards
/// seeks restart FFmpeg at that position, so captures do not decode the entire
/// preceding video. Dropping the decoder terminates and reaps its process.
pub(crate) struct VideoDecoder {
    path: PathBuf,
    info: VideoInfo,
    stream: Option<DecodeStream>,
    current: Option<(u64, DecodedFrame)>,
}

impl VideoDecoder {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path
            .as_ref()
            .canonicalize()
            .context("video file does not exist")?;
        ensure!(
            path.is_file(),
            "video path is not a regular file: {}",
            path.display()
        );
        let info = probe(&path)?;
        Ok(Self {
            path,
            info,
            stream: None,
            current: None,
        })
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Only the playback controls query video metadata.")
    )]
    pub(crate) fn info(&self) -> &VideoInfo {
        &self.info
    }

    pub(crate) fn frame_at(&mut self, elapsed: Duration) -> Result<DecodedFrame> {
        let frame_number = (elapsed.as_secs_f64() * self.info.frames_per_second).floor();
        ensure!(
            frame_number < u64::MAX as f64,
            "video playback time is too large"
        );
        let frame_number = frame_number as u64;
        if let Some((number, frame)) = &self.current
            && *number == frame_number
        {
            return Ok(frame.clone());
        }
        let restart = match &self.current {
            None => true,
            Some((number, _)) => {
                frame_number < *number
                    || frame_number - *number > (self.info.frames_per_second * 2.0) as u64
            }
        };
        if restart || self.stream.is_none() {
            // Kill the previous process before starting another one.
            self.stream = None;
            self.stream = Some(DecodeStream::start(&self.path, &self.info, frame_number)?);
        }
        let stream = self.stream.as_mut().expect("stream was created");
        loop {
            let frame = stream.next()?;
            let number = stream.next_frame - 1;
            self.current = Some((number, frame.clone()));
            if number >= frame_number {
                return Ok(frame);
            }
        }
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

fn probe(path: &Path) -> Result<VideoInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,r_frame_rate,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .context("cannot run ffprobe; install FFmpeg or enter nix develop")?;
    ensure!(
        output.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let probe: Probe =
        serde_json::from_slice(&output.stdout).context("invalid ffprobe response")?;
    let stream = probe
        .streams
        .first()
        .context("file contains no video stream")?;
    let width = stream.width.context("video width is missing")?;
    let height = stream.height.context("video height is missing")?;
    frame_bytes(width, height)?;
    let frames_per_second = stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_rate)
        .or_else(|| stream.r_frame_rate.as_deref().and_then(parse_rate))
        .context("video has no usable frame rate")?;
    ensure!(
        frames_per_second <= 1000.0,
        "video frame rate is too large: {frames_per_second}"
    );
    let duration = stream
        .duration
        .as_deref()
        .and_then(parse_duration)
        .or_else(|| {
            probe
                .format
                .and_then(|f| f.duration)
                .as_deref()
                .and_then(parse_duration)
        });
    Ok(VideoInfo {
        width,
        height,
        frames_per_second,
        duration,
    })
}

fn parse_rate(text: &str) -> Option<f64> {
    let rate = match text.split_once('/') {
        Some((n, d)) => n.parse::<f64>().ok()? / d.parse::<f64>().ok()?,
        None => text.parse().ok()?,
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

fn parse_duration(text: &str) -> Option<Duration> {
    let seconds: f64 = text.parse().ok()?;
    if seconds > 0.0 {
        Duration::try_from_secs_f64(seconds).ok()
    } else {
        None
    }
}

fn frame_bytes(width: u32, height: u32) -> Result<usize> {
    ensure!(width > 0 && height > 0, "video has zero dimensions");
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .context("video dimensions overflow")?;
    ensure!(
        bytes <= MAX_FRAME_BYTES,
        "video frame exceeds the 256 MiB limit"
    );
    Ok(bytes)
}

struct DecodeStream {
    child: Child,
    receiver: Option<Receiver<std::result::Result<Arc<[u8]>, String>>>,
    reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    next_frame: u64,
    info: VideoInfo,
}

impl DecodeStream {
    fn start(path: &Path, info: &VideoInfo, first_frame: u64) -> Result<Self> {
        let seconds = first_frame as f64 / info.frames_per_second;
        let seek = info
            .duration
            .map_or(seconds, |duration| seconds % duration.as_secs_f64());
        let mut child = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-threads",
                "2",
                "-stream_loop",
                "-1",
                "-ss",
                &format!("{seek:.9}"),
                "-noautorotate",
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-vf",
                &format!("fps={:.12}", info.frames_per_second),
                "-pix_fmt",
                "rgba",
                "-threads",
                "2",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("cannot run ffmpeg; install FFmpeg or enter nix develop")?;
        let mut stdout = child.stdout.take().context("FFmpeg stdout is missing")?;
        let mut stderr_pipe = child.stderr.take().context("FFmpeg stderr is missing")?;
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let log = stderr.clone();
        let stderr_reader = thread::spawn(move || {
            let mut buffer = [0; 1024];
            while let Ok(count) = stderr_pipe.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
                log.extend(&buffer[..count]);
                while log.len() > STDERR_LIMIT {
                    log.pop_front();
                }
            }
        });
        let bytes = frame_bytes(info.width, info.height)?;
        let (sender, receiver) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || {
            loop {
                let mut frame = vec![0; bytes];
                match stdout.read_exact(&mut frame) {
                    Ok(()) => {
                        if sender.send(Ok(Arc::from(frame))).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let message = if error.kind() == ErrorKind::UnexpectedEof {
                            "FFmpeg stopped before producing a complete frame".to_owned()
                        } else {
                            format!("reading FFmpeg output failed: {error}")
                        };
                        let _ = sender.send(Err(message));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            receiver: Some(receiver),
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
            stderr,
            next_frame: first_frame,
            info: info.clone(),
        })
    }

    fn next(&mut self) -> Result<DecodedFrame> {
        let received = self
            .receiver
            .as_ref()
            .expect("receiver exists while decoding")
            .recv_timeout(Duration::from_secs(15));
        let rgba = match received {
            Ok(Ok(frame)) => frame,
            Ok(Err(error)) => bail!("{error}: {}", self.error_log()),
            Err(RecvTimeoutError::Timeout) => bail!(
                "FFmpeg did not produce a frame within 15 seconds: {}",
                self.error_log()
            ),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("FFmpeg frame reader stopped: {}", self.error_log()));
            }
        };
        let timestamp =
            Duration::from_secs_f64(self.next_frame as f64 / self.info.frames_per_second);
        self.next_frame += 1;
        Ok(DecodedFrame {
            width: self.info.width,
            height: self.info.height,
            timestamp,
            rgba,
        })
    }

    fn error_log(&self) -> String {
        let log = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&log.iter().copied().collect::<Vec<_>>())
            .trim()
            .to_owned()
    }
}

impl Drop for DecodeStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.receiver.take();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn decodes_loop_and_seeks_a_real_video() -> Result<()> {
        if Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("FFmpeg unavailable; skipping media integration test");
            return Ok(());
        }
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("rxwpe-video-{}-{suffix}.mkv", std::process::id()));
        let status = Command::new("ffmpeg").args(["-nostdin", "-v", "error", "-f", "lavfi", "-i",
            "color=c=red:s=8x8:r=4:d=0.5[r];color=c=blue:s=8x8:r=4:d=0.5[b];[r][b]concat=n=2:v=1:a=0",
            "-c:v", "ffv1", "-y"]).arg(&path).status()?;
        ensure!(status.success(), "cannot generate video fixture");
        let outcome = (|| -> Result<()> {
            let mut decoder = VideoDecoder::open(&path)?;
            assert_eq!((decoder.info().width, decoder.info().height), (8, 8));
            for (seconds, red) in [
                (0.0, true),
                (0.75, false),
                (1.25, true),
                (30.75, false),
                (0.0, true),
            ] {
                let frame = decoder.frame_at(Duration::from_secs_f64(seconds))?;
                assert_eq!(frame.rgba.len(), 8 * 8 * 4);
                if red {
                    assert!(frame.rgba[0] > 240 && frame.rgba[1] < 20 && frame.rgba[2] < 20);
                } else {
                    assert!(frame.rgba[0] < 20 && frame.rgba[1] < 20 && frame.rgba[2] > 240);
                }
                assert_eq!(frame.rgba[3], 255);
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(path);
        outcome
    }
}
