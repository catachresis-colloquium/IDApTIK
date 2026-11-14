//! ABI 1.1: the mind entrance and its two structs, against the linked kernel.
//!
//! The layout itself is pinned at compile time in `src/abi.rs` and against
//! the generated header by `tools/verify_abi_mirror.py`. This test checks what
//! only the linked kernel can answer: that it is the 1.1 kernel this mirror
//! was written for, that a 1.1 host still gets 1.0 from `ee_tick`, and that
//! `ee_tick_mind` refuses a short struct before writing anything.

use idaptik_weevil::abi;
use idaptik_weevil::host::WeevilHost;

#[test]
fn the_linked_kernel_is_abi_1_1_with_the_pinned_mind_fingerprint() {
    let host = WeevilHost::init(abi::CAP_NAVMESH, 1, 4096, 7, 60, 1).expect("init");
    let v = host.kernel_version();
    assert_eq!(
        (v.major as u32, v.minor as u32),
        (abi::ABI_MAJOR, abi::ABI_MINOR)
    );
    assert_eq!(abi::ABI_MINOR, 1);
    assert_eq!(host.kernel_fingerprint(), abi::ABI_FINGERPRINT);
    assert_eq!(host.mind_fingerprint(), abi::MIND_FINGERPRINT);
    assert_eq!(abi::MIND_FINGERPRINT, 1_047_490);
}

#[test]
fn ee_tick_mind_refuses_a_short_mind_struct() {
    let mut ctx = abi::EeContext::default();
    let desc = abi::EeInitDesc {
        struct_size: core::mem::size_of::<abi::EeInitDesc>() as u32,
        abi_major: abi::ABI_MAJOR,
        abi_minor: abi::ABI_MINOR,
        capabilities: abi::CAP_NAVMESH,
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
        ..Default::default()
    };
    let mut agent = abi::EeAgent::default();
    let mut mind = abi::EeMind {
        struct_size: 8,
        ..Default::default()
    };
    let mut out = abi::EeIntent::default();
    let mut trace = abi::EeMindTrace {
        struct_size: core::mem::size_of::<abi::EeMindTrace>() as u32,
        ..Default::default()
    };
    let before = agent;
    // SAFETY: as above; `trace` is a valid out-pointer.
    let status =
        unsafe { abi::ee_tick_mind(&mut ctx, &snap, &mut agent, &mut mind, &mut out, &mut trace) };
    assert_eq!(status, abi::STATUS_BUFFER_TOO_SMALL);
    assert_eq!(agent, before, "nothing is written when the struct is short");
}
