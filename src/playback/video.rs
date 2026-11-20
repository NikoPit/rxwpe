//! A video-type wallpaper player. Decode is shared through `super::decoder`;
//! completed frames are uploaded to the same GPU used by the desktop output.

use std::{path::Path, time::Duration};

use super::decoder::{VideoDecoder, VideoInfo};
use crate::render::{Frame, Gpu};
use anyhow::Result;

pub(crate) struct VideoPlayer {
    decoder: VideoDecoder,
    uploaded: Option<(Duration, Frame)>,
    clock: PlaybackClock,
}

#[derive(Default)]
struct PlaybackClock {
    paused_at: Option<Duration>,
    paused_duration: Duration,
}

impl VideoPlayer {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            decoder: VideoDecoder::open(path)?,
            uploaded: None,
            clock: PlaybackClock::default(),
        })
    }

    pub(crate) fn info(&self) -> &VideoInfo {
        self.decoder.info()
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.info().width, self.info().height)
    }

    /// `elapsed` is time since this player was started, measured by the host.
    pub(crate) fn render(&mut self, gpu: &Gpu, elapsed: Duration) -> Result<Frame> {
        let time = self.clock.time(elapsed);
        let decoded = self.decoder.frame_at(time)?;
        if let Some((timestamp, frame)) = &self.uploaded
            && *timestamp == decoded.timestamp
        {
            return Ok(frame.clone());
        }
        let frame = Frame {
            image: gpu.upload_rgba(decoded.width, decoded.height, &decoded.rgba)?,
        };
        self.uploaded = Some((decoded.timestamp, frame.clone()));
        Ok(frame)
    }

    pub(crate) fn pause(&mut self, elapsed: Duration) {
        self.clock.pause(elapsed);
    }

    pub(crate) fn resume(&mut self, elapsed: Duration) {
        self.clock.resume(elapsed);
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.clock.paused_at.is_some()
    }
}

impl PlaybackClock {
    fn time(&self, elapsed: Duration) -> Duration {
        self.paused_at
            .unwrap_or(elapsed)
            .saturating_sub(self.paused_duration)
    }

    fn pause(&mut self, elapsed: Duration) {
        self.paused_at.get_or_insert(elapsed);
    }

    fn resume(&mut self, elapsed: Duration) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_duration += elapsed.saturating_sub(paused_at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::MediaFile;
    use super::*;
    use anyhow::{Context, ensure};
    use std::{process::Command, sync::Arc};

    #[test]
    fn clock_freezes_until_resume_and_excludes_paused_time() {
        let mut clock = PlaybackClock::default();
        assert_eq!(clock.time(Duration::from_secs(2)), Duration::from_secs(2));
        clock.pause(Duration::from_secs(2));
        clock.pause(Duration::from_secs(5));
        assert_eq!(clock.time(Duration::from_secs(8)), Duration::from_secs(2));
        clock.resume(Duration::from_secs(8));
        assert_eq!(clock.time(Duration::from_secs(8)), Duration::from_secs(2));
        assert_eq!(clock.time(Duration::from_secs(9)), Duration::from_secs(3));
        assert!(clock.paused_at.is_none());
    }

    #[test]
    fn clock_accumulates_multiple_pauses_and_ignores_duplicate_resume() {
        let mut clock = PlaybackClock::default();
        clock.resume(Duration::from_secs(1));
        clock.pause(Duration::from_secs(2));
        clock.resume(Duration::from_secs(5));
        clock.resume(Duration::from_secs(6));
        clock.pause(Duration::from_secs(7));
        clock.resume(Duration::from_secs(11));
        assert_eq!(clock.time(Duration::from_secs(12)), Duration::from_secs(5));
    }

    #[test]
    fn clock_saturates_when_host_time_moves_backwards() {
        let mut clock = PlaybackClock::default();
        clock.pause(Duration::from_secs(5));
        clock.resume(Duration::from_secs(2));
        assert_eq!(clock.time(Duration::from_secs(3)), Duration::from_secs(3));
        clock.pause(Duration::from_secs(5));
        clock.resume(Duration::from_secs(10));
        assert_eq!(clock.time(Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    #[ignore = "requires FFmpeg and a GPU adapter, including software renderer"]
    fn playback_reuses_uploads_and_freezes_frames_while_paused() -> Result<()> {
        let output = Command::new("ffmpeg").args([
            "-nostdin", "-v", "error", "-f", "lavfi", "-i",
            "color=c=red:s=8x8:r=4:d=0.5[r];color=c=blue:s=8x8:r=4:d=0.5[b];[r][b]concat=n=2:v=1:a=0",
            "-c:v", "ffv1", "-f", "matroska", "pipe:1",
        ]).output().context("generate video fixture with FFmpeg")?;
        ensure!(
            output.status.success(),
            "FFmpeg fixture generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let file = MediaFile::from_bytes(&output.stdout)?;
        let gpu = Gpu::headless()?;
        let mut player = VideoPlayer::open(file.path())?;
        assert_eq!(player.dimensions(), (8, 8));
        assert!(!player.is_paused());
        let first = player.render(&gpu, Duration::ZERO)?;
        let same = player.render(&gpu, Duration::from_millis(100))?;
        assert!(Arc::ptr_eq(&first.image, &same.image));
        let red = gpu.read_rgba(&first)?;
        assert!(red[0] > 240 && red[2] < 20);

        player.pause(Duration::from_millis(100));
        player.pause(Duration::from_secs(2));
        assert!(player.is_paused());
        let frozen = player.render(&gpu, Duration::from_secs(3))?;
        assert!(Arc::ptr_eq(&first.image, &frozen.image));
        player.resume(Duration::from_secs(3));
        player.resume(Duration::from_secs(3));
        assert!(!player.is_paused());
        let resumed = player.render(&gpu, Duration::from_secs(3))?;
        assert!(Arc::ptr_eq(&first.image, &resumed.image));
        let next = player.render(&gpu, Duration::from_millis(3650))?;
        assert!(!Arc::ptr_eq(&first.image, &next.image));
        let blue = gpu.read_rgba(&next)?;
        assert!(blue[0] < 20 && blue[2] > 240);
        let looped = player.render(&gpu, Duration::from_millis(4150))?;
        let red = gpu.read_rgba(&looped)?;
        assert!(red[0] > 240 && red[2] < 20);
        Ok(())
    }
}
