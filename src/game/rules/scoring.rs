use crate::game::types::TeamId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinType {
    OneTwo,
    OneThree,
    OneFour,
}

impl WinType {
    pub fn promotion_delta(self) -> u8 {
        match self {
            WinType::OneFour => 1,
            WinType::OneThree => 2,
            WinType::OneTwo => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    J,
    Q,
    K,
    A,
}

impl Level {
    pub fn promote_by(self, delta: u8) -> Level {
        let idx = self.to_idx();
        let next = idx.saturating_add(delta as u16);
        Level::from_idx(next)
    }

    fn to_idx(self) -> u16 {
        match self {
            Level::Two => 2,
            Level::Three => 3,
            Level::Four => 4,
            Level::Five => 5,
            Level::Six => 6,
            Level::Seven => 7,
            Level::Eight => 8,
            Level::Nine => 9,
            Level::Ten => 10,
            Level::J => 11,
            Level::Q => 12,
            Level::K => 13,
            Level::A => 14,
        }
    }

    fn from_idx(idx: u16) -> Level {
        match idx {
            2 => Level::Two,
            3 => Level::Three,
            4 => Level::Four,
            5 => Level::Five,
            6 => Level::Six,
            7 => Level::Seven,
            8 => Level::Eight,
            9 => Level::Nine,
            10 => Level::Ten,
            11 => Level::J,
            12 => Level::Q,
            13 => Level::K,
            _ => Level::A, // cap at A
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamProgress {
    pub team: TeamId,
    pub level: Level,
    /// Count of unsuccessful attempts as A-level declarer (not necessarily consecutive).
    pub ace_failed_attempts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandResult {
    pub winner_team: TeamId,
    pub win_type: WinType,
    pub promotion_delta: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner_team: Option<TeamId>,
    pub next_declarer: TeamId,
    pub progress_ew: TeamProgress,
    pub progress_sn: TeamProgress,
}

pub struct ScoringService;

impl ScoringService {
    /// Apply scoring to team progress.
    ///
    /// Inputs:
    /// - `declarer`: which team is declarer in this hand
    /// - `win_type`: 1-2/1-3/1-4
    /// - `winner`: which team won this hand
    /// - `ace_finish_demotes_declarer`: only relevant when declarer is A-level and loses:
    ///   if true, declarer is demoted to level 2 immediately.
    pub fn apply_hand(
        progress_ew: TeamProgress,
        progress_sn: TeamProgress,
        declarer: TeamId,
        winner: TeamId,
        win_type: WinType,
        ace_finish_demotes_declarer: bool,
    ) -> Result<GameOutcome, String> {
        let delta = win_type.promotion_delta();

        let mut ew = progress_ew;
        let mut sn = progress_sn;

        let (mut declarer_prog, opp_prog) = match declarer {
            TeamId::Ew => (ew.clone(), sn.clone()),
            TeamId::Sn => (sn.clone(), ew.clone()),
        };

        let declarer_won = winner == declarer;
        let declarer_is_a = declarer_prog.level == Level::A;

        // A-level terminal win: declarer wins by 1-2 or 1-3.
        if declarer_is_a && declarer_won && matches!(win_type, WinType::OneTwo | WinType::OneThree)
        {
            return Ok(GameOutcome {
                winner_team: Some(declarer),
                next_declarer: declarer,
                progress_ew: ew,
                progress_sn: sn,
            });
        }

        // Promotions: winner team always promotes by full delta (cap at A).
        match winner {
            TeamId::Ew => ew.level = ew.level.promote_by(delta),
            TeamId::Sn => sn.level = sn.level.promote_by(delta),
        }

        // A-level attempt tracking and special demotion.
        if declarer_is_a {
            if !declarer_won && ace_finish_demotes_declarer {
                declarer_prog.level = Level::Two;
                declarer_prog.ace_failed_attempts = 0;
            } else {
                declarer_prog.ace_failed_attempts += 1;
                if declarer_prog.ace_failed_attempts >= 3 {
                    declarer_prog.level = Level::Two;
                    declarer_prog.ace_failed_attempts = 0;
                }
            }
        }

        // Write back: winner promotion already applied to `ew`/`sn`. Only overwrite declarer's
        // level when A-level rules modified `declarer_prog` (never clobber winner's level with a
        // stale `opp_prog` snapshot).
        match declarer {
            TeamId::Ew => {
                ew.ace_failed_attempts = declarer_prog.ace_failed_attempts;
                if declarer_is_a {
                    ew.level = declarer_prog.level;
                }
                sn.ace_failed_attempts = opp_prog.ace_failed_attempts;
            }
            TeamId::Sn => {
                sn.ace_failed_attempts = declarer_prog.ace_failed_attempts;
                if declarer_is_a {
                    sn.level = declarer_prog.level;
                }
                ew.ace_failed_attempts = opp_prog.ace_failed_attempts;
            }
        }

        Ok(GameOutcome {
            winner_team: None,
            next_declarer: winner,
            progress_ew: ew,
            progress_sn: sn,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(team: TeamId, level: Level, fails: u32) -> TeamProgress {
        TeamProgress {
            team,
            level,
            ace_failed_attempts: fails,
        }
    }

    #[test]
    fn a_level_declarer_wins_12_game_over() {
        let ew = prog(TeamId::Ew, Level::A, 0);
        let sn = prog(TeamId::Sn, Level::K, 0);
        let out =
            ScoringService::apply_hand(ew, sn, TeamId::Ew, TeamId::Ew, WinType::OneTwo, false)
                .unwrap();
        assert_eq!(out.winner_team, Some(TeamId::Ew));
    }

    #[test]
    fn a_level_declarer_14_increments_fail_and_demotes_on_third() {
        let ew = prog(TeamId::Ew, Level::A, 2);
        let sn = prog(TeamId::Sn, Level::Q, 0);
        let out =
            ScoringService::apply_hand(ew, sn, TeamId::Ew, TeamId::Ew, WinType::OneFour, false)
                .unwrap();
        assert_eq!(out.winner_team, None);
        assert_eq!(out.progress_ew.level, Level::Two);
        assert_eq!(out.progress_ew.ace_failed_attempts, 0);
    }

    #[test]
    fn a_level_declarer_loses_and_ace_finish_demotes_to_two() {
        let ew = prog(TeamId::Ew, Level::A, 1);
        let sn = prog(TeamId::Sn, Level::Ten, 0);
        let out =
            ScoringService::apply_hand(ew, sn, TeamId::Ew, TeamId::Sn, WinType::OneThree, true)
                .unwrap();
        assert_eq!(out.progress_ew.level, Level::Two);
        assert_eq!(out.progress_ew.ace_failed_attempts, 0);
    }

    #[test]
    fn non_a_winner_promotes_by_win_type() {
        let ew = prog(TeamId::Ew, Level::Five, 0);
        let sn = prog(TeamId::Sn, Level::K, 0);
        let out =
            ScoringService::apply_hand(ew, sn, TeamId::Ew, TeamId::Sn, WinType::OneTwo, false)
                .unwrap();
        assert_eq!(out.progress_sn.level, Level::A);
    }

    #[test]
    fn level_promote_capped_at_a() {
        assert_eq!(Level::K.promote_by(4), Level::A);
        assert_eq!(Level::A.promote_by(4), Level::A);
    }
}
