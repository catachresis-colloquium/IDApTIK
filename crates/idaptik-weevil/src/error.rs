//! Errors the adapter can produce, kept total and non-panicking.
//!
//! The kernel answers every misuse with a status code; the adapter turns those
//! codes into typed errors and adds the host-side checks the kernel cannot make
//! (entity lifetime, tick monotonicity, enum ranges). No path here panics: a
//! misbehaving host gets a `Result`, never an abort.

use core::fmt;

/// Everything that can go wrong at the seam, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeevilError {
    /// [`crate::WeevilHost::tick`] was called before [`crate::WeevilHost::init`].
    NotInitialized,
    /// The kernel refused the init descriptor. Carries the kernel's status name.
    Init(String),
    /// The kernel's major version differs from the host's.
    VersionMismatch { host: u32, kernel: u32 },
    /// The layout fingerprint differs: the structs would be mis-read.
    FingerprintMismatch { host: u64, kernel: u64 },
    /// The kernel declined a capability the host declared.
    CapabilityUnsupported(u32),
    /// The host's `ee_init_desc` was smaller than the kernel expects.
    BufferTooSmall,
    /// A null or otherwise unusable pointer was passed across the seam.
    BadParam,
    /// The kernel caught a panic internally; treat the run as untrusted.
    PanicCaught,
    /// The observation named an agent id the host never allocated.
    AgentNotFound(u32),
    /// An agent or contact id did not fit the ABI's `u16` id field.
    IdOutOfRange { kind: &'static str, id: u32 },
    /// More agents were requested than the context was initialised for.
    TooManyAgents { requested: u32, max: u32 },
    /// The observation carried more contacts than the ABI's fixed array.
    TooManyContacts { count: usize, max: usize },
    /// The intent's action code is outside the ABI's defined range.
    InvalidAction(u32),
    /// The intent named a target id no live entity owns.
    InvalidTarget(u32),
    /// The intent's speed was outside the host's allowed band.
    InvalidSpeed { speed: i32, min: i32, max: i32 },
    /// The observation's tick was not the one the agent last processed + 1.
    StaleTick { expected: u64, got: u64 },
    /// The kernel returned a status code this adapter does not recognise.
    UnknownStatus(u32),
    /// A profile document could not be parsed.
    ProfileParse(String),
    /// A replay document could not be read back.
    ReplayParse(String),
}

impl fmt::Display for WeevilError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "weevil: host not initialised"),
            Self::Init(s) => write!(f, "weevil: ee_init refused ({s})"),
            Self::VersionMismatch { host, kernel } => {
                write!(
                    f,
                    "weevil: ABI major mismatch (host {host}, kernel {kernel})"
                )
            }
            Self::FingerprintMismatch { host, kernel } => write!(
                f,
                "weevil: layout fingerprint mismatch (host {host}, kernel {kernel})"
            ),
            Self::CapabilityUnsupported(mask) => {
                write!(
                    f,
                    "weevil: capability 0x{mask:08x} not supported by the kernel"
                )
            }
            Self::BufferTooSmall => write!(f, "weevil: init descriptor too small"),
            Self::BadParam => write!(f, "weevil: bad parameter across the seam"),
            Self::PanicCaught => write!(f, "weevil: kernel reported a caught panic"),
            Self::AgentNotFound(id) => write!(f, "weevil: unknown agent id {id}"),
            Self::IdOutOfRange { kind, id } => {
                write!(
                    f,
                    "weevil: {kind} id {id} does not fit the ABI's u16 id field"
                )
            }
            Self::TooManyAgents { requested, max } => {
                write!(
                    f,
                    "weevil: {requested} agents requested, context holds {max}"
                )
            }
            Self::TooManyContacts { count, max } => {
                write!(f, "weevil: {count} contacts, ABI holds {max}")
            }
            Self::InvalidAction(a) => write!(f, "weevil: intent action {a} out of range"),
            Self::InvalidTarget(t) => write!(f, "weevil: intent target {t} is not a live entity"),
            Self::InvalidSpeed { speed, min, max } => {
                write!(f, "weevil: intent speed {speed} outside [{min}, {max}]")
            }
            Self::StaleTick { expected, got } => {
                write!(f, "weevil: stale tick (expected {expected}, got {got})")
            }
            Self::UnknownStatus(s) => write!(f, "weevil: unknown status code {s}"),
            Self::ProfileParse(msg) => write!(f, "weevil: malformed profile: {msg}"),
            Self::ReplayParse(msg) => write!(f, "weevil: malformed replay: {msg}"),
        }
    }
}

impl std::error::Error for WeevilError {}

/// The adapter's result type.
pub type Result<T> = core::result::Result<T, WeevilError>;
