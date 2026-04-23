use crate::game::rules::combination_parser::{Combination, CombinationClass};

pub struct BeatComparator;

impl BeatComparator {
    pub fn can_beat(top: &Combination, candidate: &Combination) -> bool {
        match (top.class(), candidate.class()) {
            (CombinationClass::Ordinary, CombinationClass::Bomb) => true,
            (CombinationClass::Bomb, CombinationClass::Ordinary) => false,
            (CombinationClass::Ordinary, CombinationClass::Ordinary) => {
                // Same ordinary kind and higher primary wins.
                if top.kind != candidate.kind {
                    return false;
                }
                candidate.primary > top.primary
            }
            (CombinationClass::Bomb, CombinationClass::Bomb) => {
                if candidate.bomb_tier != top.bomb_tier {
                    return candidate.bomb_tier > top.bomb_tier;
                }
                // Same bomb tier: compare primary (for four_joker primary is 0, so equal).
                candidate.primary > top.primary
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::rules::combination_parser::{BombKind, CombinationKind, OrdinaryKind};

    #[test]
    fn ordinary_cannot_beat_different_kind() {
        let top = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
            cards_len: 2,
            primary: 5,
            bomb_tier: 0,
        };
        let cand = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Triple),
            cards_len: 3,
            primary: 10,
            bomb_tier: 0,
        };
        assert!(!BeatComparator::can_beat(&top, &cand));
    }

    #[test]
    fn bomb_beats_ordinary() {
        let top = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Single),
            cards_len: 1,
            primary: 12,
            bomb_tier: 0,
        };
        let cand = Combination {
            kind: CombinationKind::Bomb(BombKind::SameRank { n: 4 }),
            cards_len: 4,
            primary: 2,
            bomb_tier: 1,
        };
        assert!(BeatComparator::can_beat(&top, &cand));
    }

    #[test]
    fn larger_bomb_tier_beats_smaller() {
        let quad = Combination {
            kind: CombinationKind::Bomb(BombKind::SameRank { n: 4 }),
            cards_len: 4,
            primary: 5,
            bomb_tier: 1,
        };
        let quint = Combination {
            kind: CombinationKind::Bomb(BombKind::SameRank { n: 5 }),
            cards_len: 5,
            primary: 5,
            bomb_tier: 2,
        };
        assert!(BeatComparator::can_beat(&quad, &quint));
        assert!(!BeatComparator::can_beat(&quint, &quad));
    }

    #[test]
    fn same_pair_higher_primary_beats() {
        let low = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
            cards_len: 2,
            primary: 5,
            bomb_tier: 0,
        };
        let high = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
            cards_len: 2,
            primary: 9,
            bomb_tier: 0,
        };
        assert!(BeatComparator::can_beat(&low, &high));
        assert!(!BeatComparator::can_beat(&high, &low));
    }

    #[test]
    fn straight_flush_tier_sits_between_quint_and_sext_bomb() {
        let quint = Combination {
            kind: CombinationKind::Bomb(BombKind::SameRank { n: 5 }),
            cards_len: 5,
            primary: 10,
            bomb_tier: 2,
        };
        let straight_flush = Combination {
            kind: CombinationKind::Bomb(BombKind::StraightFlush),
            cards_len: 5,
            primary: 13,
            bomb_tier: 3,
        };
        let sext = Combination {
            kind: CombinationKind::Bomb(BombKind::SameRank { n: 6 }),
            cards_len: 6,
            primary: 3,
            bomb_tier: 4,
        };
        assert!(BeatComparator::can_beat(&quint, &straight_flush));
        assert!(!BeatComparator::can_beat(&sext, &straight_flush));
    }

    #[test]
    fn straight_flush_same_tier_compares_primary() {
        let low = Combination {
            kind: CombinationKind::Bomb(BombKind::StraightFlush),
            cards_len: 5,
            primary: 11,
            bomb_tier: 3,
        };
        let high = Combination {
            kind: CombinationKind::Bomb(BombKind::StraightFlush),
            cards_len: 5,
            primary: 14,
            bomb_tier: 3,
        };
        assert!(BeatComparator::can_beat(&low, &high));
        assert!(!BeatComparator::can_beat(&high, &low));
    }
}
