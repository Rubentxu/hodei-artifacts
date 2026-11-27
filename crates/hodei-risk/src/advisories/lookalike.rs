//! Look-alike names: the pure judgement behind a publish-time typosquat gate
//! (HSP-322 tramo 1, from HSP-166).
//!
//! [`signals::evaluate`](super::signals::evaluate) scores a publication as an
//! advisory. A gate needs something narrower and stricter: a yes/no answer to
//! "is this NEW name a look-alike of a name that already exists?", with a
//! reason a log can carry. This module is that answer, and nothing else:
//!
//! - **Confusable**: the two names share a *skeleton*. The skeleton folds the
//!   things a human eye does not tell apart: case, separator runs (`-`, `_`,
//!   `.`, or none at all), zero-width characters, full-width forms, the
//!   Cyrillic and Greek letters that are drawn like Latin ones, `0`/`o`,
//!   `1`/`l`, `rn`/`m`, `vv`/`w`.
//! - **Edit distance 1**: one substitution, insertion, deletion or adjacent
//!   transposition, reusing [`edit_distance_within`]. It only applies when both
//!   names have at least [`MIN_LEN_FOR_EDIT_DISTANCE`] characters: with `ab`
//!   and `ac` one edit is a different package, not a typo.
//!
//! The function is pure and total: no I/O, no allocation of anything but the
//! skeletons, and the candidate never matches itself (an identical
//! normalized name is the package, not a squat of it).

use super::catalog::{Ecosystem, normalize_name};
use super::signals::edit_distance_within;

/// Names shorter than this are compared by skeleton only: a single edit on a
/// two or three letter name reaches most of the namespace.
pub const MIN_LEN_FOR_EDIT_DISTANCE: usize = 4;

/// Why two names were judged alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookalikeKind {
    /// Same skeleton: they only differ in what the eye does not see.
    Confusable,
    /// One edit away (substitution, insertion, deletion or transposition).
    EditDistance,
}

impl LookalikeKind {
    /// The stable text for logs and audit.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confusable => "confusable",
            Self::EditDistance => "edit-distance-1",
        }
    }
}

/// The known name a candidate looks like, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookalike {
    /// The known name, as given.
    pub known: String,
    /// What made them alike.
    pub kind: LookalikeKind,
}

/// The letters drawn like a Latin one that this judgement folds. A short list
/// on purpose: it covers the Cyrillic and Greek homoglyphs of lowercase ASCII
/// that registries have seen abused; it is not a claim of full UTS #39 cover.
fn fold_char(character: char) -> Option<char> {
    Some(match character {
        // Cyrillic.
        'а' => 'a',
        'е' | 'ё' => 'e',
        'о' => 'o',
        'р' => 'p',
        'с' => 'c',
        'х' => 'x',
        'у' => 'y',
        'і' | 'ї' => 'i',
        'ј' => 'j',
        'ѕ' => 's',
        'ԁ' => 'd',
        'ԛ' => 'q',
        'ԝ' => 'w',
        'һ' => 'h',
        'ӏ' => 'l',
        'ν' => 'v',
        // Greek.
        'α' => 'a',
        'ο' => 'o',
        'ρ' => 'p',
        'ι' => 'i',
        'κ' => 'k',
        'τ' => 't',
        'υ' => 'u',
        // Latin look-alikes.
        'ɡ' => 'g',
        'ӓ' => 'a',
        'ı' => 'i',
        'ℓ' => 'l',
        // Digits drawn like letters.
        '0' => 'o',
        '1' => 'l',
        _ => return None,
    })
}

/// Whether the character is invisible: it adds nothing the eye can read.
fn is_invisible(character: char) -> bool {
    matches!(
        character,
        '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}' | '\u{180E}'
    )
}

/// The skeleton of a name: what is left when everything the eye does not tell
/// apart is folded away. Two names with the same skeleton look the same.
pub fn skeleton(ecosystem: Ecosystem, name: &str) -> String {
    let normalized = normalize_name(ecosystem, name);
    let mut folded = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        if is_invisible(character) {
            continue;
        }
        // Full-width ASCII (U+FF01..U+FF5E) is the ASCII character plus 0xFEE0.
        let character = match character {
            '\u{FF01}'..='\u{FF5E}' => {
                char::from_u32(character as u32 - 0xFEE0).unwrap_or(character)
            }
            other => other,
        };
        let character = character.to_lowercase().next().unwrap_or(character);
        let character = fold_char(character).unwrap_or(character);
        // Separators carry no identity for the eye: `react-dom` is `reactdom`.
        if matches!(character, '-' | '_' | '.') {
            continue;
        }
        folded.push(character);
    }
    folded.replace("rn", "m").replace("vv", "w")
}

/// The first known name that `candidate` looks like, if any.
///
/// Deterministic: confusable matches win over edit-distance ones, and among
/// equals the lexicographically smallest known name is reported, whatever the
/// order of `known`. A known name that normalizes to the candidate itself is
/// skipped.
pub fn find_lookalike<'a>(
    ecosystem: Ecosystem,
    candidate: &str,
    known: impl IntoIterator<Item = &'a str>,
) -> Option<Lookalike> {
    let normalized = normalize_name(ecosystem, candidate);
    let candidate_skeleton = skeleton(ecosystem, candidate);
    let candidate_len = normalized.chars().count();
    let mut best: Option<Lookalike> = None;
    for other in known {
        let other_normalized = normalize_name(ecosystem, other);
        if other_normalized == normalized {
            continue;
        }
        let kind = if skeleton(ecosystem, other) == candidate_skeleton {
            Some(LookalikeKind::Confusable)
        } else if candidate_len >= MIN_LEN_FOR_EDIT_DISTANCE
            && other_normalized.chars().count() >= MIN_LEN_FOR_EDIT_DISTANCE
            && edit_distance_within(&other_normalized, &normalized, 1).is_some()
        {
            Some(LookalikeKind::EditDistance)
        } else {
            None
        };
        let Some(kind) = kind else { continue };
        let better = match &best {
            None => true,
            Some(current) => {
                let rank = |kind: LookalikeKind| match kind {
                    LookalikeKind::Confusable => 0,
                    LookalikeKind::EditDistance => 1,
                };
                (rank(kind), other) < (rank(current.kind), current.known.as_str())
            }
        };
        if better {
            best = Some(Lookalike {
                known: other.to_string(),
                kind,
            });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alike(candidate: &str, known: &[&str]) -> Option<LookalikeKind> {
        find_lookalike(Ecosystem::Npm, candidate, known.iter().copied()).map(|found| found.kind)
    }

    #[test]
    fn a_transposition_and_the_single_edits_are_squats() {
        assert_eq!(
            alike("raect", &["react"]),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            alike("loadsh", &["lodash"]),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            alike("expres", &["express"]),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            alike("expresss", &["express"]),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            alike("exprass", &["express"]),
            Some(LookalikeKind::EditDistance)
        );
    }

    #[test]
    fn two_edits_away_is_a_different_package() {
        assert_eq!(alike("expreess1", &["express"]), None);
        assert_eq!(alike("lodashes", &["lodash"]), None);
        assert_eq!(alike("completely-other", &["react"]), None);
    }

    #[test]
    fn the_candidate_never_matches_itself() {
        assert_eq!(alike("react", &["react"]), None);
        // Normalization makes these the same name, not a look-alike.
        assert_eq!(alike("React", &["react"]), None);
        assert_eq!(alike("left_pad", &["left-pad"]), None);
    }

    #[test]
    fn separators_digits_and_digraphs_fold_into_the_skeleton() {
        assert_eq!(
            alike("reactdom", &["react-dom"]),
            Some(LookalikeKind::Confusable)
        );
        assert_eq!(
            alike("react.dom", &["reactdom"]),
            Some(LookalikeKind::Confusable)
        );
        assert_eq!(
            alike("l0dash", &["lodash"]),
            Some(LookalikeKind::Confusable)
        );
        assert_eq!(
            alike("1odash", &["lodash"]),
            Some(LookalikeKind::Confusable)
        );
        assert_eq!(
            alike("rnoment", &["moment"]),
            Some(LookalikeKind::Confusable)
        );
        assert_eq!(
            alike("vvebpack", &["webpack"]),
            Some(LookalikeKind::Confusable)
        );
    }

    #[test]
    fn unicode_homoglyphs_and_invisible_characters_are_confusable() {
        // Cyrillic `а` (U+0430) and `е` (U+0435).
        assert_eq!(alike("rеаct", &["react"]), Some(LookalikeKind::Confusable));
        // Greek omicron (U+03BF).
        assert_eq!(
            alike("l\u{3bf}dash", &["lodash"]),
            Some(LookalikeKind::Confusable)
        );
        // Full-width letters.
        assert_eq!(
            alike("\u{FF52}\u{FF45}\u{FF41}\u{FF43}\u{FF54}", &["react"]),
            Some(LookalikeKind::Confusable)
        );
        // Zero-width space in the middle.
        assert_eq!(
            alike("re\u{200B}act", &["react"]),
            Some(LookalikeKind::Confusable)
        );
    }

    #[test]
    fn short_names_are_only_compared_by_skeleton() {
        // One edit between two short names is another package.
        assert_eq!(alike("abc", &["abd"]), None);
        assert_eq!(alike("ab", &["ac"]), None);
        // But the skeleton still catches a homoglyph on a short name.
        assert_eq!(
            alike("a\u{43e}b", &["aob"]),
            Some(LookalikeKind::Confusable)
        );
        // The boundary: four characters on both sides, edit distance applies.
        assert_eq!(
            alike("vuex", &["vue"]),
            None,
            "three-letter known is below the floor"
        );
        assert_eq!(alike("reac", &["read"]), Some(LookalikeKind::EditDistance));
    }

    #[test]
    fn a_confusable_wins_and_the_choice_is_independent_of_the_order() {
        let forward = find_lookalike(Ecosystem::Npm, "reactdom", ["react-don", "react-dom"]);
        let backward = find_lookalike(Ecosystem::Npm, "reactdom", ["react-dom", "react-don"]);
        assert_eq!(forward, backward);
        assert_eq!(forward.unwrap().known, "react-dom");
    }

    #[test]
    fn scoped_names_compare_whole() {
        assert_eq!(
            find_lookalike(Ecosystem::Npm, "@acme/utils", ["@acme/utlis"]).map(|f| f.kind),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            find_lookalike(Ecosystem::Npm, "@acme/utils", ["@other/zzzzz"]),
            None
        );
    }

    #[test]
    fn pypi_normalization_applies_before_the_comparison() {
        assert_eq!(
            find_lookalike(Ecosystem::Pypi, "Flask_Logn", ["flask-login"]).map(|f| f.kind),
            Some(LookalikeKind::EditDistance)
        );
        assert_eq!(
            find_lookalike(Ecosystem::Pypi, "Flask.Login", ["flask-login"]),
            None
        );
    }

    #[test]
    fn no_known_names_means_no_lookalike() {
        assert_eq!(alike("anything", &[]), None);
    }
}
