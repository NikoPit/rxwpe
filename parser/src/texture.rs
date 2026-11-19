//! Decodes ordinary images and the first image/mipmap of Wallpaper Engine TEX files.
//!
//! TEX container layout is implemented from the publicly documented behavior of
//! compatible readers. Animated TEX and video textures need a playback runtime.

use anyhow::{Context, Result, bail, ensure};
use image::{ImageReader, Limits};
use std::io::Cursor;

const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone)]
pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Returns the borrowed encoded video payload of a TEX video container.
/// Video size is bounded by the asset store, independently of decoded image size.
pub fn video_texture(bytes: &[u8]) -> Result<Option<&[u8]>> {
    if !bytes.starts_with(b"TEXV") {
        return Ok(None);
    }
    let mut reader = Reader { bytes, offset: 0 };
    ensure!(reader.take(9)? == b"TEXV0005\0", "unsupported TEX version");
    ensure!(
        reader.take(9)? == b"TEXI0001\0",
        "unsupported TEX image header"
    );
    reader.u32()?;
    let flags = reader.u32()?;
    if flags & 32 == 0 {
        return Ok(None);
    }
    for _ in 0..5 {
        reader.u32()?;
    }
    let version = match reader.take(9)? {
        b"TEXB0003\0" => 3,
        b"TEXB0004\0" => 4,
        _ => bail!("unsupported video TEX data container"),
    };
    ensure!(reader.u32()? == 1, "video TEX requires one image");
    reader.u32()?;
    if version == 4 {
        ensure!(reader.u32()? == 1, "unsupported TEX video codec");
    }
    ensure!(reader.u32()? == 1, "video TEX requires one mipmap");
    if version == 4 {
        reader.u32()?;
        reader.u32()?;
        let length = reader.bytes[reader.offset..]
            .iter()
            .position(|v| *v == 0)
            .context("unterminated TEX video metadata")?;
        reader.take(length + 1)?;
        reader.u32()?;
    }
    let width = reader.u32()?;
    let height = reader.u32()?;
    rgba_size(width, height)?;
    ensure!(
        reader.u32()? == 0,
        "compressed TEX video payload is unsupported"
    );
    let decoded = reader.u32()? as usize;
    let stored = reader.u32()? as usize;
    ensure!(
        stored > 0 && (decoded == 0 || decoded == stored),
        "invalid TEX video payload size"
    );
    Ok(Some(reader.take(stored)?))
}

/// Decodes a static texture to tightly packed RGBA8 pixels.
///
/// Animation, video payloads and shader-dependent single/dual-channel textures
/// return an error instead of silently presenting an incorrect static image.
pub fn decode_texture(bytes: &[u8]) -> Result<DecodedTexture> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "texture file exceeds the 256 MiB limit"
    );
    if !bytes.starts_with(b"TEXV") {
        return decode_image(bytes);
    }
    let mut reader = Reader { bytes, offset: 0 };
    ensure!(reader.take(9)? == b"TEXV0005\0", "unsupported TEX version");
    ensure!(
        reader.take(9)? == b"TEXI0001\0",
        "unsupported TEX image header"
    );
    let format = reader.u32()?;
    let flags = reader.u32()?;
    let _storage_width = reader.u32()?;
    let _storage_height = reader.u32()?;
    let logical_width = reader.u32()?;
    let logical_height = reader.u32()?;
    let _reserved = reader.u32()?;
    ensure!(
        flags & 4 == 0,
        "animated TEX textures are not supported yet"
    );
    ensure!(flags & 32 == 0, "video TEX textures require video playback");
    ensure!(
        flags & 1 == 0,
        "nearest-filtered TEX textures require a per-texture sampler"
    );
    let version = match reader.take(9)? {
        b"TEXB0001\0" => 1,
        b"TEXB0002\0" => 2,
        b"TEXB0003\0" => 3,
        b"TEXB0004\0" => 4,
        _ => bail!("unsupported TEX data container"),
    };
    let images = reader.u32()?;
    ensure!(
        images == 1,
        "TEX with {images} images requires an animation or array runtime"
    );
    let encoded_format = if version >= 3 {
        reader.u32()?
    } else {
        u32::MAX
    };
    if version == 4 {
        ensure!(
            reader.u32()? == 0,
            "video TEX textures require video playback"
        );
    }
    let mipmaps = reader.u32()?;
    ensure!(
        (1..=32).contains(&mipmaps),
        "invalid TEX mipmap count {mipmaps}"
    );
    let mut first = None;
    for level in 0..mipmaps {
        // Non-video TEXB0004 uses the same mipmap layout as TEXB0003.
        let width = reader.u32()?;
        let height = reader.u32()?;
        rgba_size(width, height)?;
        let (compression, decoded_size) = if version >= 2 {
            (reader.u32()?, reader.u32()? as usize)
        } else {
            (0, 0)
        };
        ensure!(
            compression <= 1,
            "unsupported TEX compression method {compression}"
        );
        let stored_size = reader.u32()? as usize;
        ensure!(
            stored_size <= MAX_BYTES,
            "TEX mipmap exceeds the size limit"
        );
        let payload = reader.take(stored_size)?;
        // Validate all declared data, even when only the highest mip is decoded.
        ensure!(
            decoded_size <= MAX_BYTES,
            "decoded TEX mipmap exceeds the size limit"
        );
        if level == 0 {
            let data = if compression == 1 {
                ensure!(decoded_size > 0, "empty decoded TEX mipmap");
                lz4_flex::block::decompress(payload, decoded_size)
                    .context("invalid LZ4 texture block")?
            } else {
                ensure!(
                    decoded_size == 0 || decoded_size == stored_size,
                    "TEX uncompressed mipmap size does not match its payload"
                );
                payload.to_vec()
            };
            let decoded = if encoded_format != u32::MAX {
                decode_image(&data).context("unsupported or invalid image embedded in TEX")?
            } else {
                decode_raw(format, width, height, &data)?
            };
            first = Some(decoded);
        }
    }
    let decoded = first.context("TEX has no mipmaps")?;
    ensure!(
        logical_width > 0 && logical_height > 0,
        "TEX logical dimensions must be positive"
    );
    ensure!(
        logical_width <= decoded.width && logical_height <= decoded.height,
        "TEX logical dimensions exceed the decoded texture dimensions"
    );
    crop(decoded, logical_width, logical_height)
}

fn decode_image(bytes: &[u8]) -> Result<DecodedTexture> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().context("unsupported or invalid image")?;
    // Decoder allocation limits do not account for a later RGB-to-RGBA
    // conversion, which may require a larger allocation than the source image.
    rgba_size(image.width(), image.height())?;
    let image = image.to_rgba8();
    Ok(DecodedTexture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn rgba_size(width: u32, height: u32) -> Result<usize> {
    ensure!(
        width > 0 && height > 0 && width <= MAX_DIMENSION && height <= MAX_DIMENSION,
        "texture dimensions {width}x{height} exceed supported limits"
    );
    let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|x| x.checked_mul(4))
        .context("texture dimensions overflow")?;
    ensure!(
        size <= MAX_BYTES,
        "decoded texture exceeds the 256 MiB limit"
    );
    Ok(size)
}

// slice::as_chunks requires Rust 1.88; keep compatibility with the workspace MSRV.
#[allow(clippy::chunks_exact_to_as_chunks)]
fn decode_raw(format: u32, width: u32, height: u32, bytes: &[u8]) -> Result<DecodedTexture> {
    let size = rgba_size(width, height)?;
    let pixels = size / 4;
    let rgba = match format {
        0 => {
            ensure!(bytes.len() == size, "RGBA texture payload size mismatch");
            bytes.to_vec()
        }
        1 => {
            ensure!(
                bytes.len() == pixels * 3,
                "RGB texture payload size mismatch"
            );
            bytes
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        }
        2 => {
            ensure!(
                bytes.len() == pixels * 2,
                "RGB565 texture payload size mismatch"
            );
            bytes
                .chunks_exact(2)
                .flat_map(|p| {
                    let [r, g, b] = rgb565(u16::from_le_bytes([p[0], p[1]]));
                    [r, g, b, 255]
                })
                .collect()
        }
        4 | 6 | 7 => decode_bc(format, width, height, bytes)?,
        8 | 9 => bail!("TEX format {format} requires material-specific channel swizzling"),
        _ => bail!("unsupported TEX pixel format {format}"),
    };
    Ok(DecodedTexture {
        width,
        height,
        rgba,
    })
}

fn crop(texture: DecodedTexture, width: u32, height: u32) -> Result<DecodedTexture> {
    if width == texture.width && height == texture.height {
        return Ok(texture);
    }
    let mut rgba = Vec::with_capacity(rgba_size(width, height)?);
    let source_stride = texture.width as usize * 4;
    let stride = width as usize * 4;
    for row in 0..height as usize {
        rgba.extend_from_slice(&texture.rgba[row * source_stride..row * source_stride + stride]);
    }
    Ok(DecodedTexture {
        width,
        height,
        rgba,
    })
}

fn rgb565(value: u16) -> [u8; 3] {
    let r = ((value >> 11) & 31) as u8;
    let g = ((value >> 5) & 63) as u8;
    let b = (value & 31) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

fn decode_bc(format: u32, width: u32, height: u32, bytes: &[u8]) -> Result<Vec<u8>> {
    let block_size = if format == 7 { 8 } else { 16 };
    let columns = width.div_ceil(4) as usize;
    let rows = height.div_ceil(4) as usize;
    ensure!(
        bytes.len() == columns * rows * block_size,
        "BC texture payload size mismatch"
    );
    let mut rgba = vec![0; rgba_size(width, height)?];
    for (block_index, block) in bytes.chunks_exact(block_size).enumerate() {
        let color_offset = if format == 7 { 0 } else { 8 };
        let color = &block[color_offset..];
        let a = u16::from_le_bytes([color[0], color[1]]);
        let b = u16::from_le_bytes([color[2], color[3]]);
        let c0 = rgb565(a);
        let c1 = rgb565(b);
        let mut palette = [[0; 4]; 4];
        palette[0] = [c0[0], c0[1], c0[2], 255];
        palette[1] = [c1[0], c1[1], c1[2], 255];
        for channel in 0..3 {
            if a > b || format != 7 {
                palette[2][channel] = ((2 * c0[channel] as u16 + c1[channel] as u16) / 3) as u8;
                palette[3][channel] = ((c0[channel] as u16 + 2 * c1[channel] as u16) / 3) as u8;
            } else {
                palette[2][channel] = ((c0[channel] as u16 + c1[channel] as u16) / 2) as u8;
            }
        }
        palette[2][3] = 255;
        palette[3][3] = if a > b || format != 7 { 255 } else { 0 };
        let indices = u32::from_le_bytes(color[4..8].try_into().expect("four color-index bytes"));
        let alpha = if format == 4 {
            Some(bc3_alpha(block))
        } else {
            None
        };
        for i in 0..16 {
            let x = (block_index % columns) * 4 + i % 4;
            let y = (block_index / columns) * 4 + i / 4;
            if x >= width as usize || y >= height as usize {
                continue;
            }
            let mut pixel = palette[((indices >> (2 * i)) & 3) as usize];
            if format == 6 {
                pixel[3] = ((block[i / 2] >> ((i % 2) * 4)) & 15) * 17;
            } else if let Some(alpha) = &alpha {
                pixel[3] = alpha[i];
            }
            let offset = (y * width as usize + x) * 4;
            rgba[offset..offset + 4].copy_from_slice(&pixel);
        }
    }
    Ok(rgba)
}

fn bc3_alpha(block: &[u8]) -> [u8; 16] {
    let a = block[0] as u16;
    let b = block[1] as u16;
    let mut palette = [0_u8; 8];
    palette[0] = a as u8;
    palette[1] = b as u8;
    if a > b {
        for i in 1..=6 {
            palette[i + 1] = (((7 - i) as u16 * a + i as u16 * b) / 7) as u8;
        }
    } else {
        for i in 1..=4 {
            palette[i + 1] = (((5 - i) as u16 * a + i as u16 * b) / 5) as u8;
        }
        palette[6] = 0;
        palette[7] = 255;
    }
    let mut indices = 0_u64;
    for (i, &byte) in block[2..8].iter().enumerate() {
        indices |= (byte as u64) << (i * 8);
    }
    std::array::from_fn(|i| palette[((indices >> (i * 3)) & 7) as usize])
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(size)
            .context("TEX length overflow")?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .context("truncated TEX data")?;
        self.offset = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(format: u32, width: u32, height: u32, payload: &[u8], compressed: bool) -> Vec<u8> {
        let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
        for value in [format, 0, width, height, width, height, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"TEXB0002\0");
        for value in [
            1,
            1,
            width,
            height,
            u32::from(compressed),
            payload.len() as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let stored = if compressed {
            lz4_flex::block::compress(payload)
        } else {
            payload.to_vec()
        };
        bytes.extend_from_slice(&(stored.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&stored);
        bytes
    }

    #[test]
    fn rgba_and_lz4_preserve_exact_pixels() {
        let pixels = [255, 0, 17, 128, 9, 210, 0, 255];
        for compressed in [false, true] {
            let decoded = decode_texture(&tex(0, 2, 1, &pixels, compressed)).unwrap();
            assert_eq!((decoded.width, decoded.height), (2, 1));
            assert_eq!(decoded.rgba, pixels);
        }
    }

    #[test]
    fn bc1_transparency_and_partial_edge_block() {
        let mut block = vec![0, 0, 255, 255];
        block.extend_from_slice(&u32::MAX.to_le_bytes());
        let decoded = decode_texture(&tex(7, 1, 1, &block, false)).unwrap();
        assert_eq!(decoded.rgba, [0, 0, 0, 0]);
    }

    #[test]
    fn bc2_explicit_alpha_and_bc3_interpolated_alpha() {
        let mut bc2 = vec![0x88; 8];
        bc2.extend_from_slice(&[0, 248, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &decode_texture(&tex(6, 4, 4, &bc2, false)).unwrap().rgba[..4],
            &[255, 0, 0, 136]
        );
        let mut bc3 = vec![255, 0, 0, 0, 0, 0, 0, 0];
        bc3.extend_from_slice(&[224, 7, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &decode_texture(&tex(4, 4, 4, &bc3, false)).unwrap().rgba[..4],
            &[0, 255, 0, 255]
        );
    }

    #[test]
    fn truncated_and_oversized_payloads_return_errors() {
        let valid = tex(0, 1, 1, &[1, 2, 3, 4], false);
        for length in 0..valid.len() {
            assert!(decode_texture(&valid[..length]).is_err());
        }
        assert!(decode_texture(&tex(0, u32::MAX, 1, &[], false)).is_err());
        assert!(decode_texture(&tex(0, 1, 1, &[1, 2, 3], false)).is_err());
    }

    #[test]
    fn texb3_embedded_images_use_logical_dimensions() {
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 0, 255, 0, 255, 0, 255],
                2,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut bytes = tex(0, 2, 1, &png, true);
        bytes[46..55].copy_from_slice(b"TEXB0003\0");
        bytes.splice(59..59, 13u32.to_le_bytes()); // FIF_PNG
        bytes[34..38].copy_from_slice(&1u32.to_le_bytes());
        let decoded = decode_texture(&bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (1, 1));
        assert_eq!(decoded.rgba, [255, 0, 0, 255]);
    }

    #[test]
    fn texb4_static_raw_layout_and_video_rejection() {
        let mut bytes = tex(0, 1, 1, &[5, 6, 7, 255], false);
        bytes[46..55].copy_from_slice(b"TEXB0004\0");
        bytes.splice(
            59..59,
            u32::MAX.to_le_bytes().into_iter().chain(0u32.to_le_bytes()),
        );
        assert_eq!(decode_texture(&bytes).unwrap().rgba, [5, 6, 7, 255]);
        bytes[63..67].copy_from_slice(&1u32.to_le_bytes());
        assert!(
            decode_texture(&bytes)
                .unwrap_err()
                .to_string()
                .contains("video")
        );
    }

    #[test]
    fn extracts_video_without_decoding_or_copying_its_payload() {
        let mut bytes = tex(0, 2, 1, b"encoded video", false);
        bytes[22..26].copy_from_slice(&32u32.to_le_bytes());
        bytes[46..55].copy_from_slice(b"TEXB0003\0");
        bytes.splice(59..59, u32::MAX.to_le_bytes());
        assert_eq!(video_texture(&bytes).unwrap().unwrap(), b"encoded video");
        assert!(video_texture(&bytes[..bytes.len() - 1]).is_err());
        bytes[75..79].copy_from_slice(&1u32.to_le_bytes());
        assert!(video_texture(&bytes).is_err());
    }
}
