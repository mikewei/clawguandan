use crate::game::types::TeamId;
use serde_json::json;

pub fn is_big_play_combination(combination_type: &str) -> bool {
    let ct = combination_type.trim();
    ct.starts_with("bomb") || ct == "straightFlush" || ct == "fourJoker"
}

pub fn format_big_play(player_name: &str, combination_type: &str) -> String {
    let (kind_zh, kind_en) = match combination_type {
        "straightFlush" => ("同花顺炸", "straight flush bomb"),
        "fourJoker" => ("四王炸", "four-joker bomb"),
        x if x.starts_with("bomb") => ("炸弹", "bomb"),
        _ => ("大牌", "big play"),
    };
    bilingual(
        format!("{} 打出{}! 💥", safe_name(player_name), kind_zh),
        format!("{} plays {}! 💥", safe_name(player_name), kind_en),
    )
}

pub fn format_rank_announce(player_name: &str, rank: usize) -> String {
    let (rank_name_zh, rank_name_en) = match rank {
        1 => ("头游", "first out"),
        2 => ("二游", "second out"),
        3 => ("三游", "third out"),
        4 => ("末游", "last out"),
        _ => ("出完", "out"),
    };
    bilingual(
        format!("{} {}! 🏁", safe_name(player_name), rank_name_zh),
        format!("{} {}! 🏁", safe_name(player_name), rank_name_en),
    )
}

pub fn format_tribute_action(
    player_name: &str,
    card: &str,
    target_name: &str,
    is_return: bool,
) -> String {
    if is_return {
        bilingual(
            format!(
                "{}还贡了{}给{}。",
                safe_name(player_name),
                card.trim(),
                safe_name(target_name)
            ),
            format!(
                "{} returned {} to {}.",
                safe_name(player_name),
                card.trim(),
                safe_name(target_name)
            ),
        )
    } else {
        bilingual(
            format!(
                "{}进贡了{}给{}。",
                safe_name(player_name),
                card.trim(),
                safe_name(target_name)
            ),
            format!(
                "{} tributed {} to {}.",
                safe_name(player_name),
                card.trim(),
                safe_name(target_name)
            ),
        )
    }
}

pub fn format_tribute_canceled(opening_player_name: &str) -> String {
    bilingual(
        format!(
            "本局抗贡（免进贡），由{}先出。",
            safe_name(opening_player_name)
        ),
        format!(
            "Tribute canceled for this hand; {} leads first.",
            safe_name(opening_player_name)
        ),
    )
}

fn declarer_phrase(team: TeamId) -> (&'static str, &'static str) {
    match team {
        TeamId::Ew => ("EW（东西组）", "the EW team (East–West)"),
        TeamId::Sn => ("SN（南北组）", "the SN team (South–North)"),
    }
}

/// Opening line: declarer side and hand level (bilingual JSON string).
pub fn format_hand_open(declarer: TeamId, level_api: &str) -> String {
    let (d_zh, d_en) = declarer_phrase(declarer);
    let lv = level_api.trim();
    bilingual(
        format!("本局庄家方为{}，打{}。", d_zh, lv),
        format!("This hand: {} is declarer; hand level {}.", d_en, lv),
    )
}

/// Same as [`format_hand_open`] plus tribute-canceled lead (one combined message).
pub fn format_hand_open_with_tribute_canceled(
    declarer: TeamId,
    level_api: &str,
    opening_player_name: &str,
) -> String {
    let (d_zh, d_en) = declarer_phrase(declarer);
    let lv = level_api.trim();
    let name = safe_name(opening_player_name);
    bilingual(
        format!(
            "本局庄家方为{}，打{}。抗贡（免进贡），由{}先出。",
            d_zh, lv, name
        ),
        format!(
            "This hand: {} is declarer; hand level {}. Tribute canceled; {} leads first.",
            d_en, lv, name
        ),
    )
}

pub fn format_hand_end(
    finishing_names: &[String],
    level_ew: &str,
    level_sn: &str,
    waiting_ready: bool,
    game_over: bool,
    winner_team: Option<TeamId>,
) -> String {
    let ranking_zh = if finishing_names.is_empty() {
        "本手结束".to_string()
    } else {
        format!("本手排名: {}", finishing_names.join(" > "))
    };
    let ranking_en = if finishing_names.is_empty() {
        "Hand ended".to_string()
    } else {
        format!("Ranking: {}", finishing_names.join(" > "))
    };
    let levels_zh = format!("当前级别 EW {} / SN {} 📈", level_ew, level_sn);
    let levels_en = format!("Levels EW {} / SN {} 📈", level_ew, level_sn);
    if game_over {
        let final_score_zh = format!("最终成绩 EW {} / SN {}", level_ew, level_sn);
        let final_score_en = format!("Final score EW {} / SN {}", level_ew, level_sn);
        let (winner_zh, winner_en) = winner_team
            .map(winning_team_phrase)
            .unwrap_or(("未知队伍", "unknown team"));
        bilingual(
            format!(
                "{}；{}，恭喜获胜队{}，游戏结束！🎉",
                ranking_zh, final_score_zh, winner_zh
            ),
            format!(
                "{}; {}. Congratulations to the winning team {}. Game over! 🎉",
                ranking_en, final_score_en, winner_en
            ),
        )
    } else if waiting_ready {
        bilingual(
            format!("{}; {}。请全员再次准备 ▶️", ranking_zh, levels_zh),
            format!("{}; {}. Everyone ready again ▶️", ranking_en, levels_en),
        )
    } else {
        bilingual(
            format!("{}; {}。", ranking_zh, levels_zh),
            format!("{}; {}.", ranking_en, levels_en),
        )
    }
}

fn winning_team_phrase(team: TeamId) -> (&'static str, &'static str) {
    match team {
        TeamId::Ew => ("EW（东西组）", "EW (East-West)"),
        TeamId::Sn => ("SN（南北组）", "SN (South-North)"),
    }
}

pub fn format_game_end_by_leave(leaving_names: &[String]) -> String {
    let names = if leaving_names.is_empty() {
        "玩家".to_string()
    } else {
        leaving_names.join("、")
    };
    bilingual(
        format!("{} 超时离开，游戏结束，本局不计分。", names),
        format!(
            "{} timed out and left. Game ended with no score settlement.",
            names
        ),
    )
}

fn bilingual(zh: String, en: String) -> String {
    json!({ "zh": zh, "en": en }).to_string()
}

fn safe_name(name: &str) -> &str {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "玩家"
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tribute_canceled_narration_mentions_opening_player() {
        let msg = format_tribute_canceled("Alice");
        assert!(msg.contains("抗贡"));
        assert!(msg.contains("Alice"));
        assert!(msg.contains("Tribute canceled"));
    }

    #[test]
    fn hand_open_narration_includes_declarer_and_level() {
        let msg = format_hand_open(TeamId::Ew, "5");
        assert!(msg.contains("庄家方"));
        assert!(msg.contains("打5"));
        assert!(msg.contains("declarer"));
        assert!(msg.contains("hand level 5"));
    }

    #[test]
    fn hand_open_with_tribute_canceled_combines_parts() {
        let msg = format_hand_open_with_tribute_canceled(TeamId::Sn, "A", "Bob");
        assert!(msg.contains("打A"));
        assert!(msg.contains("抗贡"));
        assert!(msg.contains("Bob"));
        assert!(msg.contains("Tribute canceled"));
    }
}
