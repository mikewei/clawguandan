use crate::game::card::{RuleContext, Suit, is_wild, parse_card_symbol};

pub struct WildcardResolver;

impl WildcardResolver {
    pub fn resolve(
        cards: &[String],
        wild_targets: Option<&[String]>,
        ctx: RuleContext,
    ) -> Result<Vec<crate::game::card::Card>, String> {
        let parsed: Vec<_> = cards
            .iter()
            .map(|s| parse_card_symbol(s))
            .collect::<Result<_, _>>()?;
        let wild_count = parsed.iter().filter(|c| is_wild(**c, ctx)).count();

        let targets = wild_targets.unwrap_or(&[]);
        if wild_count != targets.len() {
            return Err(format!(
                "wildTargets length mismatch: expected {}, got {}",
                wild_count,
                targets.len()
            ));
        }

        let mut resolved = Vec::with_capacity(parsed.len());
        let mut ti = 0usize;
        for c in parsed {
            if is_wild(c, ctx) {
                let t = parse_card_symbol(&targets[ti])?;
                if t.suit == Suit::Joker {
                    return Err("wild card cannot represent joker".into());
                }
                resolved.push(t);
                ti += 1;
            } else {
                resolved.push(c);
            }
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::card::HandLevel;

    #[test]
    fn reject_joker_target_for_wild() {
        let cards = vec!["♥2".to_string()];
        let targets = vec!["🃏R".to_string()];
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let err = WildcardResolver::resolve(&cards, Some(&targets), ctx).unwrap_err();
        assert!(err.contains("cannot represent joker"));
    }

    #[test]
    fn length_mismatch_errors() {
        let cards = vec!["♥2".into(), "♥2".into()];
        let targets = vec!["♠K".into()];
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let err = WildcardResolver::resolve(&cards, Some(&targets), ctx).unwrap_err();
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn resolves_single_wild() {
        let cards = vec!["♥2".into()];
        let targets = vec!["♠K".into()];
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let out = WildcardResolver::resolve(&cards, Some(&targets), ctx).unwrap();
        assert_eq!(out.len(), 1);
    }
}
