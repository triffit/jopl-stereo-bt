//! Real-time thread scheduling through the Multimedia Class Scheduler Service.

// Two `windows` crate calls with simple handle ownership; see SAFETY notes.
#![allow(unsafe_code)]

use std::marker::PhantomData;

use wasapi::WasapiError;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
};
use windows::core::w;

use crate::Error;

/// Keeps the current thread registered with MMCSS "Pro Audio" priority.
/// Reverts on drop. Not `Send`: the registration belongs to this thread.
#[derive(Debug)]
pub struct MmcssGuard {
    handle: HANDLE,
    _not_send: PhantomData<*const ()>,
}

impl MmcssGuard {
    /// Registers the current thread as a "Pro Audio" task.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when MMCSS is unavailable; callers may continue
    /// at normal priority.
    pub fn pro_audio() -> Result<Self, Error> {
        let mut task_index = 0u32;
        // SAFETY: `w!` yields a static NUL-terminated UTF-16 string and
        // `task_index` is a valid, exclusively borrowed out-parameter.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &raw mut task_index) }
            .map_err(WasapiError::from)?;
        Ok(Self {
            handle,
            _not_send: PhantomData,
        })
    }
}

impl Drop for MmcssGuard {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful `AvSetMmThreadCharacteristicsW`
        // on this same thread (the guard is not `Send`) and is reverted once.
        // A failed revert only leaves the exiting thread at higher priority.
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.handle) };
    }
}
