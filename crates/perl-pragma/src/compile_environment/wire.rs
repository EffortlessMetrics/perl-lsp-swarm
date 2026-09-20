use super::SchemaError;
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

/// Maximum canonical state/transition snapshot bytes.
pub const MAX_SNAPSHOT_BYTES: usize = 1_048_576;
/// Maximum aggregate bundle wire bytes, including whitespace.
pub const MAX_BUNDLE_BYTES: usize = 16_777_216;
/// Maximum JSON nesting depth before deserialization.
pub const MAX_DEPTH: usize = 32;

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("snapshot byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, SchemaError> {
    let mut writer = BoundedWriter { bytes: Vec::new(), limit, exceeded: false };
    let result = serde_json::to_writer(&mut writer, value);
    if writer.exceeded {
        return Err(SchemaError::Limited("snapshot bytes"));
    }
    result.map_err(|error| SchemaError::Instrument(error.to_string()))?;
    Ok(writer.bytes)
}
pub(super) fn canonical(
    domain: &str,
    value: &impl Serialize,
    limit: usize,
) -> Result<Vec<u8>, SchemaError> {
    encode(&(domain, value), limit)
}
pub(super) fn read<T: DeserializeOwned>(
    mut reader: impl Read,
    limit: usize,
) -> Result<T, SchemaError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = limit.saturating_add(1).saturating_sub(bytes.len());
        let request = remaining.min(chunk.len());
        if request == 0 {
            return Err(SchemaError::Limited("wire bytes"));
        }
        let buffer = chunk
            .get_mut(..request)
            .ok_or_else(|| SchemaError::Instrument("read buffer".into()))?;
        let length =
            reader.read(buffer).map_err(|error| SchemaError::Instrument(error.to_string()))?;
        if length == 0 {
            break;
        }
        let part = buffer
            .get(..length)
            .ok_or_else(|| SchemaError::Instrument("reader exceeded buffer".into()))?;
        if length > limit.saturating_sub(bytes.len()) {
            return Err(SchemaError::Limited("wire bytes"));
        }
        bytes.extend_from_slice(part);
    }
    check_depth(&bytes)?;
    serde_json::from_slice(&bytes).map_err(|error| SchemaError::Invalid(error.to_string()))
}
fn check_depth(bytes: &[u8]) -> Result<(), SchemaError> {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for byte in bytes {
        if string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > MAX_DEPTH {
                        return Err(SchemaError::Limited("JSON depth"));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}
