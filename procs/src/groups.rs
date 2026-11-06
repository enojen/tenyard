//! The process groups: rules over the command line, read from the groups file.
//!
//! One rule per line: `<group> <sticky|open> <regex>`; `#` starts a comment. A process takes
//! the group of the first rule whose regex matches its command line. Below a sticky group every
//! descendant stays in it (a whole test run is `test`, also the git it calls). Below an open
//! group a descendant that matches a rule takes that rule's group, and one that matches none
//! stays in the parent's group (a tool command of claude is `claude`, its git call is `git`).
//! A process that matches nothing and has no grouped parent is `other`.

use regex::Regex;

pub const OTHER: &str = "other";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub sticky: bool,
}

impl Group {
    pub fn other() -> Group {
        Group {
            name: OTHER.to_string(),
            sticky: false,
        }
    }
}

#[derive(Debug)]
struct Rule {
    group: Group,
    pattern: Regex,
}

#[derive(Debug, Default)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    pub fn parse(text: &str) -> Result<Rules, String> {
        let mut rules = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fail = |why: &str| format!("line {}: {why}: {line}", index + 1);
            let (name, rest) = line
                .split_once(char::is_whitespace)
                .ok_or_else(|| fail("no kind"))?;
            // The name is a metric label value: lower-case letters, digits, `_` and `-`.
            if !name.starts_with(|c: char| c.is_ascii_lowercase())
                || !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                return Err(fail("group name is not a-z, 0-9, _ or -"));
            }
            let (kind, pattern) = rest
                .trim_start()
                .split_once(char::is_whitespace)
                .ok_or_else(|| fail("no regex"))?;
            let sticky = match kind {
                "sticky" => true,
                "open" => false,
                _ => return Err(fail("kind is not sticky or open")),
            };
            let pattern = Regex::new(pattern.trim()).map_err(|e| fail(&e.to_string()))?;
            rules.push(Rule {
                group: Group {
                    name: name.to_string(),
                    sticky,
                },
                pattern,
            });
        }
        Ok(Rules { rules })
    }

    /// The group of a process, from its command line and its parent's group.
    pub fn classify(&self, cmd: &str, parent: Option<&Group>) -> Group {
        if let Some(parent) = parent.filter(|p| p.sticky) {
            return parent.clone();
        }
        if let Some(rule) = self.rules.iter().find(|r| r.pattern.is_match(cmd)) {
            return rule.group.clone();
        }
        parent.cloned().unwrap_or_else(Group::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "
        # boot
        other  open    ^(\\S*/)?sudo( |$)
        claude open    ^(\\S*/)?claude( |$)
        test   sticky  \\bbun test\\b
        git    sticky  ^(\\S*/)?git( |$)
    ";

    fn group(name: &str, sticky: bool) -> Group {
        Group {
            name: name.to_string(),
            sticky,
        }
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let rules = Rules::parse(RULES).unwrap();
        assert_eq!(
            rules.classify("claude --bg -- bun test", None),
            group("claude", false)
        );
        assert_eq!(
            rules.classify("bash -c bun test sim", None),
            group("test", true)
        );
        assert_eq!(
            rules.classify("sudo -u tenbox bash -c bun test", None),
            group("other", false)
        );
    }

    #[test]
    fn below_a_sticky_group_every_process_stays_in_it() {
        let rules = Rules::parse(RULES).unwrap();
        let test = group("test", true);
        assert_eq!(rules.classify("git status", Some(&test)), test);
        assert_eq!(rules.classify("cat x", Some(&test)), test);
    }

    #[test]
    fn below_an_open_group_a_match_takes_its_own_group() {
        let rules = Rules::parse(RULES).unwrap();
        let claude = group("claude", false);
        assert_eq!(rules.classify("git log", Some(&claude)), group("git", true));
        assert_eq!(rules.classify("grep -r x", Some(&claude)), claude);
    }

    #[test]
    fn no_match_and_no_parent_is_other() {
        let rules = Rules::parse(RULES).unwrap();
        assert_eq!(rules.classify("/pause", None), Group::other());
    }

    #[test]
    fn a_bad_line_names_its_number() {
        let err = Rules::parse("test sticky \\bbun\nlint maybe x").unwrap_err();
        assert!(
            err.starts_with("line 2: kind is not sticky or open"),
            "{err}"
        );
        let err = Rules::parse("test sticky (").unwrap_err();
        assert!(err.starts_with("line 1:"), "{err}");
        let err = Rules::parse("te\"st sticky x").unwrap_err();
        assert!(err.starts_with("line 1: group name"), "{err}");
    }

    #[test]
    fn the_shipped_groups_file_parses() {
        Rules::parse(include_str!("../groups.conf")).unwrap();
    }
}
