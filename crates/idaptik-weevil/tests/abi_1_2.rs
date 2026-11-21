//! ABI 1.2: the model of the target's knowledge (evil-weevil ADR-0014),
//! against the linked kernel.
//!
//! The layout is pinned at compile time in `src/abi.rs` and against the
//! generated header by `tests/abi_mirror.rs`. This test checks what only
//! the linked kernel can answer:
//!
//! * it is the 1.2 kernel, with the 1.0 and 1.1 fingerprints unchanged and the
//!   new model fingerprint;
//! * a host that passes the 32-byte 1.1 record gets exactly 32 bytes written,
//!   with the capability declared and a cue given, and nothing past them;
//! * through [`WeevilHost::tick_model`], a cue exposes the guard with cause
//!   `CUE`, and without `CAP_OBSERVATION_CUES` the same cue changes nothing;
//! * the host refuses a cue from a contact the guard does not perceive.

use idaptik_weevil::abi;
use idaptik_weevil::fx::to_fx;
use idaptik_weevil::host::WeevilHost;
use idaptik_weevil::{Contact, ContactSource, Fx2, Observation, WeevilError};

const TARGET: u32 = 41;

fn observation(tick: u64, caps: u32) -> Observation {
    Observation {
        agent_id: 0,
        tick,
        self_pos: Fx2::ZERO,
        self_vel: Fx2::ZERO,
        health: abi::FX_ONE,
        ammo: 3 * abi::FX_ONE,
        capability_mask: caps,
        contacts: vec![Contact {
            id: TARGET,
            source: ContactSource::Player,
            bearing: Fx2 {
                x: abi::FX_ONE,
                y: 0,
            },
            distance: to_fx(3.0),
            threat: abi::FX_ONE / 2,
            confidence: 1.0,
            age_ticks: 0,
        }],
        cooldown: 0,
    }
}

#[test]
fn the_linked_kernel_is_abi_1_2_with_every_fingerprint_pinned() {
    let host = WeevilHost::init(abi::CAP_NAVMESH, 1, 4096, 7, 60, 1).expect("init");
    let v = host.kernel_version();
    assert_eq!((u32::from(v.major), u32::from(v.minor)), (1, 2));
    assert_eq!((abi::ABI_MAJOR, abi::ABI_MINOR), (1, 2));
    assert_eq!(host.kernel_fingerprint(), 18_276_775);
    assert_eq!(host.mind_fingerprint(), 1_047_490);
    assert_eq!(abi::MODEL_FINGERPRINT, 69_857);
    assert_eq!(host.model_fingerprint(), abi::MODEL_FINGERPRINT);
}

#[test]
fn a_32_byte_record_is_written_as_1_1_and_nothing_past_it() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
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
    // SAFETY: owned, aligned values that outlive each call.
    assert_eq!(unsafe { abi::ee_init(&desc, &mut ctx) }, abi::STATUS_OK);
    let snap = abi::EeSnapshot {
        tick: 1,
        capability_mask: caps,
        contact_count: 1,
        health: abi::FX_ONE,
        ammo: 3 * abi::FX_ONE,
        contact0: abi::EeContact {
            id: TARGET as u16,
            flags: abi::CONTACT_FLAG_OBSERVES,
            bearing_cos: 16384,
            bearing_sin: 8192,
            distance: to_fx(3.0),
            threat: abi::FX_ONE / 2,
        },
        ..Default::default()
    };
    let mut agent = abi::EeAgent::default();
    let mut mind = abi::EeMind {
        struct_size: core::mem::size_of::<abi::EeMind>() as u32,
        ..Default::default()
    };
    let mut out = abi::EeIntent::default();
    // A canary in bytes 32 to 39, behind a record that says it is 32 long.
    let canary = abi::EeModelTrace {
        mind: abi::EeMindTrace {
            struct_size: core::mem::size_of::<abi::EeMindTrace>() as u32,
            ..Default::default()
        },
        exposure_contact: 0xBEEF,
        exposure: 0xA5,
        exposure_prev: 0x5A,
        exposure_cause: 0xC3,
        reserved0: 0x3C,
        reserved1: 0xF00D,
    };
    let mut model = canary;
    // SAFETY: as above; `model.mind` is the first member of a live, aligned
    // 40-byte value, and the kernel is told it is 32 bytes long.
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
        (0xBEEF, 0xA5, 0x5A, 0xC3, 0x3C, 0xF00D),
        "nothing past byte 32 is touched"
    );
}

#[test]
fn a_cue_exposes_the_guard_with_its_cause_and_its_observer() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
    let mut host = WeevilHost::init(caps, 1, 4096, 7, 60, 1).expect("init");
    let quiet = host
        .tick_model(&observation(1, caps), None, &[])
        .expect("tick 1");
    assert!(!quiet.model.exposure && !quiet.model.exposure_prev);
    assert_eq!(quiet.model.cause, abi::EXPOSURE_CAUSE_NONE);
    let cued = host
        .tick_model(&observation(2, caps), None, &[TARGET])
        .expect("tick 2");
    assert!(cued.model.exposure && !cued.model.exposure_prev);
    assert_eq!(cued.model.cause, abi::EXPOSURE_CAUSE_CUE);
    assert_eq!(u32::from(cued.model.contact), TARGET);
    assert!(
        cued.agent.flags & abi::AGENT_FLAG_EXPOSED != 0,
        "the belief lives in ee_agent.flags bit 4"
    );
}

#[test]
fn without_the_capability_the_same_cue_changes_nothing() {
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT;
    let mut host = WeevilHost::init(caps, 1, 4096, 7, 60, 1).expect("init");
    for tick in 1..=3 {
        let m = host
            .tick_model(&observation(tick, caps), None, &[TARGET])
            .expect("a tick");
        assert!(!m.model.exposure && !m.model.exposure_prev);
        assert_eq!(m.model.cause, abi::EXPOSURE_CAUSE_NONE);
        assert_eq!(m.record.tells & abi::TELL_CAUGHT, 0);
        assert!(!m.record.ranking.contains(&idaptik_weevil::host::MODE_STALK));
        assert_eq!(m.agent.flags & abi::AGENT_FLAG_EXPOSED, 0);
    }
}

#[test]
fn a_cue_from_a_contact_the_guard_does_not_perceive_is_refused() {
    let caps = abi::CAP_NAVMESH | abi::CAP_OBSERVATION_CUES;
    let mut host = WeevilHost::init(caps, 1, 4096, 7, 60, 1).expect("init");
    let err = host
        .tick_model(&observation(1, caps), None, &[TARGET + 1])
        .unwrap_err();
    assert!(matches!(err, WeevilError::InvalidTarget(id) if id == TARGET + 1));
}
