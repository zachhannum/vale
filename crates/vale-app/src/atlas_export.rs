//! PDF export of atlas pages.

use std::path::{Path, PathBuf};

use vale_labeler::Fonts;
use vale_render::PdfDocument;
use vale_store::PageRecord;

use crate::document::Document;
use crate::headless;
use crate::pipeline::{Pipeline, Quality};

/// An export that composes one page at each UI frame.
pub struct PdfExport {
    doc: Document,
    path: PathBuf,
    pages: Vec<PageRecord>,
    next: usize,
    pipeline: Pipeline,
    fonts: Fonts,
    pdf: Option<PdfDocument>,
}

impl PdfExport {
    pub fn new(doc: Document, path: PathBuf, pages: Vec<PageRecord>) -> anyhow::Result<Self> {
        if pages.is_empty() {
            anyhow::bail!("the atlas has no pages");
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if !ext.is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            anyhow::bail!("cannot export to {}; use a .pdf file", path.display());
        }
        Ok(PdfExport {
            doc,
            path,
            pages,
            next: 0,
            pipeline: Pipeline::new(),
            fonts: crate::pipeline::fonts()?,
            pdf: Some(PdfDocument::new()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The page that the next step composes, and the total page count.
    pub fn progress(&self) -> (usize, usize) {
        ((self.next + 1).min(self.pages.len()), self.pages.len())
    }

    /// Composes one page. The result is present after the last page.
    pub fn step(&mut self) -> Option<anyhow::Result<PathBuf>> {
        let result = (|| {
            let page = &self.pages[self.next];
            let list =
                self.pipeline
                    .compose_page(&self.doc, page, &mut self.fonts, Quality::Final)?;
            self.pdf.as_mut().unwrap().add_page(&list)?;
            self.next += 1;
            if self.next < self.pages.len() {
                return Ok(None);
            }
            let bytes = self.pdf.take().unwrap().finish()?;
            headless::write_file(&self.path, &bytes)?;
            Ok(Some(self.path.clone()))
        })();
        match result {
            Ok(None) => None,
            Ok(Some(path)) => Some(Ok(path)),
            Err(error) => Some(Err(error)),
        }
    }
}

/// Writes pages as one PDF without a window.
pub fn export_pdf(doc: &Document, pages: Vec<PageRecord>, path: &Path) -> anyhow::Result<()> {
    let mut job = PdfExport::new(doc.clone(), path.to_path_buf(), pages)?;
    loop {
        if let Some(result) = job.step() {
            return result.map(|_| ());
        }
    }
}
