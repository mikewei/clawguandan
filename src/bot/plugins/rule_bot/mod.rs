use crate::bot::plugin::{BotDecision, BotPlugin, BotTurnContext};
use crate::game::engine::PlayerAction;
use std::sync::Arc;

use self::features::extract_rule_features;
pub use self::params::RuleBotParams;
use self::scoring::{PlayCandidate, choose_play_candidate};

mod features;
mod params;
mod scoring;

#[derive(Clone, Debug)]
pub struct RuleBotPlugin {
    params: Arc<RuleBotParams>,
}

impl Default for RuleBotPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(RuleBotParams::default_balanced()),
        }
    }
}

impl RuleBotPlugin {
    pub fn with_params(params: RuleBotParams) -> Self {
        Self {
            params: Arc::new(params),
        }
    }
}

impl BotPlugin for RuleBotPlugin {
    fn name(&self) -> &'static str {
        "rule-bot"
    }

    fn observer_prefix(&self) -> &'static str {
        "rb"
    }

    fn decide(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        match ctx.expect_kind.as_str() {
            "ready" => return Ok(BotDecision::Ready),
            "tribute" | "exchange" => return Ok(BotDecision::UseSuggest),
            "play" => {}
            _ => return Ok(BotDecision::UseSuggest),
        };

        let features = extract_rule_features(
            &ctx.state,
            self.params.enemy_low_cards_threshold,
            self.params.endgame_hand_count_threshold,
        );

        // Hard gate: only pass is legal.
        if features.can_pass && !features.can_play {
            return Ok(BotDecision::Action(PlayerAction::Pass));
        }
        // Hard gate: cannot pass.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mk_play_ctx(
        legal_actions: &[&str],
        hand_cards: usize,
        teammate_seat: &str,
        top_play_seat: Option<&str>,
        enemy_remaining: u8,
    ) -> BotTurnContext {
        let top_play = top_play_seat.map(|seat| json!({ "seat": seat }));
        let state = json!({
            "expect": { "kind": "play", "legalActions": legal_actions },
            "private": {
                "seat": "E",
                "teammateSeat": teammate_seat,
                "handCards": vec!["♠3"; hand_cards]
            },
            "hand": {
                "topPlay": top_play
            },
            "seats": {
                "E": { "remainingCount": hand_cards },
                "W": { "remainingCount": 3 },
                "N": { "remainingCount": enemy_remaining },
                "S": { "remainingCount": 8 }
            }
        });
        BotTurnContext {
            table_id: "t".into(),
            player_id: "p".into(),
            expect_kind: "play".into(),
            state,
        }
    }

    #[test]
    fn pass_only_is_directly_taken() {
        let bot = RuleBotPlugin::default();
        let ctx = mk_play_ctx(&["pass"], 10, "W", Some("N"), 5);
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::Action(PlayerAction::Pass)));
    }

    #[test]
    fn aggressive_profile_prefers_suggest_when_enemy_is_urgent() {
        let bot = RuleBotPlugin::with_params(RuleBotParams::default_aggressive());
        let ctx = mk_play_ctx(&["play", "pass"], 10, "W", Some("W"), 1);
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }

    #[test]
    fn supportive_profile_prefers_pass_when_partner_leads() {
        let bot = RuleBotPlugin::with_params(RuleBotParams::default_supportive());
        let ctx = mk_play_ctx(&["play", "pass"], 10, "W", Some("W"), 6);
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::Action(PlayerAction::Pass)));
    }

    #[test]
    fn endgame_bias_prefers_suggest_for_clear_hand() {
        let mut p = RuleBotParams::default_balanced();
        p.endgame_hand_count_threshold = 7;
        let bot = RuleBotPlugin::with_params(p);
        let ctx = mk_play_ctx(&["play", "pass"], 5, "W", Some("N"), 5);
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }

    #[test]
    fn neutral_table_with_many_small_cards_prefers_suggest() {
        let bot = RuleBotPlugin::default();
        let ctx = BotTurnContext {
            table_id: "t".into(),
            player_id: "p".into(),
            expect_kind: "play".into(),
            state: json!({
                "expect": { "kind": "play", "legalActions": ["play", "pass"] },
                "private": {
                    "seat": "E",
                    "teammateSeat": "W",
                    "handCards": ["♠3", "♥4", "♦5", "♣6", "♠7", "♥8", "♣Q", "♠K", "♦A", "♣10"]
                },
                "hand": {
                    "topPlay": { "seat": "N" }
                },
                "seats": {
                    "E": { "remainingCount": 10 },
                    "W": { "remainingCount": 7 },
                    "N": { "remainingCount": 6 },
                    "S": { "remainingCount": 8 }
                }
            }),
        };
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }

    #[test]
    fn missing_context_does_not_panic_and_can_fallback_to_suggest() {
        let bot = RuleBotPlugin::default();
        let ctx = BotTurnContext {
            table_id: "t".into(),
            player_id: "p".into(),
            expect_kind: "play".into(),
            state: json!({
                "expect": { "kind": "play", "legalActions": ["play"] }
            }),
        };
        let d = bot.decide(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }
}
