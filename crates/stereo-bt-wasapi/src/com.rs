use std::marker::PhantomData;

use crate::Error;

/// Keeps COM initialised in the multithreaded apartment on the current thread.
///
/// Create one at the start of every thread that touches WASAPI and keep it
/// alive until the thread no longer uses any WASAPI object. Not `Send`: COM
/// initialisation is per thread.
///
/// Fails with `RPC_E_CHANGED_MODE` on a thread already initialised as a
/// single-threaded apartment (such as a GUI thread); use a dedicated thread.
#[derive(Debug)]
pub struct ComGuard {
    _not_send: PhantomData<*const ()>,
}

impl ComGuard {
    /// Initialises COM (MTA) for the current thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Audio`] when COM cannot be initialised.
    pub fn init_mta() -> Result<Self, Error> {
        wasapi::initialize_mta()
            .ok()
            .map_err(wasapi::WasapiError::from)?;
        Ok(Self {
            _not_send: PhantomData,
        })
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}
