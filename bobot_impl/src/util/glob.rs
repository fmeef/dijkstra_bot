use std::fmt;

/// Wildcard matcher used to match strings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WildMatch {
    pattern: Vec<State>,
    max_questionmarks: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct State {
    next_char: Option<char>,
    has_wildcard: bool,
}

impl fmt::Display for WildMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::fmt::Write;

        for state in &self.pattern {
            if state.has_wildcard {
                f.write_char('*')?;
            }
            if let Some(c) = state.next_char {
                f.write_char(c)?;
            }
        }
        Ok(())
    }
}

impl WildMatch {
    /// Constructor with pattern which can be used for matching.
    pub fn new(pattern: &str) -> WildMatch {
        let mut simplified: Vec<State> = Vec::with_capacity(pattern.len());
        let mut prev_was_star = false;
        let mut max_questionmarks: usize = 0;
        let mut questionmarks: usize = 0;
        for current_char in pattern.chars() {
            match current_char {
                '*' => {
                    prev_was_star = true;
                    max_questionmarks = std::cmp::max(max_questionmarks, questionmarks);
                    questionmarks = 0;
                }
                _ => {
                    if current_char == '?' {
                        questionmarks += 1;
                    }
                    let s = State {
                        next_char: Some(current_char),
                        has_wildcard: prev_was_star,
                    };
                    simplified.push(s);
                    prev_was_star = false;
                }
            }
        }

        if !pattern.is_empty() {
            let final_state = State {
                next_char: None,
                has_wildcard: prev_was_star,
            };
            simplified.push(final_state);
        }

        WildMatch {
            pattern: simplified,
            max_questionmarks,
        }
    }

    #[deprecated(since = "2.0.0", note = "use `matches` instead")]
    pub fn is_match(&self, input: &str) -> bool {
        self.matches(input)
    }

    /// Returns true if pattern applies to the given input string
    pub fn matches(&self, input: &str) -> bool {
        if self.pattern.is_empty() {
            return input.is_empty();
        }
        let mut pattern_idx = 0;
        const NONE: usize = usize::MAX;
        let mut last_wildcard_idx = NONE;
        let mut questionmark_matches: Vec<char> = Vec::with_capacity(self.max_questionmarks);

        for input_char in input.chars() {
            if self.pattern[pattern_idx].next_char.is_none() {
                return true;
            }
            match self.pattern.get(pattern_idx) {
                None => {
                    return true;
                }
                Some(p) if p.next_char == Some('?') => {
                    if p.has_wildcard {
                        last_wildcard_idx = pattern_idx;
                    }
                    pattern_idx += 1;
                    if !input_char.is_whitespace() {
                        questionmark_matches.push(input_char);
                    }
                }
                Some(p) if p.next_char == Some(input_char) => {
                    if p.has_wildcard {
                        last_wildcard_idx = pattern_idx;
                        questionmark_matches.clear();
                    }
                    pattern_idx += 1;
                }
                Some(p) if p.has_wildcard => {
                    if input_char.is_whitespace() {
                        pattern_idx += 1;
                    } else if p.next_char == None {
                        return true;
                    }
                }
                _ => {
                    if last_wildcard_idx == NONE {
                        return false;
                    }
                    if !questionmark_matches.is_empty() {
                        // Try to match a different set for questionmark
                        let mut questionmark_idx = 0;
                        let current_idx = pattern_idx;
                        pattern_idx = last_wildcard_idx;
                        for prev_state in self.pattern[last_wildcard_idx + 1..current_idx].iter() {
                            if self.pattern[pattern_idx].next_char == Some('?') {
                                pattern_idx += 1;
                                continue;
                            }
                            let mut prev_input_char = prev_state.next_char;
                            if prev_input_char == Some('?') {
                                prev_input_char = Some(questionmark_matches[questionmark_idx]);
                                questionmark_idx += 1;
                            }
                            if self.pattern[pattern_idx].next_char == prev_input_char {
                                pattern_idx += 1;
                            } else {
                                pattern_idx = last_wildcard_idx;
                                questionmark_matches.clear();
                                break;
                            }
                        }
                    } else {
                        // Directly go back to the last wildcard
                        pattern_idx = last_wildcard_idx;
                    }

                    // Match last char again
                    if self.pattern[pattern_idx].next_char == Some('?')
                        || self.pattern[pattern_idx].next_char == Some(input_char)
                    {
                        pattern_idx += 1;
                    }
                }
            }
        }
        false
    }
}

impl<'a> PartialEq<&'a str> for WildMatch {
    fn eq(&self, &other: &&'a str) -> bool {
        self.matches(other)
    }
}

pub struct Glob(Vec<char>);

impl Glob {
    pub fn new(pattern: &str) -> Self {
        Self(pattern.chars().collect())
    }

    pub fn is_match(&self, m: &str) -> bool {
        let mut pattern_idx = 0;
        let mut is_star = false;
        for ch in m.chars() {
            if ch.is_whitespace() {
                is_star = false;
            }
            match self.0.get(pattern_idx) {
                Some('?') => {
                    if !ch.is_whitespace() {
                        pattern_idx += 1;
                    }
                }
                Some('*') => {
                    is_star = true;
                    pattern_idx += 1;
                }
                Some(c) => {
                    if *c == ch {
                        pattern_idx += 1;
                    } else if !is_star {
                        pattern_idx = 0;
                    }
                }
                None => {
                    return true;
                }
            };
        }
        false
    }
}

mod tests {
    use super::Glob;

    #[test]
    fn test_star() {
        let s = "thing*many";
        let glob = Glob::new(s);
        assert!(glob.is_match("doof thingsomany fue"));
        assert!(!glob.is_match("blarg boof"));
    }
}
