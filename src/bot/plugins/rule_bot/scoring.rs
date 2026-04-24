use super::features::RuleFeatures;
use super::params::RuleBotParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayCandidate {
    Pass,
    SuggestPlay,
}

#[derive(Clone, Debug, Default)]
pub struct ScoreTrace {
    pub pass_score: f32,
    pub suggest_score: f32,
    pub reasons: Vec<String>,
}

pub fn choose_play_candidate(
    params: &RuleBotParams,
    f: &RuleFeatures,
) -> (PlayCandidate, ScoreTrace) {
    let mut trace = ScoreTrace::default();
    let partner_leading = is_partner_leading(f);

    // Bias toward preserving high-value resources in non-urgent trick-follow scenarios.
    if f.can_pass && !f.leading_new_trick && !f.endgame_mode {
        trace.pass_score += params.bomb_conserve_bias * params.team_win_weight;
        trace.reasons.push("pass: conserve high resources".into());
    }

    // If partner leads current trick, prefer yielding unless the table is urgent.
    if partner_leading {
        let partner_low = f
            .teammate_remaining
            .map(|x| x <= params.partner_support_threshold)
            .unwrap_or(false);
        let mut bonus = params.yield_to_partner_bias * params.second_out_weight;
        if partner_low {
            bonus += 0.6 * params.second_out_weight;
            trace.reasons.push("pass: partner near out".into());
        }
        if !f.enemy_low_cards_urgent {
            trace.pass_score += bonus;
            trace.reasons.push("pass: keep partner tempo".into());
        }
    }

    // In neutral situations, encourage active plays to reduce idle passing.
    let neutral_table = !f.enemy_low_cards_urgent && !partner_leading;
    if neutral_table && f.can_play {
        trace.suggest_score += params.proactive_play_bias * params.team_win_weight;
        trace.reasons.push("suggest: proactive tempo".into());
        if f.low_card_count > 0 {
            trace.suggest_score +=
                params.low_card_dump_bias * f.low_card_ratio * params.first_out_weight;
            trace.reasons.push("suggest: dump small cards".into());
        }
        if f.leading_new_trick {
            trace.suggest_score += 0.35 * params.proactive_play_bias;
            trace.reasons.push("suggest: lead and shape trick".into());
        }
    }

    // Discourage repeated passive choices when no strong reason to yield.
    if f.can_pass && neutral_table && !f.endgame_mode {
        trace.pass_score -= params.pass_stall_penalty * params.team_win_weight;
        trace.reasons.push("pass: stall penalty".into());
    }

    // Opponent is close to going out: prefer taking action now.
    if f.enemy_low_cards_urgent {
        trace.suggest_score += params.bomb_aggression_when_enemy_low_cards * params.team_win_weight;
        trace.pass_score -= 0.7 * params.team_win_weight;
        trace.reasons.push("suggest: urgent intercept".into());
    }

    // Endgame mode: encourage proactive clear-hand actions.
    if f.endgame_mode {
        trace.suggest_score += params.endgame_clear_hand_bias * params.first_out_weight;
        trace.reasons.push("suggest: endgame clear-hand".into());
    }

    // If action space is asymmetric, keep deterministic fallback.
    let picked = if !f.can_play && f.can_pass {
        PlayCandidate::Pass
    } else if f.can_play && !f.can_pass {
        PlayCandidate::SuggestPlay
    } else if trace.pass_score > trace.suggest_score {
        PlayCandidate::Pass
    } else {
        PlayCandidate::SuggestPlay
    };

    (picked, trace)
}

fn is_partner_leading(f: &RuleFeatures) -> bool {
    matches!(
        (f.teammate_seat.as_deref(), f.top_play_seat.as_deref()),
        (Some(t), Some(top)) if t == top
    )
}
