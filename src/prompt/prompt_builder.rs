use crate::domain::Expect;

pub fn build_player_prompt(expect: &Expect, is_ready: Option<bool>, is_actor: bool) -> String {
    match expect.kind.as_str() {
        "join" => "Waiting for more players to join.".into(),
        "ready" => {
            if is_ready.unwrap_or(false) {
                "You are ready. Waiting for other players.".into()
            } else {
                "Please send ready when you are prepared to start.".into()
            }
        }
        "tribute" => "Tribute phase: choose your tribute card.".into(),
        "exchange" => "Exchange phase: choose your return card.".into(),
        "play" => {
            if is_actor {
                "Your turn: choose one legal action: play or pass.".into()
            } else {
                "Game in progress. Waiting for next transition.".into()
            }
        }
        "wait" => "Game in progress. Waiting for next transition.".into(),
        "game_over" => "Game finished.".into(),
        _ => "Continue.".into(),
    }
}

pub fn build_observer_prompt(expect: &Expect) -> String {
    match expect.kind.as_str() {
        "join" => "Waiting for players to join.".into(),
        "ready" => "Waiting for players to ready up.".into(),
        "tribute" => "Tribute phase in progress.".into(),
        "exchange" => "Exchange phase in progress.".into(),
        "play" => "Playing phase in progress.".into(),
        "wait" => "Game in progress.".into(),
        "game_over" => "Game finished.".into(),
        _ => "Observing.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect(kind: &str, actor: Option<&str>) -> Expect {
        Expect {
            kind: kind.to_string(),
            actor_player_id: actor.map(|x| x.to_string()),
            legal_actions: Vec::new(),
            deadline_at: None,
        }
    }

    #[test]
    fn player_prompt_play_actor_sees_your_turn() {
        let p = build_player_prompt(&expect("play", Some("p1")), Some(true), true);
        assert!(p.contains("Your turn"));
    }

    #[test]
    fn player_prompt_play_non_actor_sees_waiting() {
        let p = build_player_prompt(&expect("play", Some("p2")), Some(true), false);
        assert_eq!(p, "Game in progress. Waiting for next transition.");
    }
}

