//! `idaptik-bevy`: the Bevy evaluation frontend for the Ghost Lobby scenario
//! (ADR-0003) — a side-on 2.5D cross-section of the building over
//! [`idaptik_core`]'s `GhostLobbySim`, driven from the same `Command`/`Event`
//! wire API as the TUI.
//!
//! Local mode usage:
//!   cargo run -p idaptik-bevy [-- --seed N --difficulty story|standard|operator --reduced-motion]
//!
//! Multiplayer mode usage:
//!   cargo run -p idaptik-bevy -- --host [--role infiltrator|hacker] [--session NAME] [--url URL]
//!   cargo run -p idaptik-bevy -- --join HOST [--role infiltrator|hacker] [--session NAME] [--url URL]
//!
//! On Linux, Bevy needs system libraries — install them with
//! `just bevy-linux-deps`.

use bevy::prelude::*;
use clap::Parser;
use idaptik_bevy::FrontendPlugin;
use idaptik_bevy::driver::SimDriverPlugin;
use idaptik_core::RunConfig;
use idaptik_core::scenario::DifficultyId;
use std::path::PathBuf;

/// IDApTIK Bevy frontend - local or multiplayer mode
#[derive(Parser, Debug)]
#[command(name = "idaptik-bevy")]
#[command(about = "Bevy frontend for IDApTIK - local or multiplayer mode")]
#[command(
    after_help = "Note: Multiplayer requires a relay server running on the specified port (default: 1984 via IDAPTIK_PORT env var)"
)]
struct Cli {
    /// Run in local single-player mode (default)
    #[arg(long, default_value_t = true, conflicts_with_all = ["host", "join"])]
    local: bool,

    /// Host a multiplayer session
    #[arg(long, conflicts_with_all = ["local", "join"])]
    host: bool,

    /// Join a multiplayer session at the given host address
    #[arg(long, value_name = "HOST", conflicts_with_all = ["local", "host"])]
    join: Option<String>,

    /// Your role in multiplayer: infiltrator or hacker
    #[arg(long, value_name = "ROLE", default_value = "infiltrator")]
    role: String,

    /// Session ID to join or host (default: ghost-lobby)
    #[arg(long, value_name = "NAME", default_value = "ghost-lobby")]
    session: String,

    /// Relay URL (default: ws://127.0.0.1:1984/voice/websocket, or use IDAPTIK_PORT)
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    /// Path to script file (default: fixtures/session_relay/versus_script.json)
    #[arg(long, value_name = "PATH")]
    script: Option<PathBuf>,

    /// Input delay in ticks for delay-lockstep (default: 3)
    #[arg(long, default_value_t = 3)]
    input_delay: u64,

    /// Random seed for the run
    #[arg(long)]
    seed: Option<u32>,

    /// Difficulty level: story, standard, or operator
    #[arg(long)]
    difficulty: Option<String>,

    /// Reduce motion effects
    #[arg(long)]
    reduced_motion: bool,

    /// Play the canonical Ghost Lobby without its signal network
    #[arg(long)]
    unwired: bool,
}

fn main() -> AppExit {
    let cli = Cli::parse();

    // Determine if we're in multiplayer mode
    let netplay_config = if cli.host || cli.join.is_some() {
        Some(make_netplay_config(&cli))
    } else {
        None
    };

    // Build RunConfig from CLI
    let (cfg, seed) = make_run_config(cli.seed, cli.difficulty.clone(), cli.reduced_motion);

    let window_plugin = WindowPlugin {
        primary_window: Some(Window {
            title: "IDApTIK — Envelope 001: Ghost Lobby".to_owned(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut app = App::new();
    // No audio yet: Bevy's audio plugin opens the default output device at
    // startup even when nothing plays, which grabs it from the desktop's
    // sound server. Add it back when the game has sound.
    app.add_plugins(
        DefaultPlugins
            .set(window_plugin)
            .disable::<bevy::audio::AudioPlugin>(),
    );

    if let Some(netplay_cfg) = netplay_config {
        // Multiplayer mode
        app.add_plugins(idaptik_bevy::netplay::NetplayPlugin::new(netplay_cfg));
    } else {
        // Local mode
        app.add_plugins(SimDriverPlugin {
            cfg,
            seed,
            wired: !cli.unwired,
        });
        // In the background, redraw ten times a second, not sixty: the sim's
        // fixed timestep catches up, so play is unaffected, but the game stops
        // competing with whatever you switched to.
        app.insert_resource(bevy::winit::WinitSettings {
            focused_mode: bevy::winit::UpdateMode::Continuous,
            unfocused_mode: bevy::winit::UpdateMode::reactive_low_power(
                std::time::Duration::from_millis(100),
            ),
        });
        app.add_plugins(FrontendPlugin);

        // The Evil Weevil guard brain, opt-in behind the `weevil` feature.
        // The profile and seed come from the environment so a player can
        // compare the kernel against the classic FSM without a rebuild.
        #[cfg(feature = "weevil")]
        app.add_plugins(idaptik_bevy::guard_weevil::GuardBrainPlugin::for_profile(
            std::env::var("IDAPTIK_WEEVIL_PROFILE").unwrap_or_else(|_| "cautious-sentry".into()),
            std::env::var("IDAPTIK_WEEVIL_SEED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(7),
        ));
    }

    app.run()
}

/// Create NetplayConfig from CLI arguments
fn make_netplay_config(cli: &Cli) -> idaptik_bevy::netplay::NetplayConfig {
    use idaptik_bevy::netplay::NetplayMode;

    let port = std::env::var("IDAPTIK_PORT").unwrap_or_else(|_| "1984".into());
    let default_url = format!("ws://127.0.0.1:{}/voice/websocket", port);

    idaptik_bevy::netplay::NetplayConfig {
        mode: if cli.host {
            NetplayMode::Host
        } else {
            NetplayMode::Join {
                host: cli.join.clone().unwrap_or_else(|| "127.0.0.1".into()),
            }
        },
        role: cli.role.clone(),
        session: cli.session.clone(),
        relay_url: cli.url.clone().unwrap_or(default_url),
        script_path: cli
            .script
            .clone()
            .unwrap_or_else(|| PathBuf::from("fixtures/session_relay/versus_script.json")),
        input_delay: cli.input_delay,
    }
}

/// Build RunConfig and seed from CLI arguments
fn make_run_config(
    seed: Option<u32>,
    difficulty: Option<String>,
    reduced_motion: bool,
) -> (RunConfig, u32) {
    let mut cfg = RunConfig::standard();
    let seed = seed.unwrap_or(123456u32);

    if let Some(d) = difficulty {
        cfg.difficulty = match d.as_str() {
            "story" => DifficultyId::Story,
            "standard" => DifficultyId::Standard,
            "operator" => DifficultyId::Operator,
            other => {
                eprintln!("warning: unknown difficulty '{}', using standard", other);
                DifficultyId::Standard
            }
        };
    }

    if reduced_motion {
        cfg.reduced_motion = true;
    }

    (cfg, seed)
}
