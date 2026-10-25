/// The most cells the line LCS table may hold (rows × columns of the differing middle). A
/// source is at most 256 KiB, so the table is what bounds the work: past this, the middle is
/// shown as one removed block and one added block instead.
pub const MAX_DIFF_CELLS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLine<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDiff<'a> {
    pub lines: Vec<DiffLine<'a>>,
    /// False when the middle was too large to align line by line.
    pub exact: bool,
}

impl LineDiff<'_> {
    pub fn changed(&self) -> bool {
        self.lines
            .iter()
            .any(|line| !matches!(line, DiffLine::Same(_)))
    }
}

/// Lines of `old` and `new` aligned by their longest common subsequence, after trimming the
/// common prefix and suffix.
pub fn line_diff<'a>(old: &'a str, new: &'a str) -> LineDiff<'a> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let prefix = old
        .iter()
        .zip(&new)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let (old_middle, new_middle) = (
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
    );
    let mut lines: Vec<DiffLine> = old[..prefix]
        .iter()
        .map(|line| DiffLine::Same(line))
        .collect();
    let exact = old_middle.len().saturating_mul(new_middle.len()) <= MAX_DIFF_CELLS;
    if exact {
        lines.extend(aligned(old_middle, new_middle));
    } else {
        lines.extend(old_middle.iter().map(|line| DiffLine::Removed(line)));
        lines.extend(new_middle.iter().map(|line| DiffLine::Added(line)));
    }
    lines.extend(
        old[old.len() - suffix..]
            .iter()
            .map(|line| DiffLine::Same(line)),
    );
    LineDiff { lines, exact }
}

fn aligned<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<DiffLine<'a>> {
    let columns = new.len() + 1;
    // lcs[i * columns + j]: the LCS length of old[i..] and new[j..].
    let mut lcs = vec![0_u32; (old.len() + 1) * columns];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i * columns + j] = if old[i] == new[j] {
                lcs[(i + 1) * columns + j + 1] + 1
            } else {
                lcs[(i + 1) * columns + j].max(lcs[i * columns + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::with_capacity(old.len() + new.len());
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            lines.push(DiffLine::Same(old[i]));
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * columns + j] >= lcs[i * columns + j + 1] {
            lines.push(DiffLine::Removed(old[i]));
            i += 1;
        } else {
            lines.push(DiffLine::Added(new[j]));
            j += 1;
        }
    }
    lines.extend(old[i..].iter().map(|line| DiffLine::Removed(line)));
    lines.extend(new[j..].iter().map(|line| DiffLine::Added(line)));
    lines
}
