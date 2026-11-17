//! A video-type wallpaper player. Decode is shared through `rxwpe-media`;
//! completed frames are uploaded to the same GPU used by the desktop output.

use std::{path::Path, time::Duration};

use anyhow::Result;
use rxwpe_gpu::{Frame, Gpu};
use rxwpe_media::{VideoDecoder, VideoInfo};

pub struct VideoPlayer {
    decoder: VideoDecoder,
    uploaded: Option<(Duration, Frame)>,
    paused_at: Option<Duration>,
    paused_duration: Duration,
}

impl VideoPlayer {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            decoder: VideoDecoder::open(path)?,
            uploaded: None,
            paused_at: None,
            paused_duration: Duration::ZERO,
        })
    }

    pub fn info(&self) -> &VideoInfo {
        self.decoder.info()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.info().width, self.info().height)
    }

    /// `elapsed` is time since this player was started, measured by the host.
    pub fn render(&mut self, gpu: &Gpu, elapsed: Duration) -> Result<Frame> {
        let time = self
            .paused_at
            .unwrap_or(elapsed)
            .saturating_sub(self.paused_duration);
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

    pub fn pause(&mut self, elapsed: Duration) {
        self.paused_at.get_or_insert(elapsed);
    }

    pub fn resume(&mut self, elapsed: Duration) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_duration += elapsed.saturating_sub(paused_at);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }
}
