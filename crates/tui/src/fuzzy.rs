//! Fuzzy matching for the jump palette: every query character must appear in order;
//! consecutive runs, word starts and early matches score higher. Also returns the
//! matched positions so they can be highlighted.

/// `Some((score, char positions))` when `query` matches `text`, higher is better.
pub fn score(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    let q: Vec<char> = query
        .chars()
        .flat_map(|c| c.to_lowercase())
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some((0, Vec::new()));
    }
    let t_orig: Vec<char> = text.chars().collect();
    let t: Vec<char> = t_orig
        .iter()
        .flat_map(|c| c.to_lowercase().next())
        .collect();
    if t.len() != t_orig.len() || q.len() > t.len() {
        return None;
    }
    // Cheap subsequence test first.
    let mut it = t.iter();
    if !q.iter().all(|qc| it.any(|tc| tc == qc)) {
        return None;
    }

    let (m, n) = (q.len(), t.len());
    let boundary = |j: usize| {
        j == 0
            || matches!(t_orig[j - 1], '/' | '-' | '_' | '.' | ' ' | '\\')
            || (t_orig[j].is_uppercase() && t_orig[j - 1].is_lowercase())
    };
    let bonus = |j: usize| 16 + if boundary(j) { 14 } else { 0 } + if j == 0 { 6 } else { 0 };

    const NONE: i64 = i64::MIN / 4;
    let mut dp = vec![vec![NONE; n]; m];
    let mut from = vec![vec![usize::MAX; n]; m];
    for j in 0..n {
        if t[j] == q[0] {
            dp[0][j] = bonus(j) - j as i64 / 2;
        }
    }
    for i in 1..m {
        // best[k] = max over k' <= k of dp[i-1][k'] + k'  (a gap of g chars costs g)
        let mut best = NONE;
        let mut best_k = usize::MAX;
        for j in i..n {
            let k = j - 1;
            if dp[i - 1][k] > NONE && dp[i - 1][k] + k as i64 > best {
                best = dp[i - 1][k] + k as i64;
                best_k = k;
            }
            if t[j] != q[i] || best == NONE {
                continue;
            }
            // Either extend the run that ends right before j, or jump from the best earlier match.
            let jump = best - j as i64 + 1 - 2; // linear gap penalty plus a small cost for breaking the run
            let mut v = jump;
            let mut f = best_k;
            if dp[i - 1][k] > NONE && dp[i - 1][k] + 10 > v {
                v = dp[i - 1][k] + 10; // consecutive bonus
                f = k;
            }
            dp[i][j] = v + bonus(j);
            from[i][j] = f;
        }
    }
    let (mut j, mut s) = (usize::MAX, NONE);
    for (jj, &v) in dp[m - 1].iter().enumerate() {
        if v > s {
            s = v;
            j = jj;
        }
    }
    if s == NONE {
        return None;
    }
    let mut pos = vec![0; m];
    for i in (0..m).rev() {
        pos[i] = j;
        if i > 0 {
            j = from[i][j];
        }
    }
    // Shorter texts win ties: a match in "src" beats the same match in "src-backup-old".
    Some((s - (n as i64) / 8, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_required() {
        assert!(score("abc", "a-b-c").is_some());
        assert!(score("abc", "acb").is_none());
        assert!(score("", "anything").is_some());
        assert!(score("zz", "z").is_none());
    }

    #[test]
    fn better_matches_score_higher() {
        let s = |q: &str, t: &str| score(q, t).unwrap().0;
        assert!(
            s("doc", "Documents") > s("doc", "my-old-dustbin-of-c"),
            "prefix beats scattered"
        );
        assert!(
            s("dn", "downloads") > s("dn", "dxxxxxxxxxxxxxn"),
            "a closer match beats a scattered one"
        );
        assert!(s("src", "src") > s("src", "src-backup-old"));
        assert!(
            s("rad", "/home/a/rada") > s("rad", "/home/a/really/a/dir"),
            "word start and runs"
        );
    }

    #[test]
    fn positions_point_at_the_matched_characters() {
        let (_, pos) = score("dow", "Downloads").unwrap();
        assert_eq!(pos, vec![0, 1, 2]);
        let (_, pos) = score("pa", "projects/rada").unwrap();
        assert_eq!(pos.len(), 2);
        let text: Vec<char> = "projects/rada".chars().collect();
        assert_eq!((text[pos[0]], text[pos[1]]), ('p', 'a'));
        assert!(pos[1] > pos[0]);
    }

    #[test]
    fn unicode_and_case() {
        assert!(score("ÉCOLE", "école").is_some());
        assert!(score("日本", "日本語のフォルダ").is_some());
    }
}
