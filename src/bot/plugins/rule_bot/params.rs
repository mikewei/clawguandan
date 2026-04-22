#[derive(Clone, Debug)]
pub struct RuleBotParams {
    pub team_win_weight: f32,
    pub first_out_weight: f32,
    pub second_out_weight: f32,
    pub yield_to_partner_bias: f32,
    pub partner_support_threshold: u8,
    pub bomb_conserve_bias: f32,
    pub bomb_aggression_when_enemy_low_cards: f32,
    pub enemy_low_cards_threshold: u8,
    pub endgame_hand_count_threshold: u8,
    pub endgame_clear_hand_bias: f32,
    pub proactive_play_bias: f32,
    pub low_card_dump_bias: f32,
    pub pass_stall_penalty: f32,
    pub use_suggest_fallback: bool,
    pub enable_reason_trace: bool,
}

impl RuleBotParams {
    pub fn default_balanced() -> Self {
        Self {
            team_win_weight: 1.0,
            first_out_weight: 0.8,
            second_out_weight: 0.9,
            yield_to_partner_bias: 1.4,
            partner_support_threshold: 2,
            bomb_conserve_bias: 0.8,
            bomb_aggression_when_enemy_low_cards: 2.2,
            enemy_low_cards_threshold: 2,
            endgame_hand_count_threshold: 6,
            endgame_clear_hand_bias: 1.2,
            proactive_play_bias: 1.1,
            low_card_dump_bias: 1.4,
            pass_stall_penalty: 0.9,
            use_suggest_fallback: true,
            enable_reason_trace: false,
        }
    }

    pub fn default_aggressive() -> Self {
        Self {
            team_win_weight: 0.9,
            first_out_weight: 1.4,
            second_out_weight: 0.7,
            yield_to_partner_bias: 0.6,
            partner_support_threshold: 2,
            bomb_conserve_bias: 0.3,
            bomb_aggression_when_enemy_low_cards: 2.8,
            enemy_low_cards_threshold: 3,
            endgame_hand_count_threshold: 8,
            endgame_clear_hand_bias: 2.0,
            proactive_play_bias: 1.6,
            low_card_dump_bias: 1.1,
            pass_stall_penalty: 1.2,
            use_suggest_fallback: true,
            enable_reason_trace: false,
        }
    }

    pub fn default_supportive() -> Self {
        Self {
            team_win_weight: 1.4,
            first_out_weight: 0.7,
            second_out_weight: 1.3,
            yield_to_partner_bias: 2.2,
            partner_support_threshold: 3,
            bomb_conserve_bias: 1.1,
            bomb_aggression_when_enemy_low_cards: 1.6,
            enemy_low_cards_threshold: 2,
            endgame_hand_count_threshold: 6,
            endgame_clear_hand_bias: 1.0,
            proactive_play_bias: 0.6,
            low_card_dump_bias: 1.2,
            pass_stall_penalty: 0.5,
            use_suggest_fallback: true,
            enable_reason_trace: false,
        }
    }
}

impl Default for RuleBotParams {
    fn default() -> Self {
        Self::default_balanced()
    }
}
