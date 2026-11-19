//! Small fuzzy matcher for the command palette.

const WORD_START: i32 = 8;
const CONSECUTIVE: i32 = 5;
const MAX_GAP_PENALTY: i32 = 10;

/// Scores `candidate` against `query` (case-insensitive subsequence match).
/// Higher is better; `None` if not all query characters appear in order.
///
/// Finds the best alignment (dynamic programming), rewarding matches at word
/// starts ("spd" → **S**plit **P**ane **D**own) and consecutive runs.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(lower)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.chars().collect();
    let lc: Vec<char> = cand.iter().copied().map(lower).collect();
    let n = cand.len();

    // best[j]: best score with the current query char matched at position j.
    let mut prev: Vec<Option<i32>> = vec![None; n];
    let mut cur: Vec<Option<i32>> = vec![None; n];
    for (i, &q) in query.iter().enumerate() {
        for j in 0..n {
            cur[j] = None;
            if lc[j] != q {
                continue;
            }
            let word_start = j == 0 || !cand[j - 1].is_alphanumeric();
            let base = 1 + if word_start { WORD_START } else { 0 };
            if i == 0 {
                cur[j] = Some(base - (j as i32).min(MAX_GAP_PENALTY));
                continue;
            }
            cur[j] = (0..j)
                .filter_map(|k| {
                    let s = prev[k]?;
                    let gap = (j - k - 1) as i32;
                    let bonus = if gap == 0 { CONSECUTIVE } else { 0 };
                    Some(s + base + bonus - gap.min(MAX_GAP_PENALTY))
                })
                .max();
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let best = prev.into_iter().flatten().max()?;
    // Prefer shorter candidates on ties.
    Some(best * 100 - n as i32)
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequences() {
        assert!(score("nt", "New Tab").is_some());
        assert!(score("spr", "Split Right").is_some());
        assert!(score("xyz", "New Tab").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn word_starts_rank_higher() {
        let a = score("sr", "Split Right").unwrap();
        let b = score("sr", "Scroll Page Up").unwrap();
        assert!(a > b, "{a} vs {b}");
    }

    #[test]
    fn acronyms_pick_the_best_alignment() {
        // A greedy matcher would take the "p" of "Split" and lose to "Scroll Page Down".
        let split = score("spd", "Split Pane Down").unwrap();
        let scroll = score("spd", "Scroll Page Down").unwrap();
        assert!(split > scroll, "{split} vs {scroll}");
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(score("NEW", "new tab"), score("new", "New Tab"));
    }
}
