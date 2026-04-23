use std::sync::Arc;

use crate::bot::plugin::{BotDecision, BotTurnContext};
use crate::bot::policies::{ExchangePolicy, PlayPolicy, TributePolicy};

use super::parse::{self, parsed_decision_to_bot_decision};
use super::prompt;
use super::script;
use super::LlmBotParams;

#[derive(Debug)]
pub struct LlmScriptPolicy {
    pub(crate) params: Arc<LlmBotParams>,
}

impl LlmScriptPolicy {
    fn decide_scripted(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        if self.params.verbosity >= 2 {
            println!(
                "[llm-bot] decide: table={} player={} expect_kind={}",
                ctx.table_id, ctx.player_id, ctx.expect_kind
            );
        }

        let prompt = prompt::decision_prompt(&ctx.expect_kind, &ctx.state);
        let stdout = match script::run_script_with_timeout(
            &self.params.script,
            &prompt,
            self.params.timeout,
            self.params.verbosity,
            "decision",
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[llm-bot] script error: {e}");
                return Ok(parse::fallback_decision(&ctx.expect_kind));
            }
        };

        let parsed = parse::parse_decision_stdout(&stdout);
        if self.params.verbosity >= 2 {
            println!("[llm-bot] decide: parsed = {parsed:?}");
        }
        let mut decision = parsed_decision_to_bot_decision(parsed, &ctx.expect_kind);
        if self.params.verbosity >= 2 {
            println!("[llm-bot] decide: after parsed + DEFAULT/malformed mapping = {decision:?}");
        }
        decision = parse::validate_decision_against_state(decision, &ctx.state);
        if self.params.verbosity >= 2 {
            println!("[llm-bot] decide: after validate_decision_against_state = {decision:?}");
        }
        Ok(decision)
    }
}

impl PlayPolicy for LlmScriptPolicy {
    fn decide_play(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        self.decide_scripted(ctx)
    }
}

impl TributePolicy for LlmScriptPolicy {
    fn decide_tribute(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        self.decide_scripted(ctx)
    }
}

impl ExchangePolicy for LlmScriptPolicy {
    fn decide_exchange(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        self.decide_scripted(ctx)
    }
}
