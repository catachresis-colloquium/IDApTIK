//! ABI 1.2: the model trace, against the linked kernel (evil-weevil ADR-0014).
//!
//! The layout is pinned at compile time in `src/abi.rs` and against the
//! generated header by `tools/verify_abi_mirror.py`. These tests check what
//! only the linked kernel can answer: that it is the 1.2 kernel this mirror
//! was written for, that a 1.1-sized trace is written exactly as at 1.1 even
//! with the capability declared and a cue given, and that a 1.2-sized trace
//! carries the exposure belief with its cause.

use idaptik_weevil::abi;
use idaptik_weevil::host::WeevilHost;

#[test]
fn the_linked_kernel_is_abi_1_2_with_the_pinned_model_fingerprint() {
    let host = WeevilHost::init(abi::CAP_NAVMESH, 1, 4096, 7, 60, 1).expect("init");
    let v = host.kernel_version();
    assert_eq!(
        (v.major as u32, v.minor as u32),
        (abi::ABI_MAJOR, abi::ABI_MINOR)
    );
    assert_eq!(abi::ABI_MINOR, 2);
    assert_eq!(host.kernel_fingerprint(), abi::ABI_FINGERPRINT);
    assert_eq!(host.mind_fingerprint(), abi::MIND_FINGERPRINT);
    assert_eq!(host.model_fingerprint(), abi::MODEL_FINGERPRINT);
    assert_eq!(abi::MODEL_FINGERPRINT, 69_857);
}

fn context(caps: u32) -> abi::EeContext {
    let mut ctx = abi::EeContext::default();
    let desc = abi::EeInitDesc {
        struct_size: core::mem::size_of::<abi::EeInitDesc>() as u32,
        abi_major: abi::ABI_MAJOR,
        abi_minor: abi::ABI_MINOR,
        capabilities: caps,
        max_agents: 1,
        flags: 0,
        host_tick_num: 60,
        host_tick_den: 1,
        rng_seed: 7,
        abi_fingerprint: abi::ABI_FINGERPRINT,
        reserved0: 0,
    };
    // SAFETY: owned, aligned values that outlive the call.
    assert_eq!(unsafe { abi::ee_init(&desc, &mut ctx) }, abi::STATUS_OK);
    ctx
}

/// One contact, id 41, at 1.5 fx, observing the agent.
fn cued_snapshot(tick: u64, caps: u32) -> abi::EeSnapshot {
    abi::EeSnapshot {
        tick,
        capability_mask: caps,
        contact_count: 1,
        health: abi::FX_ONE,
        ammo: 3 * abi::FX_ONE,
        contact0: abi::EeContact {
            id: 41,
            flags: abi::CONTACT_FLAG_OBSERVES,
            bearing_cos: 16384,
            bearing_sin: 8192,
            distance: abi::FX_ONE + abi::FX_ONE / 2,
            threat: abi::FX_ONE / 2,
        },
        ..Default::default()
    }
}

fn fresh_mind() -> abi::EeMind {
    abi::EeMind {
        struct_size: core::mem::size_of::<abi::EeMind>() as u32,
        ..Default::default()
    }
}

#[test]
fn a_1_1_sized_trace_is_written_as_at_1_1_and_nothing_past_it() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
    let mut ctx = context(caps);
    let snap = cued_snapshot(1, caps);
    let mut agent = abi::EeAgent::default();
    let mut mind = fresh_mind();
    let mut out = abi::EeIntent::default();
    // A 1.2 record with a canary past the 1.1 prefix, passed at the 1.1 size.
    let mut model = abi::EeModelTrace {
        exposure_contact: 0xBEEF,
        exposure: 0xA5,
        exposure_prev: 0x5A,
        exposure_cause: 0xC3,
        reserved0: 0x3C,
        reserved1: 0xF00D,
        ..Default::default()
    };
    model.mind.struct_size = core::mem::size_of::<abi::EeMindTrace>() as u32;
    let canary = model;
    // SAFETY: owned, aligned values; `model.mind` is a valid 32-byte record.
    let status = unsafe {
        abi::ee_tick_mind(
            &mut ctx,
            &snap,
            &mut agent,
            &mut mind,
            &mut out,
            &mut model.mind,
        )
    };
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(model.mind.struct_size, 32);
    assert_eq!(model.mind.tick, 1, "the 1.1 record was written");
    assert_eq!(
        (
            model.exposure_contact,
            model.exposure,
            model.exposure_prev,
            model.exposure_cause,
            model.reserved0,
            model.reserved1
        ),
        (
            canary.exposure_contact,
            canary.exposure,
            canary.exposure_prev,
            canary.exposure_cause,
            canary.reserved0,
            canary.reserved1
        ),
        "nothing past byte 32 is touched"
    );
}

#[test]
fn a_1_2_sized_trace_carries_the_cue_as_the_cause_of_exposure() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
    let mut ctx = context(caps);
    let snap = cued_snapshot(1, caps);
    let mut agent = abi::EeAgent::default();
    let mut mind = fresh_mind();
    let mut out = abi::EeIntent::default();
    let mut model = abi::EeModelTrace::default();
    model.mind.struct_size = core::mem::size_of::<abi::EeModelTrace>() as u32;
    // SAFETY: owned, aligned values; the size says a whole 40-byte record.
    let status = unsafe {
        abi::ee_tick_mind(
            &mut ctx,
            &snap,
            &mut agent,
            &mut mind,
            &mut out,
            &mut model.mind,
        )
    };
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(
        (
            model.exposure_prev,
            model.exposure,
            model.exposure_cause,
            model.exposure_contact
        ),
        (0, 1, abi::EXPOSURE_CAUSE_CUE, 41)
    );
    assert_ne!(agent.flags & abi::AGENT_FLAG_EXPOSED, 0);
}

#[test]
fn without_the_capability_the_cue_is_ignored() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT;
    let mut ctx = context(caps);
    let snap = cued_snapshot(1, caps);
    let mut agent = abi::EeAgent::default();
    let mut mind = fresh_mind();
    let mut out = abi::EeIntent::default();
    let mut model = abi::EeModelTrace::default();
    model.mind.struct_size = core::mem::size_of::<abi::EeModelTrace>() as u32;
    // SAFETY: as above.
    let status = unsafe {
        abi::ee_tick_mind(
            &mut ctx,
            &snap,
            &mut agent,
            &mut mind,
            &mut out,
            &mut model.mind,
        )
    };
    assert_eq!(status, abi::STATUS_OK);
    assert_eq!(
        (model.exposure, model.exposure_cause),
        (0, abi::EXPOSURE_CAUSE_NONE)
    );
    assert_eq!(agent.flags & abi::AGENT_FLAG_EXPOSED, 0);
}
