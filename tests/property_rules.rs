use clawguandan::game::card::{HandLevel, RuleContext};
use clawguandan::game::rules::beat_comparator::BeatComparator;
use clawguandan::game::rules::combination_parser::{
    Combination, CombinationKind, CombinationParser, OrdinaryKind,
};
use clawguandan::game::rules::wildcard_resolver::WildcardResolver;
use proptest::prelude::*;

fn any_card_symbol() -> impl Strategy<Value = String> {
    let suits = prop_oneof![Just("♠"), Just("♥"), Just("♦"), Just("♣")];
    let ranks = prop_oneof![
        Just("A"),
        Just("K"),
        Just("Q"),
        Just("J"),
        Just("10"),
        Just("9"),
        Just("8"),
        Just("7"),
        Just("6"),
        Just("5"),
        Just("4"),
        Just("3"),
        Just("2"),
    ];
    prop_oneof![
        (suits, ranks).prop_map(|(s, r)| format!("{}{}", s, r)),
        Just("🃏R".to_string()),
        Just("🃏b".to_string()),
    ]
}

proptest! {
    #[test]
    fn parser_never_panics_on_random_cards(cards in prop::collection::vec(any_card_symbol(), 1..=10)) {
        let ctx = RuleContext { hand_level: HandLevel::Two };
        let _ = CombinationParser::parse(&cards, Some(&[]), ctx);
    }

    #[test]
    fn wildcard_resolver_never_panics(
        cards in prop::collection::vec(any_card_symbol(), 1..=10),
        targets in prop::collection::vec(any_card_symbol(), 0..=10)
    ) {
        let ctx = RuleContext { hand_level: HandLevel::Two };
        let _ = WildcardResolver::resolve(&cards, Some(&targets), ctx);
    }

    #[test]
    fn beat_ord_pair_antisymmetric(
        a in 1u8..20u8,
        b in 1u8..20u8,
    ) {
        prop_assume!(a != b);
        let low = a.min(b);
        let high = a.max(b);
        let top = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
            cards_len: 2,
            primary: low,
            bomb_tier: 0,
        };
        let cand = Combination {
            kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
            cards_len: 2,
            primary: high,
            bomb_tier: 0,
        };
        assert!(BeatComparator::can_beat(&top, &cand));
        assert!(!BeatComparator::can_beat(&cand, &top));
    }
}

