use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct RuleFeatures {
    pub legal_actions: Vec<String>,
    pub can_pass: bool,
    pub can_play: bool,
    pub my_seat: Option<String>,
    pub teammate_seat: Option<String>,
    pub top_play_seat: Option<String>,
    pub my_hand_count: usize,
    pub low_card_count: usize,
    pub low_card_ratio: f32,
    pub teammate_remaining: Option<u8>,
    pub enemy_min_remaining: Option<u8>,
    pub enemy_low_cards_urgent: bool,
    pub endgame_mode: bool,
    pub leading_new_trick: bool,
}

pub fn extract_rule_features(
    state: &Value,
    enemy_low_cards_threshold: u8,
    endgame_hand_count_threshold: u8,
) -> RuleFeatures {
    let legal_actions: Vec<String> = state
        .get("expect")
        .and_then(|x| x.get("legalActions"))
        .and_then(|x| x.as_array())
        .map(|xs| {
            xs.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let can_pass = legal_actions.iter().any(|s| s == "pass");
    let can_play = legal_actions.iter().any(|s| s == "play");

    let my_seat = state
        .get("private")
        .and_then(|x| x.get("seat"))
        .and_then(|x| x.as_str())
        .map(ToString::to_string);
    let teammate_seat = state
        .get("private")
        .and_then(|x| x.get("teammateSeat"))
        .and_then(|x| x.as_str())
        .map(ToString::to_string)
        .filter(|s| !s.is_empty());
    let top_play_seat = state
        .get("hand")
        .and_then(|h| h.get("topPlay"))
        .and_then(|tp| tp.get("seat"))
        .and_then(|x| x.as_str())
        .map(ToString::to_string);

    let hand_cards: Vec<String> = state
        .get("private")
        .and_then(|x| x.get("handCards"))
        .and_then(|x| x.as_array())
        .map(|cards| {
            cards
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let hand_level = state
        .get("hand")
        .and_then(|h| h.get("handLevel"))
        .and_then(|v| v.as_str());

    let my_hand_count = hand_cards.len();
    let low_card_count = hand_cards
        .iter()
        .filter(|s| is_small_card_symbol(s, hand_level))
        .count();
    let low_card_ratio = if my_hand_count == 0 {
        0.0
    } else {
        low_card_count as f32 / my_hand_count as f32
    };

    let teammate_remaining = teammate_seat
        .as_ref()
        .and_then(|seat| remaining_count_by_seat(state, seat));
    let enemy_min_remaining = min_enemy_remaining(state, my_seat.as_deref(), teammate_seat.as_deref());
    let enemy_low_cards_urgent = enemy_min_remaining
        .map(|r| r <= enemy_low_cards_threshold)
        .unwrap_or(false);
    let endgame_mode = my_hand_count <= endgame_hand_count_threshold as usize;
    let leading_new_trick = top_play_seat.is_none();

    RuleFeatures {
        legal_actions,
        can_pass,
        can_play,
        my_seat,
        teammate_seat,
        top_play_seat,
        my_hand_count,
        low_card_count,
        low_card_ratio,
        teammate_remaining,
        enemy_min_remaining,
        enemy_low_cards_urgent,
        endgame_mode,
        leading_new_trick,
    }
}

fn remaining_count_by_seat(state: &Value, seat: &str) -> Option<u8> {
    state
        .get("seats")
        .and_then(|x| x.get(seat))
        .and_then(|x| x.get("remainingCount"))
        .and_then(|x| x.as_u64())
        .map(|x| x as u8)
}

fn min_enemy_remaining(state: &Value, my_seat: Option<&str>, teammate_seat: Option<&str>) -> Option<u8> {
    let seats = state.get("seats").and_then(|x| x.as_object())?;
    seats
        .iter()
        .filter_map(|(seat, _)| {
            if my_seat == Some(seat.as_str()) || teammate_seat == Some(seat.as_str()) {
                None
            } else {
                remaining_count_by_seat(state, seat)
            }
        })
        .min()
}

fn is_small_card_symbol(card: &str, hand_level: Option<&str>) -> bool {
    let Some(rank) = card_rank_token(card) else {
        return false;
    };
    let is_small = matches!(rank, "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10");
    if !is_small {
        return false;
    }
    hand_level != Some(rank)
}

fn card_rank_token(card: &str) -> Option<&str> {
    let t = card.trim();
    if t.starts_with("🃏") {
        return None;
    }
    for suit in ["♠", "♥", "♦", "♣"] {
        if let Some(rest) = t.strip_prefix(suit) {
            return Some(rest);
        }
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_card_range_is_two_to_ten() {
        assert!(is_small_card_symbol("♠2", Some("A")));
        assert!(is_small_card_symbol("♥10", Some("A")));
        assert!(!is_small_card_symbol("♣J", Some("A")));
        assert!(!is_small_card_symbol("🃏R", Some("A")));
    }

    #[test]
    fn small_card_excludes_current_hand_level() {
        assert!(!is_small_card_symbol("♠2", Some("2")));
        assert!(!is_small_card_symbol("♥10", Some("10")));
        assert!(is_small_card_symbol("♦9", Some("10")));
    }
}
