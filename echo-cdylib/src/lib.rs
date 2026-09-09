//! Echo plugin — cdylib form.
//!
//! Implements a minimal request/response C-ABI. The host dlopens this
//! `.so` / `.dylib` at runtime, finds the `ECHO_EXPORTS` symbol, and
//! wraps the exports into a cordis Plugin that registers the capability
//! with the Dispatcher.
//!
//! ## Contract
//!
//! All functions are `extern "C"`, infallible from the ABI perspective;
//! errors are reported through the return code. ABI version mismatch is
//! the host's responsibility (it must read `abi_version` before calling).
//!
//! Buffers are caller-owned. The plugin never retains pointers past a
//! single call.

use std::ptr;

/// Bumped on incompatible contract changes. Host rejects mismatches.
pub const ECHO_ABI_VERSION: u32 = 1;

/// What the host sees.
///
/// `name` writes the plugin's display name into a caller-provided buffer.
///
/// `invoke` runs the plugin's logic on raw input bytes and writes raw
/// output bytes back. Returns 0 on success; non-zero is a plugin-defined
/// error code (1 = output buffer too small, 2 = invalid input).
#[repr(C)]
pub struct EchoExports {
    pub abi_version: u32,
    pub name: unsafe extern "C" fn(out_buf: *mut u8, cap: usize, out_len: *mut usize),
    pub invoke: unsafe extern "C" fn(
        in_buf: *const u8,
        in_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_len: *mut usize,
    ) -> i32,
}

// ---------------------------------------------------------------------------
// Implementation.
// ---------------------------------------------------------------------------

const PLUGIN_NAME: &[u8] = b"echo-cdylib";

unsafe extern "C" fn name_impl(out_buf: *mut u8, cap: usize, out_len: *mut usize) {
    // SAFETY: caller guarantees out_buf is valid for `cap` bytes when
    // out_len is non-null. We never read out_buf; only write.
    unsafe {
        if PLUGIN_NAME.len() > cap {
            *out_len = 0;
            return;
        }
        ptr::copy_nonoverlapping(PLUGIN_NAME.as_ptr(), out_buf, PLUGIN_NAME.len());
        *out_len = PLUGIN_NAME.len();
    }
}

unsafe extern "C" fn invoke_impl(
    in_buf: *const u8,
    in_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: caller guarantees in_buf is valid for in_len bytes and
    // out_buf is valid for out_cap bytes.
    unsafe {
        if in_len > out_cap {
            return 1; // ECHO_ERR_SMALL_BUFFER
        }
        if in_buf.is_null() && in_len > 0 {
            return 2; // ECHO_ERR_INVALID_INPUT
        }
        ptr::copy_nonoverlapping(in_buf, out_buf, in_len);
        *out_len = in_len;
        0
    }
}

// ---------------------------------------------------------------------------
// The exported symbol the host looks up.
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub static ECHO_EXPORTS: EchoExports = EchoExports {
    abi_version: ECHO_ABI_VERSION,
    name: name_impl,
    invoke: invoke_impl,
};
