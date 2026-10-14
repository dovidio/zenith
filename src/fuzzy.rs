//! A small subsequence matcher for the "go to file" palette.
//!
//! Deliberately not fzf: no ranking magic, no case-insensitive scoring tables. It
//! rewards matches at word boundaries and consecutive runs, and it always explains
//! itself by returning the matched byte positions so the UI can show *why* a file
//! was found.

#[derive(Clone, Debug)]
pub struct Hit {
    pub score: i32,
    /// Byte offsets of matched characters in the haystack.
    pub positions: Vec<usize>,
}

/// `hay` is a relative path; `needle` is what the reader typed.
pub fn match_path(hay: &str, needle: &str) -> Option<Hit> {
    if needle.is_empty() {
        return Some(Hit {
            score: 0,
            positions: Vec::new(),
        });
    }
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let hlow: Vec<char> = h.iter().flat_map(|c| c.to_lowercase()).collect();
    let nlow: Vec<char> = n.iter().flat_map(|c| c.to_lowercase()).collect();

    // Character-index based greedy match, then score.
    let mut positions: Vec<usize> = Vec::with_capacity(nlow.len());
    let mut hi = 0usize;
    for &nc in &nlow {
        let mut found = None;
        while hi < hlow.len() {
            if hlow[hi] == nc {
                found = Some(hi);
                hi += 1;
                break;
            }
            hi += 1;
        }
        let idx = found?;
        positions.push(idx);
    }

    let basename_start = hay.rfind('/').map(|i| i + 1).unwrap_or(0);
    let basename_chars = hay[basename_start..].chars().count();
    let base_off = h.len() - basename_chars;

    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    for &p in &positions {
        // Consecutive characters are what the eye reads as "the same word".
        if prev == Some(p.wrapping_sub(1)) {
            score += 8;
        }
        let prev_ch = if p > 0 { h[p - 1] } else { '/' };
        if matches!(prev_ch, '/' | '_' | '-' | '.' | ' ' | '\\') {
            score += 12;
        }
        if p >= base_off && p == base_off {
            score += 10;
        }
        // Prefer matches in the file name over deep directory names.
        if p >= base_off {
            score += 4;
        }
        if n.len() == 1 {
            // Single letters are noise unless they start a word.
            if !matches!(prev_ch, '/' | '_' | '-' | '.' | ' ') {
                score -= 6;
            }
        }
        prev = Some(p);
    }
    // Shorter paths win ties; long paths cost a little.
    score -= (h.len() as i32) / 8;
    // Exact-ish basename match is almost always what was meant.
    if hlow[base_off..] == nlow[..] {
        score += 60;
    }
    score -= (positions.len() as i32 - n.len() as i32).max(0) * 2;

    Some(Hit { score, positions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_subsequences() {
        assert!(match_path("src/app.rs", "app").is_some());
        assert!(match_path("src/app.rs", "sr").is_some());
        assert!(match_path("src/app.rs", "zzz").is_none());
    }

    #[test]
    fn prefers_basenames() {
        let a = match_path("app/src/main.rs", "main").unwrap();
        let b = match_path("main/src/app.rs", "main").unwrap();
        assert!(a.score > b.score);
    }

    #[test]
    fn reports_positions() {
        // "main.rs": m at 0, then n at 3.
        let hit = match_path("main.rs", "mn").unwrap();
        assert_eq!(hit.positions, vec![0, 3]);
    }
}
