use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub const PREFIX: &str = "tbxph_";

#[derive(Debug, PartialEq)]
enum Scheme {
    Basic { user: String },
    Word(String),
    Bare,
}

#[derive(Debug, PartialEq)]
pub struct Found {
    pub placeholder: String,
    scheme: Scheme,
}

pub fn find(header: &str) -> Option<Found> {
    let header = header.trim();
    match header.split_once(' ') {
        Some((word, rest)) if word.eq_ignore_ascii_case("basic") => {
            let decoded = STANDARD.decode(rest.trim()).ok()?;
            let text = String::from_utf8(decoded).ok()?;
            let (user, password) = text.split_once(':')?;
            let scheme = Scheme::Basic {
                user: user.to_string(),
            };
            placeholder(password).map(|placeholder| Found {
                placeholder,
                scheme,
            })
        }
        Some((word, rest)) => placeholder(rest.trim()).map(|placeholder| Found {
            placeholder,
            scheme: Scheme::Word(word.to_string()),
        }),
        None => placeholder(header).map(|placeholder| Found {
            placeholder,
            scheme: Scheme::Bare,
        }),
    }
}

fn placeholder(text: &str) -> Option<String> {
    let rest = text.strip_prefix(PREFIX)?;
    let valid = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    (!rest.is_empty() && rest.bytes().all(valid)).then(|| text.to_string())
}

pub fn fill(found: &Found, value: &str) -> String {
    match &found.scheme {
        Scheme::Basic { user } => format!("Basic {}", STANDARD.encode(format!("{user}:{value}"))),
        Scheme::Word(word) => format!("{word} {value}"),
        Scheme::Bare => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACEHOLDER: &str = "tbxph_Ab3-x_9";

    fn basic(user: &str, password: &str) -> String {
        format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
    }

    #[test]
    fn git_basic_gets_the_value_as_password_and_keeps_the_user() {
        let header = basic("x-access-token", PLACEHOLDER);
        let found = find(&header).expect("a placeholder");
        assert_eq!(found.placeholder, PLACEHOLDER);
        assert_eq!(
            fill(&found, "ghs_real"),
            basic("x-access-token", "ghs_real")
        );
    }

    #[test]
    fn gh_token_and_bearer_keep_their_word() {
        let token = find(&format!("token {PLACEHOLDER}")).expect("a placeholder");
        assert_eq!(fill(&token, "ghs_real"), "token ghs_real");
        let bearer = find(&format!("Bearer {PLACEHOLDER}")).expect("a placeholder");
        assert_eq!(fill(&bearer, "ghs_real"), "Bearer ghs_real");
    }

    #[test]
    fn a_bare_tracker_key_is_the_value_alone() {
        let found = find(PLACEHOLDER).expect("a placeholder");
        assert_eq!(fill(&found, "lin_api_real"), "lin_api_real");
    }

    #[test]
    fn a_header_without_a_placeholder_is_left_alone() {
        assert_eq!(find("token ghp_someone_else"), None);
        assert_eq!(find(&basic("x-access-token", "ghs_real")), None);
        assert_eq!(find("Basic not-base64!"), None);
        assert_eq!(find("tbxph_"), None);
        assert_eq!(find("tbxph_a b"), None);
        assert_eq!(find("tbxph_a/b"), None);
    }

    #[test]
    fn a_placeholder_as_the_basic_user_is_not_taken() {
        assert_eq!(find(&basic(PLACEHOLDER, "x")), None);
    }
}
