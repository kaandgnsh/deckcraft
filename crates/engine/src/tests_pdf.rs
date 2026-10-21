//! Opening PDF files: one editable slide per page.

use serde_json::json;

use crate::cmd::file::{open_presentation, pdf_bytes};
use crate::*;

fn sample_pdf() -> (deckcraft_model::Presentation, Vec<u8>) {
    let mut s = Session::with_new();
    crate::sample::open_sample(&mut s).unwrap();
    let doc = (*s.doc().unwrap().doc).clone();
    let pdf = pdf_bytes(&doc, &json!({})).unwrap();
    (doc, pdf)
}

/// Every text on a slide, joined.
fn slide_text(slide: &deckcraft_model::Slide) -> String {
    slide.shapes.iter().filter_map(|sh| sh.text.as_ref().map(|t| t.text())).collect::<Vec<_>>().join(" ")
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A one-page PDF of `w`×`h` points with a line of Helvetica.
fn page_pdf(w: u32, h: u32) -> Vec<u8> {
    let body = "BT /F1 18 Tf 72 300 Td (Quarterly results) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-pdf-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn pdf_opens_one_slide_per_page() {
    let (doc, pdf) = sample_pdf();
    let opened = open_presentation("a.pdf", &pdf).unwrap();
    assert_eq!(opened.slides.len(), doc.slides.len());
    let title = squash(&doc.slides[0].title());
    let text = squash(&slide_text(&opened.slides[0]));
    assert!(!title.is_empty() && text.contains(&title), "{title:?} in {text:?}");
}

#[test]
fn pdf_slide_size_follows_page() {
    let p = open_presentation("a4.pdf", &page_pdf(595, 842)).unwrap();
    let ratio = p.slide_size.width / p.slide_size.height;
    assert!((ratio - 595.0 / 842.0).abs() < 0.01, "{ratio}");
}

#[test]
fn pdf_text_is_editable_shape() {
    let p = open_presentation("a.pdf", &page_pdf(960, 540)).unwrap();
    assert!(p.slides[0].shapes.iter().any(|sh| sh.text.as_ref().is_some_and(|t| t.text().contains("Quarterly results"))));
}

#[test]
fn pdf_by_signature() {
    assert_eq!(open_presentation("a.pptx", &page_pdf(960, 540)).unwrap().slides.len(), 1);
    let e = open_presentation("a.pdf", b"merhaba").unwrap_err().to_string();
    assert!(e.contains("a.pdf"), "{e}");
}

#[test]
fn file_open_pdf_does_not_keep_its_path() {
    let path = tmp("deck.pdf", &page_pdf(960, 540));
    let mut s = Session::with_new();
    s.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 1);
    assert_eq!(s.doc().unwrap().path, None, "Save must not write a presentation over the PDF");
}

#[test]
fn reuse_slides_from_pdf() {
    let (_, pdf) = sample_pdf();
    let path = tmp("reuse.pdf", &pdf);
    let mut s = Session::with_new();
    let before = s.doc().unwrap().doc.slides.len();
    s.execute("slide.reuse", &json!({"path": path.to_str().unwrap(), "slides": [1]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), before + 1);
}

#[test]
fn hostile_pdf_never_panics() {
    let (_, good) = sample_pdf();
    for step in 1..8 {
        let _ = open_presentation("cut.pdf", &good[..good.len() * step / 8]);
    }
    for seed in 0..96usize {
        let mut v = good.clone();
        for i in (seed % 89..v.len()).step_by(89) {
            v[i] = v[i].wrapping_add(seed as u8).rotate_left(3);
        }
        let _ = open_presentation("bad.pdf", &v);
    }
}

/// A PDF made by PdfCraft (File › Protect) with the open password `kullanici`; see
/// ATTRIBUTION.md.
const ENCRYPTED: &[u8] = include_bytes!("../tests/data/encrypted.pdf");

#[test]
fn encrypted_pdf_asks_for_password() {
    let path = tmp("enc.pdf", ENCRYPTED);
    let mut s = Session::with_new();
    let before = s.documents().len();
    let e = s.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap_err().to_string();
    assert!(crate::cmd::file::needs_password(&e) && !crate::cmd::file::wrong_password(&e), "{e}");
    assert_eq!(s.documents().len(), before, "nothing opened");
}

#[test]
fn wrong_password_asks_again() {
    let path = tmp("enc-wrong.pdf", ENCRYPTED);
    let mut s = Session::with_new();
    let e = s.execute("file.open", &json!({"path": path.to_str().unwrap(), "password": "yanlis"})).unwrap_err().to_string();
    assert!(crate::cmd::file::needs_password(&e) && crate::cmd::file::wrong_password(&e), "{e}");
}

#[test]
fn right_password_opens() {
    let path = tmp("enc-right.pdf", ENCRYPTED);
    let mut s = Session::with_new();
    s.execute("file.open", &json!({"path": path.to_str().unwrap(), "password": "kullanici"})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert!(slide_text(&doc.slides[0]).contains("Secret text"), "{:?}", slide_text(&doc.slides[0]));
}

#[test]
fn open_pdf_pages_param() {
    let (doc, pdf) = sample_pdf();
    let path = tmp("pages.pdf", &pdf);
    let mut s = Session::with_new();
    s.execute("file.open", &json!({"path": path.to_str().unwrap(), "pages": "2"})).unwrap();
    let opened = &s.doc().unwrap().doc;
    assert_eq!(opened.slides.len(), 1);
    let want = squash(&slide_text(&doc.slides[1]));
    let first = want.split(' ').next().unwrap_or("");
    assert!(squash(&slide_text(&opened.slides[0])).contains(first), "{want:?}");
}

#[test]
fn open_pdf_bad_pages() {
    let (doc, pdf) = sample_pdf();
    let path = tmp("bad-pages.pdf", &pdf);
    let mut s = Session::with_new();
    let e = s.execute("file.open", &json!({"path": path.to_str().unwrap(), "pages": "99"})).unwrap_err().to_string();
    assert!(e.contains("99") && e.contains(&doc.slides.len().to_string()), "{e}");
}

#[test]
fn open_pdf_says_it_was_converted() {
    let (_, pdf) = sample_pdf();
    let path = tmp("note.pdf", &pdf);
    let mut s = Session::with_new();
    let r = s.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!(r["converted"]["from"], json!("pdf"), "{r}");
    let msg = s.ui_requests.iter().find_map(|u| match u {
        crate::UiRequest::Message { text } => Some(text.clone()),
        _ => None,
    });
    assert!(msg.as_deref().is_some_and(|m| m.contains("PDF")), "{msg:?}");
}
