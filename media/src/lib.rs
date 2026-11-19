//! Shared media decoding. FFmpeg produces a bounded stream of CPU RGBA frames.
//!
//! Scene audio playback uses an owned FFplay process. The video decoder
//! deliberately disables embedded video audio to avoid unsynchronised streams.

mod audio;
mod file;
mod video;

pub use audio::AudioPlayback;
pub use file::MediaFile;
pub use video::{DecodedFrame, VideoDecoder, VideoInfo};
