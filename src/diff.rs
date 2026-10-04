use similar::{ChangeTag, TextDiff};

/// What an aligned display row represents in one pane.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Present on both sides, identical.
    Equal,
    /// Real content that differs from the other side.
    Diff,
    /// Blank filler row with no real content, inserted purely so the OTHER side's
    /// corresponding `Diff` row stays on the same row index (this is what keeps the two
    /// panes visually aligned line-for-line, like git/Beyond Compare/Notepad++ Compare).
    Filler,
}

/// Line-aligned comparison of two documents: `left_display`/`right_display` are
/// synthetic, read-only texts with blank filler lines inserted so that `left_rows[i]`
/// and `right_rows[i]` always refer to the same logical row on both sides.
pub struct DiffResult {
    pub left_display: String,
    pub right_display: String,
    pub left_rows: Vec<RowKind>,
    pub right_rows: Vec<RowKind>,
    block_count: usize,
}

impl DiffResult {
    pub fn count(&self) -> usize {
        self.block_count
    }
}

fn line_text(change: &similar::Change<&str>) -> String {
    change
        .as_str()
        .unwrap_or("")
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_string()
}

pub fn diff_texts(left: &str, right: &str) -> DiffResult {
    let diff = TextDiff::from_lines(left, right);

    let mut left_lines: Vec<String> = Vec::new();
    let mut right_lines: Vec<String> = Vec::new();
    let mut left_rows: Vec<RowKind> = Vec::new();
    let mut right_rows: Vec<RowKind> = Vec::new();
    let mut block_count = 0usize;
    let mut in_block = false;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                let text = line_text(&change);
                left_lines.push(text.clone());
                right_lines.push(text);
                left_rows.push(RowKind::Equal);
                right_rows.push(RowKind::Equal);
                in_block = false;
            }
            ChangeTag::Delete => {
                left_lines.push(line_text(&change));
                right_lines.push(String::new());
                left_rows.push(RowKind::Diff);
                right_rows.push(RowKind::Filler);
                if !in_block {
                    block_count += 1;
                    in_block = true;
                }
            }
            ChangeTag::Insert => {
                left_lines.push(String::new());
                right_lines.push(line_text(&change));
                left_rows.push(RowKind::Filler);
                right_rows.push(RowKind::Diff);
                if !in_block {
                    block_count += 1;
                    in_block = true;
                }
            }
        }
    }

    DiffResult {
        left_display: left_lines.join("\n"),
        right_display: right_lines.join("\n"),
        left_rows,
        right_rows,
        block_count,
    }
}
