//! The rules for the path of a raw content.
//!
//! A path is what the client says, and what the client says is what
//! will end up on disk. That is why the rules are **strict** and
//! are applied before touching anything:
//!
//! - The path has to be under `/repository/`.
//! - The repository key has to be a name, not a path.
//! - Each segment has to be an RFC 3986 `pchar`: no slashes, no
//!   backslashes, no delimiters that change the meaning of
//!   the reference, no space, and no `.` and `..` segments.
//! - A segment is decoded **once**, and the decoded form has
//!   to be a valid segment: since the `%` symbol is not a `pchar`, a
//!   double-encoded one is a form that still carries encoding, and a
//!   form that still carries encoding is not a name.
//!
//! What is accepted is exactly what can be named without
//! ambiguity. A path that two systems interpret differently
//! is a path that one day is one file and another day a different one, and an
//! artifact registry that stores a path it cannot compare is a
//! registry with silent discrepancies.

/// What is rejected about a path, and why.
///
/// The errors are **distinct** on purpose: whoever receives a 400 knows
/// whether the path was not for a raw repository, whether the repository
/// name was not valid, or whether the problem was in a segment.
/// An error that says nothing is an error that has to be guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPathError {
    /// The path is not under `/repository/`.
    NotRaw,
    /// The repository name is not a name.
    InvalidRepository,
    /// There is no content path inside the repository.
    EmptyPath,
    /// The path is longer than what is admitted.
    TooLong,
    /// A segment is not a valid segment.
    InvalidSegment,
    /// The path is not properly encoded.
    BadEncoding,
}

impl RawPathError {
    /// What is told to the client.
    ///
    /// It does not say **where** the problem is in the path: a client that
    /// receives the error knows its path is not valid, and does not receive a
    /// description of how its path is written so it can try
    /// other variants.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotRaw => "the path is not a raw repository path",
            Self::InvalidRepository => "the name of the repository is not valid",
            Self::EmptyPath => "the path of the content is empty",
            Self::TooLong => "the path of the content is too long",
            Self::InvalidSegment => "the path of the content is not valid",
            Self::BadEncoding => "the path of the content is not correctly encoded",
        }
    }
}

/// Where a content lives: the repository and the path within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTarget {
    /// The repository key.
    pub repository: String,
    /// The path within the repository.
    pub asset_path: String,
}

/// The prefix of the raw content surface.
const RAW_PREFIX: &str = "/repository/";

/// How many segments a path has at most.
const MAX_SEGMENTS: usize = 64;

/// How many bytes a segment has at most.
const MAX_SEGMENT_BYTES: usize = 255;

/// How many bytes the whole path has at most.
const MAX_PATH_BYTES: usize = 1024;

/// The requested path, read from the raw URI.
///
/// The **raw** URI is the one that arrived, undecoded: if the decoded
/// one were used, a `%2e%2e` would already have become `..` and could be
/// neither seen nor refuted.
pub fn the_target(raw_path: &str) -> Result<RawTarget, RawPathError> {
    let rest = raw_path
        .strip_prefix(RAW_PREFIX)
        .ok_or(RawPathError::NotRaw)?;
    if rest.is_empty() {
        return Err(RawPathError::EmptyPath);
    }

    let raw_segments: Vec<&str> = rest.split('/').collect();
    let (raw_repository, raw_asset) = raw_segments.split_first().ok_or(RawPathError::EmptyPath)?;

    // The repository name is a **name**: it carries no slash, no
    // escaping, nothing that turns it into something else. A name does not
    // admit `%`, so there is nothing to decode: it is checked as
    // it arrives.
    if !valid_repository_name(raw_repository) {
        return Err(RawPathError::InvalidRepository);
    }
    let repository = raw_repository.to_string();

    // A trailing slash is not a segment: it is a path with no content.
    if raw_asset.len() == 1 && raw_asset[0].is_empty() {
        return Err(RawPathError::EmptyPath);
    }
    if raw_asset.len() > MAX_SEGMENTS {
        return Err(RawPathError::TooLong);
    }

    let mut segments = Vec::with_capacity(raw_asset.len());
    for raw_segment in raw_asset {
        let segment = decode_segment(raw_segment, valid_segment)?;
        segments.push(segment);
    }

    let asset_path = segments.join("/");
    if asset_path.len() > MAX_PATH_BYTES {
        return Err(RawPathError::TooLong);
    }

    Ok(RawTarget {
        repository,
        asset_path,
    })
}

/// A path segment: an RFC 3986 `pchar`.
///
/// These are the characters that can go in a URI segment without
/// encoding: unreserved ones, sub-delimiters, and `:` and `@`.
///
/// Neither slash nor backslash: both are separators in some
/// system, and a name that is a separator in one system is a
/// traversal in that system. Neither `#` nor `?`: they are the delimiters that
/// change the meaning of the reference, and a `Location` that points to
/// another resource is worse than a `Location` that does not exist. Nor the space:
/// the same name written in two ways is two names.
///
/// The list is not arbitrary: it is exactly the one that makes the path
/// an unambiguous **URI reference**.
fn valid_segment(segment: &str) -> bool {
    if segment.is_empty() || segment.len() > MAX_SEGMENT_BYTES {
        return false;
    }
    if segment == "." || segment == ".." {
        return false;
    }
    segment.chars().all(is_a_pchar)
}

/// An RFC 3986 `pchar`.
///
/// A `pchar` is an unreserved character, a sub-delimiter, `:` or
/// `@`. Unreserved ones are alphanumerics and `-._~`, and the
/// sub-delimiters are `!$&'()*+,;=`.
fn is_a_pchar(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '-' | '.'
                | '_'
                | '~'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | ':'
                | '@'
        )
}

/// A repository name.
///
/// The same criterion used by the administrative surface: a name is
/// a name, and a name with slashes is a disguised path.
pub fn valid_repository_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// Decodes a segment, and checks that it hides nothing.
///
/// A segment arrives **encoded**: `%2f` is a slash, `%20` is a
/// space and `%25` is a percent symbol. The name that is stored is the
/// **decoded** one, and that name has to be an entire `pchar`: no
/// slash, no space, no symbol that another system could
/// decode again.
///
/// It is decoded **once**, and only once. Since the `%` symbol is not a
/// `pchar`, a form that still carries encoding --a double-encoded
/// one-- is a form that is **not** a name, and it is rejected:
/// `%252e%252e%252f` decodes to `%2e%2e%2f`, and it stops there.
///
/// That is the rule: what is stored is always a literal name, and a
/// literal name cannot be read in two ways.
fn decode_segment(raw: &str, valid: fn(&str) -> bool) -> Result<String, RawPathError> {
    let decoded = decode_once(raw)?;
    if !valid(&decoded) {
        return Err(RawPathError::InvalidSegment);
    }
    Ok(decoded)
}

/// A percent-decoding pass.
///
/// Only `%XX` with two hexadecimal digits is accepted. The `+` of a
/// query string is **not** a space here: in a path it is just another
/// character.
fn decode_once(text: &str) -> Result<String, RawPathError> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1).ok_or(RawPathError::BadEncoding)?;
            let low = *bytes.get(index + 2).ok_or(RawPathError::BadEncoding)?;
            let high = hex_value(high).ok_or(RawPathError::BadEncoding)?;
            let low = hex_value(low).ok_or(RawPathError::BadEncoding)?;
            out.push(high * 16 + low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| RawPathError::BadEncoding)
}

/// The value of a hexadecimal digit.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shortcut to read the result as error text.
    fn the_error(path: &str) -> RawPathError {
        the_target(path).expect_err("the path is refused")
    }

    #[test]
    fn a_well_formed_path_is_read() {
        let target = the_target("/repository/releases/path/to/file.bin").expect("the path is read");
        assert_eq!(target.repository, "releases");
        assert_eq!(target.asset_path, "path/to/file.bin");
    }

    #[test]
    fn a_path_outside_the_raw_surface_is_refused() {
        assert_eq!(the_error("/api/v1/repositories"), RawPathError::NotRaw);
        assert_eq!(the_error("/repository"), RawPathError::NotRaw);
    }

    #[test]
    fn the_traversal_is_refused() {
        assert_eq!(
            the_error("/repository/releases/../etc/passwd"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a/../../b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/./a"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_double_decoding_is_refused() {
        // `%252e%252e%252f` is `../` with an extra layer.
        assert_eq!(
            the_error("/repository/releases/%252e%252e%252fetc"),
            RawPathError::InvalidSegment
        );
        // And the encoded slash too: a segment that when decoded
        // brings a slash is two segments, and the client did not ask for that.
        assert_eq!(
            the_error("/repository/releases/a%2fb"),
            RawPathError::InvalidSegment
        );
        // And the slash encoded twice: `a%252fb` is `a/b` with an
        // extra layer. The intermediate form (`a%2fb`) is a valid
        // segment, and that is why the segment has to check **each**
        // form, not just the last one.
        assert_eq!(
            the_error("/repository/releases/a%252fb"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_control_characters_are_refused() {
        assert_eq!(
            the_error("/repository/releases/a%00b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%09b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_backslash_is_refused() {
        assert_eq!(
            the_error("/repository/releases/a%5cb"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_ambiguous_unicode_is_refused() {
        // A path with non-ASCII characters can be written in
        // several ways, and not all of them say the same thing.
        assert_eq!(
            the_error("/repository/releases/caf%C3%A9"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_bad_encoding_is_refused() {
        assert_eq!(
            the_error("/repository/releases/a%zzb"),
            RawPathError::BadEncoding
        );
        assert_eq!(
            the_error("/repository/releases/a%2"),
            RawPathError::BadEncoding
        );
    }

    #[test]
    fn the_empty_path_is_refused() {
        assert_eq!(the_error("/repository/releases/"), RawPathError::EmptyPath);
    }

    #[test]
    fn the_invalid_repository_name_is_refused() {
        assert_eq!(
            the_error("/repository/rel%2Fases/a"),
            RawPathError::InvalidRepository
        );
        assert_eq!(
            the_error("/repository/re leases/a"),
            RawPathError::InvalidRepository
        );
    }

    #[test]
    fn the_too_long_path_is_refused() {
        let long = "a".repeat(300);
        assert_eq!(
            the_error(&format!("/repository/releases/{long}")),
            RawPathError::InvalidSegment
        );
        let many = vec!["a"; 70].join("/");
        assert_eq!(
            the_error(&format!("/repository/releases/{many}")),
            RawPathError::TooLong
        );
    }

    #[test]
    fn the_encoded_space_is_refused() {
        // The space is not a valid printable ASCII character in a
        // content path: the same name written in two ways is
        // two names.
        assert_eq!(
            the_error("/repository/releases/a%20b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_delimiters_that_change_the_meaning_are_refused() {
        // A `#` or a `?` in a segment makes the path something else
        // when read as a reference: `/repository/repos/a?b` is
        // `/repository/repos/a` with a query. And a `Location` that
        // points to another resource is worse than a `Location` that does not exist.
        assert_eq!(
            the_error("/repository/releases/a%23b"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%3fb"),
            RawPathError::InvalidSegment
        );
        // Brackets are only valid in a URI's host.
        assert_eq!(
            the_error("/repository/releases/a%5Bb"),
            RawPathError::InvalidSegment
        );
        assert_eq!(
            the_error("/repository/releases/a%5Db"),
            RawPathError::InvalidSegment
        );
        // And quotes are not a `pchar`.
        assert_eq!(
            the_error("/repository/releases/a%22b"),
            RawPathError::InvalidSegment
        );
    }

    #[test]
    fn the_characters_of_a_uri_segment_are_accepted() {
        // RFC 3986 `pchar`s: the ones a client really writes.
        let target = the_target("/repository/releases/informe+final_v2.1@2024.tar.gz")
            .expect("la ruta se lee");
        assert_eq!(target.asset_path, "informe+final_v2.1@2024.tar.gz");
    }

    #[test]
    fn an_encoded_character_is_the_character_it_encodes() {
        // `%41` is `A`: what is stored is the literal name, and there are not
        // two names for the same thing. A client that encodes what does not
        // need encoding is not asking for something else.
        let target = the_target("/repository/releases/a%41b").expect("la ruta se lee");
        assert_eq!(target.asset_path, "aAb");
        // And a double-encoded one is not a name: `%2541` decodes to
        // `%41`, which still carries the encoding inside.
        assert_eq!(
            the_error("/repository/releases/a%2541b"),
            RawPathError::InvalidSegment
        );
    }
}
