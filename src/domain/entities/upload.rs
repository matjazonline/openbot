//! What the app accepts when someone picks a file: the bytes, and what they actually are.
//!
//! The client's `Content-Type` and file name are both taken as hints, never as facts — a browser
//! will happily label anything, and these bytes end up on a public URL that other people's
//! browsers then render. The format is therefore decided here, from the magic bytes, at the one
//! place an upload enters the system.

/// The picture formats an avatar may be stored in.
///
/// An enum rather than the submitted MIME string because every downstream use — the extension in
/// the object key, the `Content-Type` written to the bucket — has to agree about what the file is,
/// and a new format should be a compile error at each of them rather than an unhandled string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    /// What these bytes are, by their signature; `None` for anything that is not a picture.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
        const JPEG: &[u8] = b"\xff\xd8\xff";
        const GIF87: &[u8] = b"GIF87a";
        const GIF89: &[u8] = b"GIF89a";

        if bytes.starts_with(PNG) {
            Some(Self::Png)
        } else if bytes.starts_with(JPEG) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(GIF87) || bytes.starts_with(GIF89) {
            Some(Self::Gif)
        } else if is_webp(bytes) {
            Some(Self::Webp)
        } else {
            None
        }
    }

    /// The `Content-Type` the object is stored and served with.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// The extension the object key ends in.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// What a file picker should offer, so the browser filters before the bytes are ever sent.
    pub const ACCEPT_ATTRIBUTE: &'static str = "image/png,image/jpeg,image/gif,image/webp";
}

/// A RIFF container whose form type is `WEBP` — `RIFF` + 4 size bytes + `WEBP`.
fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
}

/// An image that has been accepted for storage: bytes that are known to be a picture, small
/// enough to keep, paired with the format they were recognized as.
#[derive(Debug, Clone)]
pub struct ImageUpload {
    format: ImageFormat,
    bytes: Vec<u8>,
}

impl ImageUpload {
    /// The largest avatar accepted, matching the body limit on the upload route.
    pub const MAX_BYTES: usize = 5 * 1024 * 1024;

    /// What a picked file means: the picture, or why it was refused, worded for the person who
    /// picked it.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("Choose a file to upload.".to_string());
        }

        if bytes.len() > Self::MAX_BYTES {
            return Err(format!(
                "That file is {:.1} MB. Pictures have to be under {} MB.",
                bytes.len() as f64 / (1024.0 * 1024.0),
                Self::MAX_BYTES / (1024 * 1024),
            ));
        }

        let Some(format) = ImageFormat::detect(&bytes) else {
            return Err("That file is not a PNG, JPEG, GIF or WebP picture.".to_string());
        };

        Ok(Self { format, bytes })
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(extra: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(std::iter::repeat_n(0u8, extra));
        bytes
    }

    #[test]
    fn format_comes_from_the_bytes_not_the_extension() {
        assert_eq!(ImageFormat::detect(&png(4)), Some(ImageFormat::Png));
        assert_eq!(
            ImageFormat::detect(b"\xff\xd8\xff\xe0\x00\x10JFIF"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(ImageFormat::detect(b"GIF89a...."), Some(ImageFormat::Gif));
        assert_eq!(
            ImageFormat::detect(b"RIFF\x24\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::Webp)
        );

        // The two shapes that must never reach a bucket that is served to browsers.
        assert_eq!(ImageFormat::detect(b"<svg onload=alert(1)>"), None);
        assert_eq!(ImageFormat::detect(b"<!DOCTYPE html><html>"), None);
        assert_eq!(ImageFormat::detect(b"RIFF\x24\x00\x00\x00WAVEfmt "), None);
        assert_eq!(ImageFormat::detect(b""), None);
    }

    #[test]
    fn mime_and_extension_agree_on_the_format() {
        assert_eq!(ImageFormat::Png.mime(), "image/png");
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
    }

    #[test]
    fn parse_accepts_a_picture_and_keeps_its_bytes() {
        let upload = ImageUpload::parse(png(16)).expect("a PNG is a picture");
        assert_eq!(upload.format(), ImageFormat::Png);
        assert_eq!(upload.bytes().len(), 24);
    }

    #[test]
    fn parse_refuses_empty_oversized_and_non_pictures() {
        assert!(ImageUpload::parse(Vec::new()).is_err());
        assert!(ImageUpload::parse(b"just some text".to_vec()).is_err());

        let too_big = png(ImageUpload::MAX_BYTES);
        let message = ImageUpload::parse(too_big).expect_err("over the size limit");
        assert!(message.contains("5 MB"), "{message}");
    }
}
