//! Shared media decoding. FFmpeg produces a bounded stream of CPU RGBA frames.
//!
//! Audio playback and system-audio capture are not implemented yet. The video
//! decoder deliberately disables audio rather than starting an unsynchronised
//! second player.

mod file;
mod video;

pub use file::MediaFile;
pub use video::{DecodedFrame, VideoDecoder, VideoInfo};
