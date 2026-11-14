//! Error types for LiteRT-LM Rust bindings.

use thiserror::Error;

use crate::ffi;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the LiteRT-LM Rust bindings.
#[derive(Debug, Error)]
pub enum Error {
    #[error("native call `{0}` returned a null pointer")]
    NullPointer(&'static str),

    #[error("native call `{op}` failed with status {status}: {message}")]
    NativeStatus {
        op: &'static str,
        status: u32,
        message: String,
    },

    #[error("invalid model path (not valid UTF-8 or contains interior NUL)")]
    ModelPath,

    #[error("NUL byte in C string: {0}")]
    Nul(#[from] std::ffi::NulError),

    #[error("invalid UTF-8 from native library: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("index {0} out of range (len={1})")]
    IndexOutOfRange(usize, usize),

    #[error("{0}")]
    Message(String),
}

/// Message from the thread-local native error reporter, or empty if none.
pub(crate) fn last_error_message() -> String {
    unsafe {
        let ptr = ffi::litert_lm_get_last_error_message();
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

/// Convert a `LiteRtLmStatusCode` into `Result<()>`, capturing the native
/// error message on failure.
pub(crate) fn check(op: &'static str, status: ffi::LiteRtLmStatusCode) -> Result<()> {
    if status == ffi::LiteRtLmStatusCode_kLiteRtLmStatusOk {
        Ok(())
    } else {
        Err(Error::NativeStatus {
            op,
            status,
            message: last_error_message(),
        })
    }
}
