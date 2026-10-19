use crate::term::BufWrite as _;

#[derive(Clone, Debug)]
pub struct Row {
    // a trimmed row (see trim) holds only its cells up to the last one that
    // isn't default: the rest are read as default cells up to cols
    cells: Vec<crate::Cell>,
    cols: u16,
    wrapped: bool,
    // whether a cell may have a hyperlink: set when one is written, and
    // cleared when the row is cleared or its cells are checked, so that rows
    // without links can be skipped
    hyperlinks: bool,
}

impl Row {
    pub fn new(cols: u16) -> Self {
        Self::blank(cols, crate::attrs::Attrs::default())
    }

    // a row of blank cells as an erase leaves them (see Cell::clear)
    pub fn blank(cols: u16, attrs: crate::attrs::Attrs) -> Self {
        Self {
            cells: vec![crate::Cell::blank(attrs); usize::from(cols)],
            cols,
            wrapped: false,
            hyperlinks: false,
        }
    }

    fn cols(&self) -> u16 {
        self.cols
    }

    // a copy of the row without the default cells at its end, keeping its
    // width: scrollback holds many rows and most of their cells are padding.
    // the cells read the same, and writing to the row puts them back (see
    // untrim).
    pub fn trimmed(&self) -> Self {
        let len = self
            .cells
            .iter()
            .rposition(|cell| cell != &crate::cell::DEFAULT)
            .map_or(0, |i| i + 1);
        Self {
            cells: self.cells[..len].to_vec(),
            ..*self
        }
    }

    fn untrim(&mut self) {
        self.cells
            .resize(usize::from(self.cols), crate::Cell::new());
    }

    // after a change to the number of cells, which only an untrimmed row
    // sees
    fn sync_cols(&mut self) {
        self.cols = self
            .cells
            .len()
            .try_into()
            // we limit the number of cols to a u16 (see Size)
            .unwrap();
    }

    // the cell at col, which must be within the row
    fn cell(&self, col: u16) -> &crate::Cell {
        self.get(col).unwrap()
    }

    pub fn clear(&mut self, attrs: crate::attrs::Attrs) {
        self.untrim();
        for cell in &mut self.cells {
            cell.clear(attrs);
        }
        self.wrapped = false;
        self.hyperlinks = false;
    }

    fn cells(&self) -> impl Iterator<Item = &crate::Cell> {
        self.cells.iter().chain(
            std::iter::repeat(&crate::cell::DEFAULT)
                .take(usize::from(self.cols) - self.cells.len()),
        )
    }

    pub fn get(&self, col: u16) -> Option<&crate::Cell> {
        if col >= self.cols {
            return None;
        }
        Some(
            self.cells
                .get(usize::from(col))
                .unwrap_or(&crate::cell::DEFAULT),
        )
    }

    pub fn get_mut(&mut self, col: u16) -> Option<&mut crate::Cell> {
        self.untrim();
        self.cells.get_mut(usize::from(col))
    }

    pub fn insert(&mut self, i: u16, cell: crate::Cell) {
        self.untrim();
        self.cells.insert(usize::from(i), cell);
        self.sync_cols();
        self.wrapped = false;
    }

    pub fn remove(&mut self, i: u16) {
        self.clear_wide(i);
        self.cells.remove(usize::from(i));
        self.sync_cols();
        self.wrapped = false;
    }

    pub fn erase(&mut self, i: u16, attrs: crate::attrs::Attrs) {
        self.untrim();
        let wide = self.cells[usize::from(i)].is_wide();
        self.clear_wide(i);
        self.cells[usize::from(i)].clear(attrs);
        // `wide` is captured pre-erase on purpose: a 2-wide char occupies the
        // logical last column at cols() - 2. saturating_sub guards the
        // degenerate case of an orphaned wide cell in a one-column row, where
        // cols() - 2 would underflow.
        if i == self.cols().saturating_sub(if wide { 2 } else { 1 }) {
            self.wrapped = false;
        }
    }

    pub fn truncate(&mut self, len: u16) {
        self.untrim();
        self.cells.truncate(usize::from(len));
        self.sync_cols();
        self.wrapped = false;
        let last_cell = &mut self.cells[usize::from(len) - 1];
        if last_cell.is_wide() {
            last_cell.clear(*last_cell.attrs());
        }
    }

    pub fn resize(&mut self, len: u16, cell: crate::Cell) {
        self.untrim();
        if usize::from(len) < self.cells.len() {
            self.truncate(len);
        } else {
            self.cells.resize(usize::from(len), cell);
            self.sync_cols();
            self.wrapped = false;
        }
    }

    pub fn wrap(&mut self, wrap: bool) {
        self.wrapped = wrap;
    }

    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    pub fn clear_wide(&mut self, col: u16) {
        self.untrim();
        let col_idx = usize::from(col);
        let cell = &self.cells[col_idx];
        let other = if cell.is_wide() {
            let next = col_idx + 1;
            if next >= self.cells.len() {
                // Wide char at the last column with no continuation
                // (e.g. a shrinking resize that split the pair).
                // Clear the cell in place instead of indexing past
                // the end.
                let attrs = *self.cells[col_idx].attrs();
                self.cells[col_idx].clear(attrs);
                return;
            }
            &mut self.cells[next]
        } else if cell.is_wide_continuation() {
            &mut self.cells[usize::from(col - 1)]
        } else {
            return;
        };
        other.clear(*other.attrs());
    }

    pub fn has_hyperlinks(&self) -> bool {
        self.hyperlinks
    }

    pub fn add_hyperlink(&mut self) {
        self.hyperlinks = true;
    }

    pub fn mark_hyperlinks(&mut self, in_use: &mut [bool]) {
        if !self.hyperlinks {
            return;
        }
        self.hyperlinks = false;
        for id in self.cells.iter().filter_map(crate::Cell::hyperlink) {
            in_use[usize::from(id.get())] = true;
            self.hyperlinks = true;
        }
    }

    pub fn forget_hyperlinks(&mut self, forgotten: &[bool]) {
        if !self.hyperlinks {
            return;
        }
        for cell in &mut self.cells {
            if cell
                .hyperlink()
                .is_some_and(|id| forgotten[usize::from(id.get())])
            {
                cell.set_hyperlink(None);
            }
        }
    }

    pub fn write_contents(
        &self,
        contents: &mut String,
        start: u16,
        width: u16,
        wrapping: bool,
    ) {
        let mut prev_was_wide = false;

        let mut prev_col = start;
        for (col, cell) in self
            .cells()
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            if cell.has_contents() {
                for _ in 0..(col - prev_col) {
                    contents.push(' ');
                }
                prev_col += col - prev_col;

                contents.push_str(cell.contents());
                prev_col += if cell.is_wide() { 2 } else { 1 };
            }
        }
        if prev_col == start && wrapping {
            contents.push('\n');
        }
    }

    pub fn write_contents_formatted(
        &self,
        contents: &mut Vec<u8>,
        start: u16,
        width: u16,
        row: u16,
        wrapping: bool,
        prev_pos: Option<crate::grid::Pos>,
        prev_attrs: Option<crate::attrs::Attrs>,
    ) -> (crate::grid::Pos, crate::attrs::Attrs) {
        let mut prev_was_wide = false;
        let default_cell = crate::Cell::new();

        let mut prev_pos = prev_pos.unwrap_or_else(|| {
            if wrapping {
                crate::grid::Pos {
                    row: row - 1,
                    col: self.cols(),
                }
            } else {
                crate::grid::Pos { row, col: start }
            }
        });
        let mut prev_attrs = prev_attrs.unwrap_or_default();

        let first_cell = self.cell(start);
        if wrapping && first_cell == &default_cell {
            let default_attrs = default_cell.attrs();
            if &prev_attrs != default_attrs {
                default_attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *default_attrs;
            }
            contents.push(b' ');
            crate::term::Backspace.write_buf(contents);
            crate::term::EraseChar::new(1).write_buf(contents);
            prev_pos = crate::grid::Pos { row, col: 0 };
        }

        let mut erase: Option<(u16, &crate::attrs::Attrs)> = None;
        for (col, cell) in self
            .cells()
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            let pos = crate::grid::Pos { row, col };

            if let Some((prev_col, attrs)) = erase {
                if cell.has_contents() || cell.attrs() != attrs {
                    let new_pos = crate::grid::Pos { row, col: prev_col };
                    if wrapping
                        && prev_pos.row + 1 == new_pos.row
                        && prev_pos.col >= self.cols()
                    {
                        if new_pos.col > 0 {
                            contents.extend(
                                " ".repeat(usize::from(new_pos.col))
                                    .as_bytes(),
                            );
                        } else {
                            contents.extend(b" ");
                            crate::term::Backspace.write_buf(contents);
                        }
                    } else {
                        crate::term::MoveFromTo::new(prev_pos, new_pos)
                            .write_buf(contents);
                    }
                    prev_pos = new_pos;
                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }
                    crate::term::EraseChar::new(pos.col - prev_col)
                        .write_buf(contents);
                    erase = None;
                }
            }

            if cell != &default_cell {
                let attrs = cell.attrs();
                if cell.has_contents() {
                    if pos != prev_pos {
                        if !wrapping
                            || prev_pos.row + 1 != pos.row
                            || prev_pos.col
                                < self.cols() - u16::from(cell.is_wide())
                            || pos.col != 0
                        {
                            crate::term::MoveFromTo::new(prev_pos, pos)
                                .write_buf(contents);
                        }
                        prev_pos = pos;
                    }

                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }

                    prev_pos.col += if cell.is_wide() { 2 } else { 1 };
                    let cell_contents = cell.contents();
                    contents.extend(cell_contents.as_bytes());
                } else if erase.is_none() {
                    erase = Some((pos.col, attrs));
                }
            }
        }
        if let Some((prev_col, attrs)) = erase {
            let new_pos = crate::grid::Pos { row, col: prev_col };
            if wrapping
                && prev_pos.row + 1 == new_pos.row
                && prev_pos.col >= self.cols()
            {
                if new_pos.col > 0 {
                    contents.extend(
                        " ".repeat(usize::from(new_pos.col)).as_bytes(),
                    );
                } else {
                    contents.extend(b" ");
                    crate::term::Backspace.write_buf(contents);
                }
            } else {
                crate::term::MoveFromTo::new(prev_pos, new_pos)
                    .write_buf(contents);
            }
            prev_pos = new_pos;
            if &prev_attrs != attrs {
                attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *attrs;
            }
            crate::term::ClearRowForward.write_buf(contents);
        }

        (prev_pos, prev_attrs)
    }

    // while it's true that most of the logic in this is identical to
    // write_contents_formatted, i can't figure out how to break out the
    // common parts without making things noticeably slower.
    pub fn write_contents_diff(
        &self,
        contents: &mut Vec<u8>,
        prev: &Self,
        start: u16,
        width: u16,
        row: u16,
        wrapping: bool,
        prev_wrapping: bool,
        mut prev_pos: crate::grid::Pos,
        mut prev_attrs: crate::attrs::Attrs,
    ) -> (crate::grid::Pos, crate::attrs::Attrs) {
        let mut prev_was_wide = false;

        let first_cell = self.cell(start);
        let prev_first_cell = prev.cell(start);
        if wrapping
            && !prev_wrapping
            && first_cell == prev_first_cell
            && prev_pos.row + 1 == row
            && prev_pos.col
                >= self.cols() - u16::from(prev_first_cell.is_wide())
        {
            let first_cell_attrs = first_cell.attrs();
            if &prev_attrs != first_cell_attrs {
                first_cell_attrs
                    .write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *first_cell_attrs;
            }
            let mut cell_contents = prev_first_cell.contents();
            let need_erase = if cell_contents.is_empty() {
                cell_contents = " ";
                true
            } else {
                false
            };
            contents.extend(cell_contents.as_bytes());
            crate::term::Backspace.write_buf(contents);
            if prev_first_cell.is_wide() {
                crate::term::Backspace.write_buf(contents);
            }
            if need_erase {
                crate::term::EraseChar::new(1).write_buf(contents);
            }
            prev_pos = crate::grid::Pos { row, col: 0 };
        }

        let mut erase: Option<(u16, &crate::attrs::Attrs)> = None;
        for (col, (cell, prev_cell)) in self
            .cells()
            .zip(prev.cells())
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            let pos = crate::grid::Pos { row, col };

            if let Some((prev_col, attrs)) = erase {
                if cell.has_contents() || cell.attrs() != attrs {
                    let new_pos = crate::grid::Pos { row, col: prev_col };
                    if wrapping
                        && prev_pos.row + 1 == new_pos.row
                        && prev_pos.col >= self.cols()
                    {
                        if new_pos.col > 0 {
                            contents.extend(
                                " ".repeat(usize::from(new_pos.col))
                                    .as_bytes(),
                            );
                        } else {
                            contents.extend(b" ");
                            crate::term::Backspace.write_buf(contents);
                        }
                    } else {
                        crate::term::MoveFromTo::new(prev_pos, new_pos)
                            .write_buf(contents);
                    }
                    prev_pos = new_pos;
                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }
                    crate::term::EraseChar::new(pos.col - prev_col)
                        .write_buf(contents);
                    erase = None;
                }
            }

            if cell != prev_cell {
                let attrs = cell.attrs();
                if cell.has_contents() {
                    if pos != prev_pos {
                        if !wrapping
                            || prev_pos.row + 1 != pos.row
                            || prev_pos.col
                                < self.cols() - u16::from(cell.is_wide())
                            || pos.col != 0
                        {
                            crate::term::MoveFromTo::new(prev_pos, pos)
                                .write_buf(contents);
                        }
                        prev_pos = pos;
                    }

                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }

                    prev_pos.col += if cell.is_wide() { 2 } else { 1 };
                    contents.extend(cell.contents().as_bytes());
                } else if erase.is_none() {
                    erase = Some((pos.col, attrs));
                }
            }
        }
        if let Some((prev_col, attrs)) = erase {
            let new_pos = crate::grid::Pos { row, col: prev_col };
            if wrapping
                && prev_pos.row + 1 == new_pos.row
                && prev_pos.col >= self.cols()
            {
                if new_pos.col > 0 {
                    contents.extend(
                        " ".repeat(usize::from(new_pos.col)).as_bytes(),
                    );
                } else {
                    contents.extend(b" ");
                    crate::term::Backspace.write_buf(contents);
                }
            } else {
                crate::term::MoveFromTo::new(prev_pos, new_pos)
                    .write_buf(contents);
            }
            prev_pos = new_pos;
            if &prev_attrs != attrs {
                attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *attrs;
            }
            crate::term::ClearRowForward.write_buf(contents);
        }

        // if this row is going from wrapped to not wrapped, we need to erase
        // and redraw the last character to break wrapping. if this row is
        // wrapped, we need to redraw the last character without erasing it to
        // position the cursor after the end of the line correctly so that
        // drawing the next line can just start writing and be wrapped.
        if (!self.wrapped && prev.wrapped) || (!prev.wrapped && self.wrapped)
        {
            let end_pos = if self.cell(self.cols() - 1).is_wide_continuation()
            {
                crate::grid::Pos {
                    row,
                    col: self.cols() - 2,
                }
            } else {
                crate::grid::Pos {
                    row,
                    col: self.cols() - 1,
                }
            };
            crate::term::MoveFromTo::new(prev_pos, end_pos)
                .write_buf(contents);
            prev_pos = end_pos;
            if !self.wrapped {
                crate::term::EraseChar::new(1).write_buf(contents);
            }
            let end_cell = self.cell(end_pos.col);
            if end_cell.has_contents() {
                let attrs = end_cell.attrs();
                if &prev_attrs != attrs {
                    attrs.write_escape_code_diff(contents, &prev_attrs);
                    prev_attrs = *attrs;
                }
                contents.extend(end_cell.contents().as_bytes());
                prev_pos.col += if end_cell.is_wide() { 2 } else { 1 };
            }
        }

        (prev_pos, prev_attrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An orphaned wide cell (wide flag set, no continuation) in a
    // one-column row must not make Row::erase underflow when it derives the
    // logical last column as cols() - 2. Covers the Copilot review note on
    // the clear_wide OOB fix: clear_wide itself was hardened, but its caller
    // erase computed cols() - 2 == 1 - 2 from the captured `wide` value.
    #[test]
    fn erase_wide_cell_in_one_column_row_does_not_underflow() {
        let attrs = crate::attrs::Attrs::default();
        let mut row = Row::new(1);
        // A 2-wide char in the only column: a wide cell with no room for a
        // continuation cell.
        row.get_mut(0).unwrap().set('中', attrs);
        assert!(row.get(0).unwrap().is_wide());

        row.erase(0, attrs);

        let cell = row.get(0).unwrap();
        assert!(!cell.is_wide(), "erased cell must no longer be wide");
        assert!(!cell.has_contents(), "erased cell must be empty");
    }

    // A row trimmed for scrollback keeps only its cells up to the last one
    // that isn't default, and reads and draws as it did before.
    #[test]
    fn trimmed_row_reads_the_same() {
        let plain = crate::attrs::Attrs::default();
        let mut bold = plain;
        bold.set_bold();
        let blue = crate::attrs::Attrs {
            bgcolor: crate::Color::Idx(4),
            ..plain
        };
        let mut row = Row::new(20);
        row.get_mut(0).unwrap().set('a', bold);
        row.get_mut(2).unwrap().set('中', plain);
        row.get_mut(3).unwrap().set_wide_continuation(true);
        // a blank cell with only a background, and one with only a link
        row.get_mut(5).unwrap().clear(blue);
        row.get_mut(7)
            .unwrap()
            .set_hyperlink(std::num::NonZeroU16::new(1));
        row.add_hyperlink();
        row.wrap(true);
        let full = row.clone();

        row = row.trimmed();
        assert_eq!(row.cells.len(), 8);
        assert_eq!(row.cols(), 20);
        assert!(row.wrapped());
        for col in 0..=20 {
            assert_eq!(row.get(col), full.get(col), "col {col}");
        }
        assert!(row.cells().eq(full.cells()));
        assert_eq!(drawn(&row), drawn(&full));
        let mut in_use = [false; 2];
        row.mark_hyperlinks(&mut in_use);
        assert_eq!(in_use, [false, true]);

        // writing to it puts the dropped cells back
        let mut written = full;
        row.get_mut(19).unwrap().set('z', plain);
        written.get_mut(19).unwrap().set('z', plain);
        assert_eq!(row.cells, written.cells);
        row = row.trimmed();
        row.resize(25, crate::Cell::new());
        written.resize(25, crate::Cell::new());
        assert_eq!(row.cells, written.cells);
        assert_eq!(row.cols(), 25);
    }

    #[test]
    fn trimmed_blank_row_holds_no_cells() {
        let row = Row::new(80).trimmed();
        assert_eq!(row.cells.capacity(), 0);
        assert_eq!(row.get(79), Some(&crate::Cell::new()));
        assert_eq!(row.get(80), None);
    }

    fn drawn(row: &Row) -> (String, Vec<u8>, Vec<u8>) {
        let mut text = String::new();
        row.write_contents(&mut text, 0, row.cols(), false);
        let mut formatted = vec![];
        row.write_contents_formatted(
            &mut formatted,
            0,
            row.cols(),
            1,
            true,
            None,
            None,
        );
        let mut diff = vec![];
        row.write_contents_diff(
            &mut diff,
            &Row::new(row.cols()),
            0,
            row.cols(),
            1,
            true,
            false,
            crate::grid::Pos { row: 0, col: 0 },
            crate::attrs::Attrs::default(),
        );
        (text, formatted, diff)
    }
}
