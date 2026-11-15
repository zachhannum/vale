//! The form that exports pages of the atlas to a PDF file or to PNG files.

use vale_store::PageId;

use super::{AppState, ExportFormat, ExportSpec, atlas};
use crate::atlas_export::DEFAULT_PPI;

/// The fewest pixels per inch of an export.
pub const MIN_PPI: u32 = 72;
/// The most pixels per inch of an export.
pub const MAX_PPI: u32 = 600;

/// The options of the form that the next form of the session starts with.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub format: ExportFormat,
    pub ppi: u32,
    pub transparent: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format: ExportFormat::Pdf,
            ppi: DEFAULT_PPI,
            transparent: false,
        }
    }
}

/// The choice of pages of the form.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PagesMode {
    /// Each page of the atlas.
    All,
    /// The open page.
    This,
    /// The ticked pages of the strip, or the pages from `from` to `to`.
    Range,
}

/// The state of the export form.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportForm {
    pub format: ExportFormat,
    /// The pages of the atlas, in order.
    pub page_ids: Vec<PageId>,
    /// The index of the open page.
    pub open: usize,
    /// One tick for each page of the strip.
    pub selected: Vec<bool>,
    pub pages: PagesMode,
    /// The page number of the first page of the range, as typed.
    pub from: String,
    /// The page number of the last page of the range, as typed.
    pub to: String,
    pub ppi: u32,
    pub transparent: bool,
    /// The file name with no extension.
    pub name: String,
    /// The page that the strip scrolls to on the next frame.
    pub center_on: Option<usize>,
}

impl ExportForm {
    /// A form of the pages of the atlas of `state`, with the options of the
    /// last form of the session. It starts with each page.
    pub fn new(state: &AppState) -> Self {
        let pages = state.doc.project.atlas.pages.iter().map(|p| p.id).collect();
        let (open, _) = atlas::page_index(state);
        Self::with_pages(pages, open, state.export_options, state.export_stem())
    }

    /// A form of these pages, with the page at `open` open.
    pub fn with_pages(page_ids: Vec<PageId>, open: usize, options: Options, name: String) -> Self {
        let count = page_ids.len();
        let mut form = ExportForm {
            format: options.format,
            page_ids,
            open: open.min(count.saturating_sub(1)),
            selected: vec![false; count],
            pages: PagesMode::All,
            from: String::new(),
            to: String::new(),
            ppi: options.ppi.clamp(MIN_PPI, MAX_PPI),
            transparent: options.transparent,
            name,
            center_on: None,
        };
        form.set_mode(PagesMode::All);
        form.center_on = Some(form.open);
        form
    }

    /// The options that the next form of the session starts with.
    pub fn options(&self) -> Options {
        Options {
            format: self.format,
            ppi: self.ppi,
            transparent: self.transparent,
        }
    }

    /// Sets the pixels per inch, inside `MIN_PPI` and `MAX_PPI`.
    pub fn set_ppi(&mut self, ppi: u32) {
        self.ppi = ppi.clamp(MIN_PPI, MAX_PPI);
    }

    /// Ticks the pages of a choice. "This page" scrolls the strip to the
    /// open page. A range keeps the ticks of the strip.
    pub fn set_mode(&mut self, mode: PagesMode) {
        self.pages = mode;
        match mode {
            PagesMode::All => {
                self.selected.fill(true);
                self.from.clear();
                self.to.clear();
            }
            PagesMode::This => {
                self.selected.fill(false);
                if let Some(tick) = self.selected.get_mut(self.open) {
                    *tick = true;
                }
                self.from.clear();
                self.to.clear();
                self.center_on = Some(self.open);
            }
            PagesMode::Range => self.fill_range(),
        }
    }

    /// Ticks or clears one page of the strip.
    pub fn toggle(&mut self, index: usize) {
        if let Some(tick) = self.selected.get_mut(index) {
            *tick = !*tick;
            self.set_mode(PagesMode::Range);
        }
    }

    /// Gives each page from `a` to `b` the tick `on`.
    pub fn select_run(&mut self, a: usize, b: usize, on: bool) {
        let last = self.selected.len().saturating_sub(1);
        let (start, end) = (a.min(b).min(last), a.max(b).min(last));
        if let Some(run) = self.selected.get_mut(start..=end) {
            run.fill(on);
        }
        self.set_mode(PagesMode::Range);
    }

    /// Sets the text of the range. Two page numbers in order tick the pages
    /// from the first to the last.
    pub fn set_from_to(&mut self, from: &str, to: &str) {
        self.pages = PagesMode::Range;
        self.from = from.to_string();
        self.to = to.to_string();
        let count = self.selected.len();
        let number = |text: &str| {
            let n = text.trim().parse::<usize>().ok()?;
            (1..=count).contains(&n).then_some(n - 1)
        };
        if let (Some(first), Some(last)) = (number(from), number(to))
            && first <= last
        {
            for (index, tick) in self.selected.iter_mut().enumerate() {
                *tick = (first..=last).contains(&index);
            }
        }
    }

    /// Fills the range from the ticks: one run of pages gives its first and
    /// last page numbers, and gaps give an empty range.
    fn fill_range(&mut self) {
        let ticked: Vec<usize> = (self.selected.iter().enumerate())
            .filter(|(_, tick)| **tick)
            .map(|(index, _)| index)
            .collect();
        match (ticked.first(), ticked.last()) {
            (Some(&first), Some(&last)) if last - first + 1 == ticked.len() => {
                self.from = (first + 1).to_string();
                self.to = (last + 1).to_string();
            }
            _ => {
                self.from.clear();
                self.to.clear();
            }
        }
    }

    /// True if the form has a page and a name.
    pub fn can_export(&self) -> bool {
        self.selected.contains(&true) && !self.name.trim().is_empty()
    }

    /// The export of the ticked pages.
    pub fn spec(&self) -> ExportSpec {
        let pages = (self.page_ids.iter().zip(&self.selected))
            .filter(|(_, tick)| **tick)
            .map(|(id, _)| *id)
            .collect();
        ExportSpec {
            format: self.format,
            pages,
            ppi: self.ppi,
            transparent: self.transparent,
            name: self.name.trim().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(count: u32, open: usize) -> ExportForm {
        let pages = (1..=count).map(PageId).collect();
        ExportForm::with_pages(pages, open, Options::default(), "Terra".to_string())
    }

    fn ticks(form: &ExportForm) -> Vec<usize> {
        (form.selected.iter().enumerate())
            .filter(|(_, tick)| **tick)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn all_pages_ticks_each_page() {
        let mut form = form(5, 2);
        form.set_mode(PagesMode::This);
        form.set_mode(PagesMode::All);
        assert_eq!(ticks(&form), [0, 1, 2, 3, 4]);
        assert_eq!(form.spec().pages, (1..=5).map(PageId).collect::<Vec<_>>());
    }

    #[test]
    fn this_page_ticks_only_the_open_page_and_scrolls_to_it() {
        let mut form = form(5, 3);
        form.center_on = None;
        form.set_mode(PagesMode::This);
        assert_eq!(ticks(&form), [3]);
        assert_eq!(form.center_on, Some(3));
        assert_eq!(form.spec().pages, [PageId(4)]);
    }

    #[test]
    fn one_run_in_the_strip_gives_a_range_with_its_page_numbers() {
        let mut form = form(6, 0);
        form.set_mode(PagesMode::This);
        form.select_run(3, 1, true);
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(ticks(&form), [0, 1, 2, 3]);
        assert_eq!((form.from.as_str(), form.to.as_str()), ("1", "4"));
    }

    #[test]
    fn gaps_in_the_strip_give_an_empty_range() {
        let mut form = form(6, 0);
        form.toggle(2);
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(ticks(&form), [0, 1, 3, 4, 5]);
        assert_eq!((form.from.as_str(), form.to.as_str()), ("", ""));
    }

    #[test]
    fn an_edit_of_the_range_ticks_the_strip() {
        let mut form = form(6, 0);
        form.set_from_to("2", "4");
        assert_eq!(form.pages, PagesMode::Range);
        assert_eq!(ticks(&form), [1, 2, 3]);
        // A number outside the atlas leaves the strip as it is.
        form.set_from_to("2", "9");
        assert_eq!(ticks(&form), [1, 2, 3]);
        assert_eq!(form.to, "9");
    }

    #[test]
    fn a_form_with_no_page_or_no_name_cannot_export() {
        let mut form = form(2, 0);
        assert!(form.can_export());
        form.select_run(0, 1, false);
        assert!(!form.can_export());
        form.set_mode(PagesMode::All);
        form.name = "  ".to_string();
        assert!(!form.can_export());
    }
}
