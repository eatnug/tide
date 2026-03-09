// Line-level diff algorithm using LCS (Longest Common Subsequence).

/// A single diff operation.
#[derive(Debug, Clone)]
pub enum DiffOp {
    /// Line exists in both disk and buffer (unchanged).
    Equal(usize),      // buffer line index
    /// Line exists only in buffer (added).
    Insert(usize),     // buffer line index
    /// Line exists only on disk (deleted).
    Delete(usize),     // disk line index
}

/// Compute a line-level unified diff between disk content and buffer content.
/// Returns a list of DiffOps representing how to display the diff.
pub fn compute_diff(disk: &[String], buffer: &[String]) -> Vec<DiffOp> {
    let n = disk.len();
    let m = buffer.len();

    // Build LCS table
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if disk[i - 1] == buffer[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce diff ops
    let mut ops = Vec::new();
    let mut i = n;
    let mut j = m;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && disk[i - 1] == buffer[j - 1] {
            ops.push(DiffOp::Equal(j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Insert(j - 1));
            j -= 1;
        } else {
            ops.push(DiffOp::Delete(i - 1));
            i -= 1;
        }
    }

    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_inputs() {
        let ops = compute_diff(&[], &[]);
        assert!(ops.is_empty());
    }

    #[test]
    fn identical_inputs() {
        let lines = s(&["a", "b", "c"]);
        let ops = compute_diff(&lines, &lines);
        assert_eq!(ops.len(), 3);
        assert!(ops.iter().all(|op| matches!(op, DiffOp::Equal(_))));
    }

    #[test]
    fn all_inserted() {
        let ops = compute_diff(&[], &s(&["x", "y"]));
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], DiffOp::Insert(0)));
        assert!(matches!(ops[1], DiffOp::Insert(1)));
    }

    #[test]
    fn all_deleted() {
        let ops = compute_diff(&s(&["a", "b"]), &[]);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], DiffOp::Delete(0)));
        assert!(matches!(ops[1], DiffOp::Delete(1)));
    }

    #[test]
    fn mixed_changes() {
        let disk = s(&["a", "b", "c", "d"]);
        let buffer = s(&["a", "x", "c", "d", "e"]);
        let ops = compute_diff(&disk, &buffer);

        // Expected: Equal(a), Delete(b), Insert(x), Equal(c), Equal(d), Insert(e)
        let equals = ops.iter().filter(|op| matches!(op, DiffOp::Equal(_))).count();
        let inserts = ops.iter().filter(|op| matches!(op, DiffOp::Insert(_))).count();
        let deletes = ops.iter().filter(|op| matches!(op, DiffOp::Delete(_))).count();
        assert_eq!(equals, 3); // a, c, d
        assert_eq!(deletes, 1); // b
        assert_eq!(inserts, 2); // x, e
    }

    #[test]
    fn single_line_replacement() {
        let disk = s(&["hello"]);
        let buffer = s(&["world"]);
        let ops = compute_diff(&disk, &buffer);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], DiffOp::Delete(0)));
        assert!(matches!(ops[1], DiffOp::Insert(0)));
    }
}
