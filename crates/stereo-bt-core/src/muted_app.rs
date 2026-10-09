/// An application that is playing on a device but muted for that device in
/// the Windows volume mixer, so its audio reaches neither that device nor,
/// through loopback, any other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutedApp {
    /// Process id of the application.
    pub pid: u32,
    /// Executable name without extension, e.g. `chrome`.
    pub name: String,
}
