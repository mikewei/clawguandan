//! CLI subprocess helpers and optional pure-engine simulation.

pub mod cli;
pub mod engine;
pub mod http;

pub use cli::{
    cli_argv_play_pass, cli_argv_play_playcards, cli_argv_play_playcards_wild, cli_argv_play_ready,
    cli_argv_play_returncard, cli_argv_play_suggest, cli_argv_play_tribute, cli_argv_play_wait4myturn,
    cli_argv_table_create, cli_argv_table_join, cli_argv_table_nextstate,
    cli_argv_table_nextstate_observer, cli_argv_table_snapshot, run_cli_command, CliRunError,
};
pub use engine::{run_match_engine, EngineSimError, EngineSimOutcome};
pub use http::{run_hand_until_scoring_via_router, HttpSimError};
