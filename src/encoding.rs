use serde::{Deserialize, Serialize};

/// Text encodings offered via the "Encoding" menu, mirroring Notepad++'s common set.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl TextEncoding {
    pub const ALL: [TextEncoding; 5] = [
        TextEncoding::Utf8,
        TextEncoding::Utf8Bom,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
        TextEncoding::Windows1252,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8-BOM",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
            TextEncoding::Windows1252 => "ANSI (Windows-1252)",
        }
    }

    /// Sniffs a BOM (or assumes UTF-8) and decodes `bytes` into a `String`.
    pub fn decode(bytes: &[u8]) -> (String, TextEncoding) {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            let (text, _, _) = encoding_rs::UTF_8.decode(&bytes[3..]);
            return (text.into_owned(), TextEncoding::Utf8Bom);
        }
        if bytes.starts_with(&[0xFF, 0xFE]) {
            let (text, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
            return (text.into_owned(), TextEncoding::Utf16Le);
        }
        if bytes.starts_with(&[0xFE, 0xFF]) {
            let (text, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
            return (text.into_owned(), TextEncoding::Utf16Be);
        }

        // No BOM: try strict UTF-8 first, fall back to Windows-1252 (ANSI) which never fails.
        match std::str::from_utf8(bytes) {
            Ok(text) => (text.to_string(), TextEncoding::Utf8),
            Err(_) => {
                let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
                (text.into_owned(), TextEncoding::Windows1252)
            }
        }
    }

    /// Decodes `bytes` using exactly *this* encoding - no auto-detection or fallback -
    /// used when the user explicitly picks an encoding from the "Encoding" menu to
    /// reinterpret a file's on-disk bytes, as opposed to `decode` (used when first
    /// opening a file), which auto-detects the encoding from a BOM or content. A leading
    /// BOM matching this encoding is stripped if present, same as `decode` does.
    pub fn decode_with(self, bytes: &[u8]) -> String {
        match self {
            TextEncoding::Utf8 | TextEncoding::Utf8Bom => {
                let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
                String::from_utf8_lossy(bytes).into_owned()
            }
            TextEncoding::Utf16Le => {
                let bytes = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
                let (text, _, _) = encoding_rs::UTF_16LE.decode(bytes);
                text.into_owned()
            }
            TextEncoding::Utf16Be => {
                let bytes = bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes);
                let (text, _, _) = encoding_rs::UTF_16BE.decode(bytes);
                text.into_owned()
            }
            TextEncoding::Windows1252 => {
                let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
                text.into_owned()
            }
        }
    }

    /// Encodes `text` back into bytes for saving, including a BOM where applicable.
    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            TextEncoding::Utf8 => text.as_bytes().to_vec(),
            TextEncoding::Utf8Bom => {
                let mut out = vec![0xEF, 0xBB, 0xBF];
                out.extend_from_slice(text.as_bytes());
                out
            }
            TextEncoding::Utf16Le => {
                let mut out = vec![0xFF, 0xFE];
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                out
            }
            TextEncoding::Utf16Be => {
                let mut out = vec![0xFE, 0xFF];
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
                out
            }
            TextEncoding::Windows1252 => {
                let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(text);
                bytes.into_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TextEncoding;

    #[test]
    fn decode_with_strips_matching_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello".as_bytes());
        assert_eq!(TextEncoding::Utf8.decode_with(&bytes), "hello");
    }

    #[test]
    fn decode_with_round_trips_each_encoding() {
        let text = "hello world";
        for encoding in TextEncoding::ALL {
            let bytes = encoding.encode(text);
            assert_eq!(
                encoding.decode_with(&bytes),
                text,
                "round-trip failed for {}",
                encoding.label()
            );
        }
    }

    #[test]
    fn decode_with_windows_1252_handles_high_bytes() {
        // 0xE9 is 'é' in Windows-1252, not valid standalone UTF-8.
        let bytes = [0x63, 0x61, 0x66, 0xE9]; // "caf" + é
        assert_eq!(TextEncoding::Windows1252.decode_with(&bytes), "café");
    }
}
