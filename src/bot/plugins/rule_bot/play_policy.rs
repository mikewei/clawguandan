use std::sync::Arc;

use crate::bot::plugin::{BotDecision, BotTurnContext};
use crate::bot::policies::PlayPolicy;
use crate::game::engine::PlayerAction;

use super::features::extract_rule_features;
use super::params::RuleBotParams;
use super::scoring::{PlayCandidate, choose_play_candidate};

#[derive(Debug)]
pub struct RulePlayPolicy {
    pub params: Arc<RuleBotParams>,
}

impl PlayPolicy for RulePlayPolicy {
    fn decide_play(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        let features = extract_rule_features(
            &ctx.state,
            self.params.enemy_low_cards_threshold,
            self.params.endgame_hand_count_threshold,
        );

        if features.can_pass && !features.can_play {
            return Ok(BotDecision::Action(PlayerAction::Pass));
        }
        if features.can_play && !features.can_pass {
            return Ok(BotDecision::UseSuggest);
        }

        let (picked, trace) = choose_play_candidate(&self.params, &features);
        if self.params.enable_reason_trace {
            eprintln!(
                "[rule-bot] hand={} pass_score={:.2} suggest_score={:.2} legal={:?} my_seat={:?} enemy_min={:?} reasons={:?}",
                features.my_hand_count,
                trace.pass_score,
                trace.suggest_score,
                features.legal_actions,
                features.my_seat,
                features.enemy_min_remaining,
                trace.reasons
            );
        }
        match picked {
            PlayCandidate::Pass => Ok(BotDecision::Action(PlayerAction::Pass)),
            PlayCandidate::SuggestPlay => {
                if self.params.use_suggest_fallback {
                    Ok(BotDecision::UseSuggest)
                } else {
                    Ok(BotDecision::Action(PlayerAction::Pass))
                }
            }
        }
    }
}
