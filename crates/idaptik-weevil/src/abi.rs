//! The Evil Weevil C ABI, mirrored from the *generated* header.
//!
//! `include/evil_weevil/ee.h` is generated from the Idris2 model
//! (`src/interface/Abi/Types.idr`) and is the single source of truth for the
//! wire format. This module is a faithful `#[repr(C)]` transcription of that
//! header plus compile-time assertions that the transcription still matches the
//! declared sizes and offsets. It is **not** a reimplementation: every entry
//! point here is the kernel's own exported symbol, resolved at link time.
//!
//! Two rules from ADR-0005 are load-bearing for this mirror:
//!
//! * No pointer-sized field crosses the ABI. Pointers are function parameters
//!   only, so the layout is identical on every 64-bit target and host addresses
//!   never enter deterministic state.
//! * Field order is descending by alignment with explicit pads, so the declared
//!   offsets equal C's natural layout — which the header's own
//!   `_Static_assert`s verify per compiler, and which the const assertions below
//!   re-check from the Rust side.
//!
//! 32-bit targets, including `wasm32`, are **not** supported by this layout
//! (the header says so explicitly); a distinct, separately-proved ABI is Phase 4
//! work. Do not claim otherwise.

#![allow(non_camel_case_types)]

use core::mem::{offset_of, size_of};

// ── ABI identity ────────────────────────────────────────────────────────────

/// ABI major version this adapter was written against.
pub const ABI_MAJOR: u32 = 1;
/// ABI minor version this adapter was written against.
pub const ABI_MINOR: u32 = 0;
/// The layout fingerprint the kernel pins. A host that links a kernel with a
/// different fingerprint must refuse to run rather than mis-read the structs.
pub const ABI_FINGERPRINT: u64 = 18276775;

// ── Fixed point (Q16.16) ────────────────────────────────────────────────────

/// Fraction bits in `ee_fx`.
pub const FX_FRACTION_BITS: u32 = 16;
/// The value `1.0` in Q16.16.
pub const FX_ONE: i32 = 65536;

// ── Capabilities (bit masks, not indices) ───────────────────────────────────

pub const CAP_NAVMESH: u32 = 1u32 << 0;
pub const CAP_LINE_OF_SIGHT: u32 = 1u32 << 1;
pub const CAP_RAYCAST: u32 = 1u32 << 2;
pub const CAP_WAYPOINT_GRAPH: u32 = 1u32 << 3;
pub const CAP_SPAWN_RIGHTS: u32 = 1u32 << 4;
pub const CAP_ANIMATION_HOOKS: u32 = 1u32 << 5;
pub const CAP_DAMAGE_EVENTS: u32 = 1u32 << 6;
pub const CAP_SOUND_EVENTS: u32 = 1u32 << 7;
pub const CAP_VEHICLE_BOARDING: u32 = 1u32 << 8;
pub const CAP_TERRAIN_HEIGHT: u32 = 1u32 << 9;

// ── Status codes ────────────────────────────────────────────────────────────

pub const STATUS_OK: u32 = 0;
pub const STATUS_BAD_PARAM: u32 = 1;
pub const STATUS_VERSION_MISMATCH: u32 = 2;
pub const STATUS_FINGERPRINT_MISMATCH: u32 = 3;
pub const STATUS_CAPABILITY_UNSUPPORTED: u32 = 4;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 5;
pub const STATUS_PANIC_CAUGHT: u32 = 6;

/// Every entry point's return value.
pub type EeStatus = u32;

// ── Actions ────────────────────────────────────────────────────────────────

pub const ACTION_IDLE: u32 = 0;
pub const ACTION_ADVANCE: u32 = 1;
pub const ACTION_HOLD: u32 = 2;
pub const ACTION_FLEE: u32 = 3;
pub const ACTION_ATTACK: u32 = 4;
pub const ACTION_RELOAD: u32 = 5;
pub const ACTION_TAKE_COVER: u32 = 6;
pub const ACTION_REGROUP: u32 = 7;
pub const ACTION_INVESTIGATE: u32 = 8;

/// The largest action code the ABI currently defines.
pub const ACTION_MAX: u32 = ACTION_INVESTIGATE;

// ── Intent flags ───────────────────────────────────────────────────────────

pub const INTENT_FLAG_DEGRADED: u32 = 1u32 << 0;
pub const INTENT_FLAG_NEW_TARGET: u32 = 1u32 << 1;
pub const INTENT_FLAG_UNREACHABLE: u32 = 1u32 << 2;
pub const INTENT_FLAG_FROM_MEMORY: u32 = 1u32 << 3;

// ── Agent flags ─────────────────────────────────────────────────────────────

pub const AGENT_FLAG_MEMORY_VALID: u32 = 1u32 << 0;
pub const AGENT_MODE_SHIFT: u32 = 1;
pub const AGENT_MODE_MASK: u32 = 14;
pub const AGENT_MODE_ADVANCE: u32 = 0;
pub const AGENT_MODE_ENGAGE: u32 = 1;
pub const AGENT_MODE_EVADE: u32 = 2;
pub const AGENT_MODE_RELOAD: u32 = 3;
pub const AGENT_MODE_INVESTIGATE: u32 = 4;

/// Ticks a sighting stays actionable before the agent forgets it.
pub const MEMORY_TTL: u32 = 36;

/// The kernel examines at most this many contacts per snapshot.
pub const MAX_CONTACTS: usize = 8;

// ── Structs ────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub reserved: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeInitDesc {
    pub struct_size: u32,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub capabilities: u32,
    pub max_agents: u32,
    pub flags: u32,
    pub host_tick_num: u64,
    pub host_tick_den: u64,
    pub rng_seed: u64,
    pub abi_fingerprint: u64,
    pub reserved0: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeContact {
    pub id: u16,
    pub flags: u16,
    pub bearing_cos: u16,
    pub bearing_sin: u16,
    pub distance: i32,
    pub threat: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeSnapshot {
    pub tick: u64,
    pub capability_mask: u32,
    pub agent_id: u16,
    pub contact_count: u16,
    pub self_x: i32,
    pub self_y: i32,
    pub self_z: i32,
    pub self_vx: i32,
    pub self_vy: i32,
    pub health: i32,
    pub ammo: i32,
    pub flags: u32,
    pub contact0: EeContact,
    pub contact1: EeContact,
    pub contact2: EeContact,
    pub contact3: EeContact,
    pub contact4: EeContact,
    pub contact5: EeContact,
    pub contact6: EeContact,
    pub contact7: EeContact,
    pub cooldown: u32,
    pub reserved0: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeAgent {
    pub rng_state: u64,
    pub tick_last: u64,
    pub memory_x: i32,
    pub memory_y: i32,
    pub memory_age: u32,
    pub cached_action: u32,
    pub cached_target: u32,
    pub budget_used: u32,
    pub degraded_ticks: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeIntent {
    pub action: u32,
    pub target_id: u32,
    pub move_x: i32,
    pub move_y: i32,
    pub move_z: i32,
    pub look_x: i32,
    pub look_y: i32,
    pub speed: i32,
    pub priority: u32,
    pub budget_used: u32,
    pub flags: u32,
    pub reserved0: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EeContext {
    pub abi_major: u32,
    pub flags: u32,
    pub capabilities: u32,
    pub budget_units: u32,
    pub max_agents: u32,
    pub reserved0: u32,
    pub abi_fingerprint: u64,
    pub rng_seed: u64,
    pub tick_rate_num: u64,
    pub tick_rate_den: u64,
    pub ticks_run: u64,
    pub degraded_ticks: u64,
}

// ── Compile-time layout proofs ──────────────────────────────────────────────
//
// These mirror the header's `_Static_assert`s. If the kernel's header moves,
// these fire at compile time here too, before any host code runs against a
// stale layout.

const _: () = assert!(size_of::<EeVersion>() == 8, "ee_version size");
const _: () = assert!(size_of::<EeInitDesc>() == 64, "ee_init_desc size");
const _: () = assert!(size_of::<EeContact>() == 16, "ee_contact size");
const _: () = assert!(size_of::<EeSnapshot>() == 184, "ee_snapshot size");
const _: () = assert!(size_of::<EeAgent>() == 48, "ee_agent size");
const _: () = assert!(size_of::<EeIntent>() == 48, "ee_intent size");
const _: () = assert!(size_of::<EeContext>() == 72, "ee_context size");

const _: () = assert!(offset_of!(EeSnapshot, tick) == 0, "snapshot.tick");
const _: () = assert!(offset_of!(EeSnapshot, contact0) == 48, "snapshot.contact0");
const _: () = assert!(offset_of!(EeSnapshot, contact7) == 160, "snapshot.contact7");
const _: () = assert!(offset_of!(EeSnapshot, cooldown) == 176, "snapshot.cooldown");
const _: () = assert!(offset_of!(EeAgent, rng_state) == 0, "agent.rng_state");
const _: () = assert!(offset_of!(EeAgent, flags) == 44, "agent.flags");
const _: () = assert!(offset_of!(EeIntent, action) == 0, "intent.action");
const _: () = assert!(offset_of!(EeIntent, speed) == 28, "intent.speed");
const _: () = assert!(offset_of!(EeContext, abi_major) == 0, "context.abi_major");
const _: () = assert!(offset_of!(EeContext, ticks_run) == 56, "context.ticks_run");

// ── Exported entry points ───────────────────────────────────────────────────
//
// The kernel performs NO allocation: `ee_context` and the `ee_agent` array are
// host-owned storage whose size is fixed by the header. `ee_init` validates
// `struct_size`, the major version and the fingerprint and answers every
// mismatch with a status code rather than throwing. `ee_tick` is pure with
// respect to the host: it reads the snapshot and the agent, writes the agent and
// returns an intent, and never calls back into the host mid-tick.

#[link(name = "evil_weevil")]
extern "C" {
    pub fn ee_init(desc: *const EeInitDesc, ctx: *mut EeContext) -> EeStatus;
    pub fn ee_tick(ctx: *mut EeContext, snap: *const EeSnapshot, agent: *mut EeAgent, out: *mut EeIntent) -> EeStatus;
    pub fn ee_version_info() -> EeVersion;
    pub fn ee_abi_fingerprint() -> u64;
    pub fn ee_status_name(status: EeStatus) -> *const core::ffi::c_char;
}

/// Translate a status code into a static name, without crossing the FFI again
/// for the common paths. Unknown codes get a stable, non-null name so a caller
/// can never dereference a null.
pub fn status_name(status: EeStatus) -> &'static str {
    match status {
        STATUS_OK => "OK",
        STATUS_BAD_PARAM => "BAD_PARAM",
        STATUS_VERSION_MISMATCH => "VERSION_MISMATCH",
        STATUS_FINGERPRINT_MISMATCH => "FINGERPRINT_MISMATCH",
        STATUS_CAPABILITY_UNSUPPORTED => "CAPABILITY_UNSUPPORTED",
        STATUS_BUFFER_TOO_SMALL => "BUFFER_TOO_SMALL",
        STATUS_PANIC_CAUGHT => "PANIC_CAUGHT",
        _ => "UNKNOWN",
    }
}
