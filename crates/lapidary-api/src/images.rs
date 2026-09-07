//! Turning bytes somebody handed us into an image we are willing to store.
//!
//! Every path into the gallery comes through [`normalize`] — an upload today, a fetched URL
//! next — and it exists because none of those bytes are trustworthy. `docs/DATA.md` §4.1
//! sets the controls; this is where the ones about the *bytes* live, and `fetch.rs` owns
//! the ones about the *network*.
//!
//! What it does, in order, and each step is load-bearing:
//!
//! 1. **Reads the magic bytes, never a declared type.** A `Content-Type: image/png` is a
//!    claim by whoever sent it. `image::guess_format` reads the file's own header, and a
//!    format we did not compile a decoder for is refused here rather than by a decoder that
//!    does not exist.
//! 2. **Decodes under explicit [`image::Limits`].** The reason is in `DATA.md` §4.1 and it
//!    is not hypothetical: a 200 KB PNG can declare 50000×50000 and take the process out
//!    before a single pixel is wrong. The limit is on the allocation, so it refuses before
//!    it allocates.
//! 3. **Bounds the result at both ends.** Too small is refused — a 16×16 favicon dragged
//!    off a web page is not a photograph of a part, and storing it would put a smudge on a
//!    card where a picture should be. Too large is *resized*, not refused: a phone
//!    photograph is legitimately 12 megapixels and the person attaching it has done nothing
//!    wrong.
//! 4. **Re-encodes to WebP.** This is the step that does the most for the least: it
//!    normalizes every input to one format, it means a decompression bomb never reaches
//!    disk in its original shape, and **it strips EXIF**, which downloaded and phone images
//!    carry surprisingly often and which contains GPS coordinates surprisingly often.
//!
//! The output is bytes and a size, and nothing here writes anything. Where those bytes go —
//! inline under 64 KB, a blob above — is `part_image`'s rule and the caller's business.

use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

/// The largest input we will look at, before decoding.
///
/// The same 10 MB `DATA.md` §4.1 puts on a fetched URL, applied to an upload for the same
/// reason: it is the ceiling on what a single request can make this process hold. A
/// photograph of a part is well under it; anything over is a file somebody meant to send
/// somewhere else.
pub const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;

/// The largest pixel buffer a decode may allocate.
///
/// Not the same thing as [`MAX_INPUT_BYTES`], and that is the entire point: the input is
/// compressed, and the ratio between the two is what a decompression bomb is made of. 4096
/// × 4096 × 4 bytes is 64 MiB, which bounds one decode inside a container limited to 512 MB
/// while leaving room for an image nobody would call unreasonable.
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;

/// Below this on the short edge, we refuse rather than store.
///
/// A favicon, a tracking pixel or a spacer GIF dragged off a page is not a picture of a
/// part, and putting one on a card is worse than showing the render — it looks like the
/// application got it wrong rather than like the input did.
pub const MIN_EDGE_PX: u32 = 64;

/// Above this on the long edge, we resize rather than refuse.
///
/// A phone photograph is 4000-odd pixels wide and the person attaching it has done nothing
/// wrong, so refusing would be punishing them for owning a camera. A card renders at a few
/// hundred pixels and the detail view at a thousand; 2048 is generous for both and keeps a
/// stored image in the hundreds of kilobytes rather than the megabytes.
pub const MAX_EDGE_PX: u32 = 2048;

/// Why a candidate image was refused. Every variant is a sentence a user can act on —
/// `CLAUDE.md`: errors say what broke and what to do.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error(
        "That file is {got} bytes, and the limit is {MAX_INPUT_BYTES}. Save it at a smaller size, or export it as JPEG rather than PNG, and try again."
    )]
    TooLarge { got: usize },

    #[error(
        "That does not look like an image file. Lapidary reads PNG, JPEG and WebP; the file's own header says it is none of them, whatever its name or content type claims."
    )]
    NotAnImage,

    #[error(
        "That image could not be read. It may be truncated, or saved in a variant of its format that this build does not decode — open it and re-export it, and try again."
    )]
    Undecodable,

    #[error(
        "That image is {width}×{height}, and the smallest side must be at least {MIN_EDGE_PX} pixels. That size is usually an icon or a placeholder picked up by mistake rather than a photograph."
    )]
    TooSmall { width: u32, height: u32 },

    #[error(
        "That image could not be re-encoded, so it has not been stored. This is a fault on our side rather than a problem with your file; the api service log has the detail."
    )]
    Unencodable,
}

/// A stored-ready image: WebP bytes, and what they came out as.
#[derive(Debug)]
pub struct NormalizedImage {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Take untrusted bytes to WebP, or say why not.
///
/// See the module doc for the order and why each step is there. Nothing here touches the
/// filesystem or the database.
pub fn normalize(bytes: &[u8]) -> Result<NormalizedImage, ImageError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ImageError::TooLarge { got: bytes.len() });
    }

    // The file's own header, not a caller's claim about it. `guess_format` also rejects the
    // formats this build has no decoder for, which is every format but three — so a TIFF
    // stops here rather than inside a decoder that was never compiled in.
    let format = image::guess_format(bytes).map_err(|_| ImageError::NotAnImage)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(ImageError::NotAnImage);
    }

    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    // **The bomb guard.** Set before `decode`, and on the allocation rather than on the
    // declared dimensions, so an image claiming 50000×50000 is refused when it asks for the
    // memory instead of after it gets it.
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);

    let decoded = reader.decode().map_err(|_| ImageError::Undecodable)?;
    let (width, height) = (decoded.width(), decoded.height());
    if width.min(height) < MIN_EDGE_PX {
        return Err(ImageError::TooSmall { width, height });
    }

    // Resized, not refused: a phone photograph is legitimately this big. `thumbnail` keeps
    // the aspect ratio and picks the cheaper filter, which is the right trade for something
    // that is about to be looked at rather than measured.
    let bounded = if width.max(height) > MAX_EDGE_PX {
        decoded.thumbnail(MAX_EDGE_PX, MAX_EDGE_PX)
    } else {
        decoded
    };

    Ok(NormalizedImage {
        width: bounded.width(),
        height: bounded.height(),
        webp: encode_webp(&bounded)?,
    })
}

/// The re-encode, and the step that strips EXIF.
///
/// Not "we also remove metadata": there is no metadata-removal code here, and there does not
/// need to be. The encoder is handed a decoded pixel buffer, which carries no EXIF because
/// EXIF is a container-level thing — so what comes out has none, by construction rather than
/// by a filter somebody has to remember to keep working.
fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>, ImageError> {
    // Lossless: these are stored once and read many times, and a lossy pass over a
    // photograph that is already a lossy JPEG stacks a second generation of artefacts onto
    // the thing somebody is looking at in order to decide something.
    let rgba = image.to_rgba8();
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .encode(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| {
            tracing::error!(error = %err, "webp encode failed for an image that decoded");
            ImageError::Unencodable
        })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    /// A real image of `width`×`height`, encoded as `format`. Built rather than committed:
    /// a fixture file would be bytes nobody in this repository could read to check.
    fn encoded(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let buffer = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        });
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(buffer)
            .write_to(&mut out, format)
            .expect("the fixture encodes");
        out.into_inner()
    }

    #[test]
    fn a_png_comes_back_as_webp_at_its_own_size() {
        let out = normalize(&encoded(320, 240, ImageFormat::Png)).expect("a real png normalizes");
        assert_eq!(
            (out.width, out.height),
            (320, 240),
            "within bounds, so unchanged"
        );
        assert_eq!(
            image::guess_format(&out.webp).expect("the output has a format"),
            ImageFormat::WebP,
            "one stored format, whatever went in"
        );
    }

    #[test]
    fn a_jpeg_comes_back_as_webp_too() {
        let out = normalize(&encoded(320, 240, ImageFormat::Jpeg)).expect("a real jpeg normalizes");
        assert_eq!(
            image::guess_format(&out.webp).expect("the output has a format"),
            ImageFormat::WebP
        );
    }

    /// The bound that is a resize rather than a refusal, because a phone photograph is
    /// legitimately this big and its owner has done nothing wrong.
    #[test]
    fn an_image_over_the_long_edge_is_resized_and_keeps_its_shape() {
        let out = normalize(&encoded(3000, 1500, ImageFormat::Png)).expect("normalizes");
        assert_eq!(out.width, MAX_EDGE_PX, "the long edge lands on the bound");
        assert_eq!(
            out.height,
            MAX_EDGE_PX / 2,
            "and the aspect ratio survives — a squashed photograph is worse than a large one"
        );
    }

    /// The bound that *is* a refusal. A 16×16 dragged off a web page is an icon, and putting
    /// one on a card looks like the application got it wrong rather than the input.
    #[test]
    fn a_favicon_sized_image_is_refused_and_says_what_is_wrong_with_it() {
        let err = normalize(&encoded(16, 16, ImageFormat::Png)).expect_err("refused");
        assert!(matches!(err, ImageError::TooSmall { .. }));
        let message = err.to_string();
        assert!(
            message.contains("16×16") && message.contains("64"),
            "the refusal names the size it got and the size it wants: {message}"
        );
    }

    /// **The magic-byte check, which is the one a content type cannot do.** `DATA.md` §4.1
    /// requires both, and this is why: a caller controls the header and does not control the
    /// header of the file.
    #[test]
    fn bytes_that_are_not_an_image_are_refused_however_they_are_labelled() {
        let err = normalize(b"GIF89a not really, and not a format we decode either")
            .expect_err("refused");
        assert!(matches!(err, ImageError::NotAnImage));
    }

    /// A format the build has no decoder for stops at the header rather than inside a
    /// decoder that is not there. `default-features = false` is what makes that true, and
    /// this is the test that notices if somebody turns them back on.
    #[test]
    fn a_format_this_build_does_not_decode_is_refused_at_the_header() {
        // A BMP header: a real image format, and deliberately not one of the three.
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 64]);
        assert!(matches!(
            normalize(&bmp).expect_err("refused"),
            ImageError::NotAnImage
        ));
    }

    /// Truncation is a different failure from "not an image", and says something different
    /// to whoever hit it.
    #[test]
    fn a_truncated_image_is_refused_as_unreadable_rather_than_as_not_an_image() {
        let png = encoded(320, 240, ImageFormat::Png);
        let half = &png[..png.len() / 2];
        assert!(matches!(
            normalize(half).expect_err("refused"),
            ImageError::Undecodable
        ));
    }

    /// The input ceiling, refused **before** any decoding is attempted — which is the point
    /// of checking the length first rather than letting the decoder find out.
    #[test]
    fn an_oversized_file_is_refused_without_being_decoded() {
        let err = normalize(&vec![0u8; MAX_INPUT_BYTES + 1]).expect_err("refused");
        assert!(
            matches!(err, ImageError::TooLarge { .. }),
            "and as too large rather than as not-an-image, which is what a decoder would \
             have said about the same bytes"
        );
    }

    /// **The decompression-bomb guard**, exercised rather than asserted about.
    ///
    /// 4200×4200 RGBA is 70.5 MB, over `MAX_DECODE_BYTES`, and compresses to well under the
    /// input ceiling — which is exactly the ratio a bomb is made of. Without `Limits` this
    /// decodes and allocates; with them it is refused. `DATA.md` §4.1: a 200 KB PNG can
    /// declare 50000×50000 and take the process out.
    #[test]
    fn an_image_that_would_allocate_past_the_decode_limit_is_refused() {
        let big = encoded(4200, 4200, ImageFormat::Png);
        assert!(
            big.len() < MAX_INPUT_BYTES,
            "the fixture has to be small enough to get past the input ceiling, or this test \
             proves the wrong guard: {} bytes",
            big.len()
        );
        assert!(matches!(
            normalize(&big).expect_err("the limit refuses it"),
            ImageError::Undecodable
        ));
    }
}
