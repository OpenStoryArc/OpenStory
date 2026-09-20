//! The reading laws (memory hands, E-04), enforced by a validator the
//! write hands reuse. A reading is a grouping over an arc's exchange
//! handles: every handle exactly once, in the arc's order, and never a
//! handle the arc does not hold. The model is trusted for judgment and
//! for nothing else.

use open_story_patterns::story::{
    validate_reading, Author, Paragraph, Reading, ReadingError, Standing,
};

fn h(n: u32) -> String {
    format!("ex{n:014}")
}

fn author() -> Author {
    Author {
        host: "claude-code".into(),
        model: "claude-fable-5-1".into(),
    }
}

fn reading(groups: &[&[u32]]) -> Reading {
    Reading {
        handle: "arc0000000000001".into(),
        standing: Standing::Final,
        paragraphs: groups
            .iter()
            .map(|g| Paragraph {
                exchanges: g.iter().map(|n| h(*n)).collect(),
                intent: "an intent".into(),
            })
            .collect(),
        author: author(),
    }
}

fn arc() -> Vec<String> {
    (1..=5).map(h).collect()
}

mod when_a_reading_is_validated {
    use super::*;

    #[test]
    fn every_exchange_is_in_exactly_one_group() {
        assert_eq!(
            validate_reading(&reading(&[&[1, 2], &[3], &[4, 5]]), &arc()),
            Ok(())
        );
        assert_eq!(
            validate_reading(&reading(&[&[1, 2], &[4, 5]]), &arc()),
            Err(ReadingError::Missing(vec![h(3)])),
            "a handle left out of every group"
        );
        assert_eq!(
            validate_reading(&reading(&[&[1, 2, 3], &[3, 4, 5]]), &arc()),
            Err(ReadingError::Duplicated(vec![h(3)])),
            "a handle in two groups"
        );
    }

    #[test]
    fn groups_keep_the_arcs_order() {
        assert_eq!(
            validate_reading(&reading(&[&[2, 1], &[3, 4, 5]]), &arc()),
            Err(ReadingError::OutOfOrder {
                expected: h(1),
                found: h(2)
            })
        );
    }

    #[test]
    fn an_empty_group_is_rejected() {
        assert!(matches!(
            validate_reading(&reading(&[&[1, 2, 3, 4, 5], &[]]), &arc()),
            Err(ReadingError::EmptyGroup(1))
        ));
    }
}

mod when_a_reading_names_an_unknown_handle {
    use super::*;

    #[test]
    fn it_is_rejected() {
        assert_eq!(
            validate_reading(&reading(&[&[1, 2, 3, 4, 5, 9]]), &arc()),
            Err(ReadingError::Unknown(vec![h(9)])),
            "a reading never introduces a handle"
        );
    }
}
