//! The shape of the scale model.
//!
//! The model has a shape, and the shape is what the measurement is about: a
//! model whose accounts were all the same size would measure the easy case
//! and hide the hard one.

use hodei_evidence_storage::scale::ScaleModel;

#[test]
fn the_spread_goes_from_the_smallest_to_the_largest() {
    let model = ScaleModel::reference();

    assert_eq!(
        model.spread(5, 60, 0, model.accounts),
        5,
        "the first account is the smallest"
    );
    assert_eq!(
        model.spread(5, 60, model.accounts - 1, model.accounts),
        60,
        "the last account is the largest"
    );
}

#[test]
fn the_spread_never_leaves_the_bounds() {
    let model = ScaleModel::reference();

    for index in 0..model.accounts {
        let value = model.spread(
            model.min_repositories,
            model.max_repositories,
            index,
            model.accounts,
        );
        assert!(
            value >= model.min_repositories && value <= model.max_repositories,
            "the spread of the account {index} is {value}, which is outside the bounds"
        );
    }
}

#[test]
fn the_spread_grows_and_never_shrinks() {
    let model = ScaleModel::reference();

    let mut previous = 0;
    for index in 0..model.accounts {
        let value = model.spread(
            model.min_repositories,
            model.max_repositories,
            index,
            model.accounts,
        );
        assert!(
            value >= previous,
            "the account {index} is smaller than the one before it: {value} < {previous}"
        );
        previous = value;
    }
}

#[test]
fn the_spread_grows_slowly_at_the_beginning() {
    let model = ScaleModel::reference();

    // The whole point of the spread: the small sizes are many and the big
    // ones are few. A straight line would make the first account already
    // half of the largest one.
    let first = model.spread(5, 60, 0, model.accounts);
    let half = model.spread(5, 60, model.accounts / 2, model.accounts);
    assert!(
        first * 3 < half,
        "the spread grows too fast at the beginning: {first} then {half}"
    );
}

#[test]
fn the_small_model_is_small() {
    let model = ScaleModel::small();

    assert!(
        model.expected_events() < 500,
        "the small model is not small"
    );
}

#[test]
fn the_reference_model_is_bigger_than_the_small_one() {
    assert!(
        ScaleModel::reference().expected_events() > ScaleModel::small().expected_events(),
        "the reference model has to be bigger than the small one"
    );
}

#[test]
fn the_count_of_the_events_is_the_size_of_the_journeys() {
    let model = ScaleModel::small();

    let journeys: usize = (0..model.accounts)
        .map(|index| model.journey(&format!("account-{index}"), index).len())
        .sum();

    assert_eq!(
        journeys,
        model.expected_events(),
        "the model says how many pieces of evidence there are, and the journeys have to add up"
    );
}

#[test]
fn the_count_of_the_events_is_the_size_of_the_journeys_of_the_reference_model() {
    let model = ScaleModel::reference();

    let journeys: usize = (0..model.accounts)
        .map(|index| model.journey(&format!("account-{index}"), index).len())
        .sum();

    assert_eq!(
        journeys,
        model.expected_events(),
        "the model says how many pieces of evidence there are, and the journeys have to add up"
    );
}

#[test]
fn the_journey_of_the_biggest_account_is_bigger_than_the_smallest_one() {
    let model = ScaleModel::reference();

    let smallest = model.journey("account-0", 0).len();
    let biggest = model.journey("account-last", model.accounts - 1).len();

    assert!(
        biggest > smallest,
        "the biggest account has {biggest} pieces and the smallest {smallest}; \
         the model does not spread the accounts"
    );
}

#[test]
fn the_shape_of_the_accounts_is_spread_out_at_every_level() {
    let model = ScaleModel::reference();

    for index in 0..model.accounts {
        let shape = model.shape(index);
        assert!(
            shape.repositories > 0,
            "the account {index} has no repository"
        );
        assert_eq!(
            shape.revisions.len(),
            shape.repositories,
            "the account {index} says how many revisions each repository has"
        );
        for (repository, builds) in shape.builds.iter().enumerate() {
            assert_eq!(
                builds.len(),
                shape.revisions[repository],
                "the repository {repository} of the account {index} says how many builds \
                 each revision has"
            );
        }
    }
}

#[test]
fn the_journey_is_connected() {
    let model = ScaleModel::small();

    for index in 0..model.accounts {
        let journey = model.journey(&format!("account-{index}"), index);
        assert!(!journey.is_empty(), "the journey of an account is empty");

        for (_, _, _, claim) in &journey {
            let claim = claim
                .as_ref()
                .expect("every piece of the journey states the relation it carries");
            assert!(
                !claim.from.to_string().is_empty() && !claim.to.to_string().is_empty(),
                "a relation of the journey has an empty side"
            );
        }
    }
}

/// The pieces of a journey are told apart by their identity.
///
/// Two pieces that carry the same identity are the same piece: the store
/// takes the second one for a repetition and keeps only one fact. The
/// measure of the scale model found it: 858 pieces of the reference load
/// carried the identity of another one and never became facts, so the
/// load was smaller than the model said.
///
/// The identity of a piece is the identity of the external record it
/// speaks of, so two pieces of the same build that claim different
/// artefacts are two pieces.
#[test]
fn no_two_pieces_of_a_journey_carry_the_same_identity() {
    for model in [ScaleModel::small(), ScaleModel::reference()] {
        for index in 0..model.accounts {
            let journey = model.journey(&format!("account-{index}"), index);

            let mut identities = std::collections::HashSet::new();
            for (identity, _, _, _) in &journey {
                assert!(
                    identities.insert(identity.clone()),
                    "the journey of account {index} carries the identity {identity} twice"
                );
            }
        }
    }
}

/// The pieces of the journeys of two accounts are told apart as well.
///
/// The accounts share the store, so a piece of one account that carries
/// the identity of a piece of another account would be taken for a
/// repetition, and one of the accounts would lose facts.
#[test]
fn no_two_accounts_carry_the_same_identity() {
    let model = ScaleModel::small();

    let mut identities = std::collections::HashSet::new();
    for index in 0..model.accounts {
        let journey = model.journey(&format!("account-{index}"), index);
        for (identity, _, _, _) in &journey {
            assert!(
                identities.insert(identity.clone()),
                "two accounts carry the identity {identity}"
            );
        }
    }
}
