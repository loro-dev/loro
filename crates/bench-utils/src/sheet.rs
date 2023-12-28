use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary, PartialEq, Eq)]
pub enum SheetAction {
    SetValue {
        row: usize,
        col: usize,
        value: usize,
    },
    InsertRow {
        row: usize,
    },
    InsertCol {
        col: usize,
    },
}

impl SheetAction {
    pub const MAX_ROW: usize = 1_048_576;
    pub const MAX_COL: usize = 16_384;
    /// Excel has a limit of 1,048,576 rows and 16,384 columns per sheet.
    // We need to normalize the action to fit the limit.
    pub fn normalize(&mut self) {
        match self {
            SheetAction::SetValue { row, col, .. } => {
                *row %= Self::MAX_ROW;
                *col %= Self::MAX_COL;
            }
            SheetAction::InsertRow { row } => {
                *row %= Self::MAX_ROW;
            }
            SheetAction::InsertCol { col } => {
                *col %= Self::MAX_COL;
            }
        }
    }
}
