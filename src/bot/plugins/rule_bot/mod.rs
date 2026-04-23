use crate::bot::plugin::BotPlugin;
use crate::bot::policies::PlayPolicy;
use std::sync::Arc;

pub use self::params::RuleBotParams;
use self::play_policy::RulePlayPolicy;

mod features;
mod params;
mod play_policy;
mod scoring;

#[derive(Clone)]
pub struct RuleBotPlugin {
    play: Arc<dyn PlayPolicy>,
}

impl Default for RuleBotPlugin {
    fn default() -> Self {
        Self::with_params(RuleBotParams::default_balanced())
    }
}

impl RuleBotPlugin {
    pub fn with_params(params: RuleBotParams) -> Self {
        let params = Arc::new(params);
        Self {
            play: Arc::new(RulePlayPolicy {
                params: Arc::clone(&params),
            }),
        }
    }
}

impl BotPlugin for RuleBotPlugin {
    fn plugin_id(&self) -> &'static str {
        "rule-bot"
    }

    fn play_policy(&self) -> Arc<dyn PlayPolicy> {
        Arc::clone(&self.play)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::plugin::{BotDecision, BotTurnContext};
    use crate::game::engine::PlayerAction;
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
        let d = bot.play_policy().decide_play(&ctx).unwrap();
        assert!(matches!(d, BotDecision::Action(PlayerAction::Pass)));
    }

    #[test]
    fn aggressive_profile_prefers_suggest_when_enemy_is_urgent() {
        let bot = RuleBotPlugin::with_params(RuleBotParams::default_aggressive());
        let ctx = mk_play_ctx(&["play", "pass"], 10, "W", Some("W"), 1);
        let d = bot.play_policy().decide_play(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }

    #[test]
    fn supportive_profile_prefers_pass_when_partner_leads() {
        let bot = RuleBotPlugin::with_params(RuleBotParams::default_supportive());
        let ctx = mk_play_ctx(&["play", "pass"], 10, "W", Some("W"), 6);
        let d = bot.play_policy().decide_play(&ctx).unwrap();
        assert!(matches!(d, BotDecision::Action(PlayerAction::Pass)));
    }

    #[test]
    fn endgame_bias_prefers_suggest_for_clear_hand() {
        let mut p = RuleBotParams::default_balanced();
        p.endgame_hand_count_threshold = 7;
        let bot = RuleBotPlugin::with_params(p);
        let ctx = mk_play_ctx(&["play", "pass"], 5, "W", Some("N"), 5);
        let d = bot.play_policy().decide_play(&ctx).unwrap();
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
        let d = bot.play_policy().decide_play(&ctx).unwrap();
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
        let d = bot.play_policy().decide_play(&ctx).unwrap();
        assert!(matches!(d, BotDecision::UseSuggest));
    }
}
