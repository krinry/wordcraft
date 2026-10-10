use std::sync::Arc;

use wordcraft_doc::para::InlineObject;
use wordcraft_doc::props::NumRef;
use wordcraft_doc::{Block, CharProps, Document, ListKind, Paragraph, Table, Watermark, para_block};

use crate::*;

fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(16, 8, |x, y| image::Rgba([(x * 15) as u8, (y * 30) as u8, 200, 255]));
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    buf
}

/// Headings, a bold run, a hyperlink, an internal link, a list, a table and a picture.
fn sample() -> Document {
    let mut d = Document::new();
    d.core.title = "PDF Sample".into();
    d.core.creator = "Ada Writer".into();
    d.core.subject = "Testing".into();
    d.core.keywords = "pdf, export".into();
    d.core.created = "2024-05-06T07:08:09Z".into();
    let mut b = Vec::new();
    let mut h1 = Paragraph::with_text("Introduction", CharProps::default()).styled("Heading1");
    let n = h1.len();
    h1.insert_object(n, InlineObject::BookmarkStart { name: "intro".into() }, &CharProps::default()).unwrap();
    b.push(para_block(h1));
    let mut p = Paragraph::with_text("Hello WordCraft, ", CharProps::default());
    let n = p.len();
    p.insert_text(n, "bold words", &CharProps { bold: Some(true), ..Default::default() }).unwrap();
    let n = p.len();
    p.insert_text(n, " and a website", &CharProps { link: Some("https://example.com/".into()), ..Default::default() }).unwrap();
    let n = p.len();
    p.insert_text(n, " back to top", &CharProps { link: Some("#intro".into()), italic: Some(true), ..Default::default() }).unwrap();
    b.push(para_block(p));
    b.push(para_block(Paragraph::with_text("Details", CharProps::default()).styled("Heading2")));
    let list = d.numbering.add_list(ListKind::Numbered);
    for t in ["alpha item", "beta item"] {
        let mut q = Paragraph::with_text(t, CharProps::default()).styled("ListParagraph");
        q.props.numbering = Some(NumRef { num: list, level: 0 });
        b.push(para_block(q));
    }
    let mut t = Table::new(2, 2, 400.0);
    for (r, c, s) in [(0, 0, "Cell00"), (0, 1, "Cell01"), (1, 0, "Cell10"), (1, 1, "Cell11")] {
        t.rows[r].cells[c].blocks = vec![para_block(Paragraph::with_text(s, CharProps::default()))];
    }
    b.push(Arc::new(Block::Table(t)));
    let key = d.add_media(png(), "png");
    let mut ip = Paragraph::with_text("Figure: ", CharProps::default());
    let n = ip.len();
    ip.insert_object(
        n,
        InlineObject::Image { media: key, w: 80.0, h: 40.0, alt: "gradient".into(), float: Default::default(), crop: [0.1, 0.0, 0.0, 0.0] },
        &CharProps::default(),
    )
    .unwrap();
    b.push(para_block(ip));
    let mut big = Paragraph::with_text("Second page", CharProps::default()).styled("Heading1");
    big.props.page_break_before = Some(true);
    b.push(para_block(big));
    b.push(para_block(Paragraph::with_text(
        "Dotted and wavy",
        CharProps { underline: Some(wordcraft_doc::props::Underline::Wave), ..Default::default() },
    )));
    d.body = b;
    d.settings.watermark = Some(Watermark::default());
    d.settings.page_color = Some(Rgb(0xFF, 0xFE, 0xF0));
    d
}

/// Text of every page through hayro's interpreter (glyph → Unicode via ToUnicode / ActualText).
fn extract_text(bytes: &[u8]) -> Vec<String> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    use hayro_syntax::Pdf;
    struct Ex(String);
    impl Device<'_> for Ex {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            match g.as_unicode() {
                Some(hayro_cmap::BfString::Char(c)) => self.0.push(c),
                Some(hayro_cmap::BfString::String(s)) => self.0.push_str(&s),
                None => self.0.push('\u{FFFD}'),
            }
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|page| {
            let mut ctx =
                Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
            let mut ex = Ex(String::new());
            interpret_page(page, &mut ctx, &mut ex);
            ex.0
        })
        .collect()
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn exports_pages_text_and_structure() {
    let d = sample();
    let lay = layout(&d, &mut LayoutCache::new(), &LayoutOptions::default());
    assert!(lay.pages.len() >= 2);
    let bytes = export(&d, &PdfOptions::default()).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).expect("parse");
    assert_eq!(pdf.pages().len(), lay.pages.len());
    let text = extract_text(&bytes);
    let all = squash(&text.concat());
    for w in [
        "Introduction",
        "Hello WordCraft,",
        "bold words",
        "website",
        "Details",
        "alpha item",
        "beta item",
        "Cell00",
        "Cell11",
        "Second page",
        "DRAFT",
    ] {
        assert!(all.contains(w), "{w:?} missing from {all:?}");
    }
    assert!(squash(&text[0]).contains("Hello WordCraft, bold words and a website back to top"), "{:?}", text[0]);

    let raw = String::from_utf8_lossy(&export(&d, &PdfOptions { compress: false, ..Default::default() }).unwrap()).into_owned();
    assert!(raw.contains("/FontFile2") || raw.contains("/FontFile3"), "embedded fonts");
    assert!(raw.contains("/ToUnicode"));
    assert!(raw.contains("/URI (https://example.com/)") || raw.contains("/URI(https://example.com/)"), "URL link");
    assert!(raw.contains("/Subtype /Link") || raw.contains("/Subtype/Link"));
    assert!(raw.contains("/Outlines"), "outline");
    assert!(raw.contains("Introduction"));
    assert!(raw.contains("/StructTreeRoot"), "tagged");
    assert!(raw.contains("/Image"), "picture");
    assert!(raw.contains("PDF Sample"), "title");
    assert!(raw.contains("Ada Writer"), "author");
    assert!(raw.contains("D:20240506070809"), "creation date");
}

#[test]
fn page_selection_and_errors() {
    let d = sample();
    let one = export(&d, &PdfOptions { pages: Some(vec![1]), tagged: false, ..Default::default() }).unwrap();
    let pdf = hayro_syntax::Pdf::new(one.clone()).unwrap();
    assert_eq!(pdf.pages().len(), 1);
    assert!(squash(&extract_text(&one)[0]).contains("Second page"));
    assert_eq!(export(&d, &PdfOptions { pages: Some(vec![99]), ..Default::default() }), Err(PdfError::BadPage(100)));
    assert_eq!(export(&d, &PdfOptions { pages: Some(vec![]), ..Default::default() }), Err(PdfError::NoPages));
}

#[test]
fn hostile_documents_export() {
    let mut d = Document::from_text("x");
    d.last_section.page_w = f32::NAN;
    d.last_section.page_h = 1e9;
    d.media.insert("bad.png".into(), Arc::new(b"not an image".to_vec()));
    let mut p = Paragraph::with_text("pic ", CharProps::default());
    p.insert_object(
        4,
        InlineObject::Image {
            media: "bad.png".into(),
            w: f32::INFINITY,
            h: -3.0,
            alt: String::new(),
            float: Default::default(),
            crop: [f32::NAN; 4],
        },
        &CharProps::default(),
    )
    .unwrap();
    p.insert_object(
        0,
        InlineObject::Image { media: "missing".into(), w: 10.0, h: 10.0, alt: String::new(), float: Default::default(), crop: [0.0; 4] },
        &CharProps::default(),
    )
    .unwrap();
    d.body.push(para_block(p));
    let mut q =
        Paragraph::with_text("Unknown font 😀 עברית", CharProps { font: Some("No Such Font".into()), size: Some(2000.0), ..Default::default() });
    q.props.style = Some("Heading9".into());
    d.body.push(para_block(q));
    d.settings.watermark = Some(Watermark { text: "   ".into(), ..Default::default() });
    let _ = export(&d, &PdfOptions::default());
    d.settings.watermark = Some(Watermark { text: "😀".repeat(500), font: "Nope".into(), diagonal: false, ..Default::default() });
    let _ = export(&d, &PdfOptions { tagged: false, ..Default::default() });
    assert!(export(&Document::new(), &PdfOptions::default()).unwrap().starts_with(b"%PDF"));
}

#[test]
fn text_mapping_handles_ligatures_and_extras() {
    let d = Document::from_text("office affine");
    let lay = layout(&d, &mut LayoutCache::new(), &LayoutOptions::default());
    let bytes = export_layout(&d, &lay, &PdfOptions::default()).unwrap();
    assert!(squash(&extract_text(&bytes)[0]).contains("office affine"));
}

#[test]
fn shapes_have_paths() {
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    for k in [
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::SingleCornerSnipped,
        ShapeKind::TopCornersSnipped,
        ShapeKind::DiagonalCornersSnipped,
        ShapeKind::SnippedAndRounded,
        ShapeKind::RoundSingleCorner,
        ShapeKind::RoundSameSideCorner,
        ShapeKind::RoundDiagonalCorner,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::RightTriangle,
        ShapeKind::Parallelogram,
        ShapeKind::Trapezoid,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::Heptagon,
        ShapeKind::Octagon,
        ShapeKind::Decagon,
        ShapeKind::Dodecagon,
        ShapeKind::Pie,
        ShapeKind::Chord,
        ShapeKind::Teardrop,
        ShapeKind::Frame,
        ShapeKind::HalfFrame,
        ShapeKind::LShape,
        ShapeKind::DiagonalStripe,
        ShapeKind::Cross,
        ShapeKind::Plaque,
        ShapeKind::Cylinder,
        ShapeKind::Cube,
        ShapeKind::Bevel,
        ShapeKind::Donut,
        ShapeKind::NoSymbol,
        ShapeKind::BlockArc,
        ShapeKind::FoldedCorner,
        ShapeKind::SmileyFace,
        ShapeKind::Heart,
        ShapeKind::LightningBolt,
        ShapeKind::Sun,
        ShapeKind::Moon,
        ShapeKind::Cloud,
        ShapeKind::Arc,
        ShapeKind::DoubleBracket,
        ShapeKind::DoubleBrace,
        ShapeKind::LeftBracket,
        ShapeKind::RightBracket,
        ShapeKind::LeftBrace,
        ShapeKind::RightBrace,
        ShapeKind::TextBox,
        ShapeKind::Line,
        ShapeKind::Arrow,
        ShapeKind::DoubleArrow,
        ShapeKind::ElbowConnector,
        ShapeKind::ElbowArrow,
        ShapeKind::ElbowDoubleArrow,
        ShapeKind::CurvedConnector,
        ShapeKind::CurvedArrow,
        ShapeKind::CurvedDoubleArrow,
        ShapeKind::Curve,
        ShapeKind::Freeform,
        ShapeKind::Scribble,
        ShapeKind::LeftArrow,
        ShapeKind::UpArrow,
        ShapeKind::DownArrow,
        ShapeKind::LeftRightArrow,
        ShapeKind::UpDownArrow,
        ShapeKind::QuadArrow,
        ShapeKind::LeftRightUpArrow,
        ShapeKind::BentArrow,
        ShapeKind::UTurnArrow,
        ShapeKind::LeftUpArrow,
        ShapeKind::BentUpArrow,
        ShapeKind::StripedRightArrow,
        ShapeKind::NotchedRightArrow,
        ShapeKind::PentagonArrow,
        ShapeKind::Chevron,
        ShapeKind::RightArrowCallout,
        ShapeKind::DownArrowCallout,
        ShapeKind::LeftArrowCallout,
        ShapeKind::UpArrowCallout,
        ShapeKind::LeftRightArrowCallout,
        ShapeKind::QuadArrowCallout,
        ShapeKind::CircularArrow,
        ShapeKind::MathPlus,
        ShapeKind::MathMinus,
        ShapeKind::MathMultiply,
        ShapeKind::MathDivide,
        ShapeKind::MathEqual,
        ShapeKind::MathNotEqual,
        ShapeKind::FlowchartProcess,
        ShapeKind::FlowchartAlternateProcess,
        ShapeKind::FlowchartDecision,
        ShapeKind::FlowchartData,
        ShapeKind::FlowchartPredefinedProcess,
        ShapeKind::FlowchartInternalStorage,
        ShapeKind::FlowchartDocument,
        ShapeKind::FlowchartMultidocument,
        ShapeKind::FlowchartTerminator,
        ShapeKind::FlowchartPreparation,
        ShapeKind::FlowchartManualInput,
        ShapeKind::FlowchartManualOperation,
        ShapeKind::FlowchartConnector,
        ShapeKind::FlowchartOffpageConnector,
        ShapeKind::FlowchartCard,
        ShapeKind::FlowchartPunchedTape,
        ShapeKind::FlowchartSummingJunction,
        ShapeKind::FlowchartOr,
        ShapeKind::FlowchartCollate,
        ShapeKind::FlowchartSort,
        ShapeKind::FlowchartExtract,
        ShapeKind::FlowchartMerge,
        ShapeKind::FlowchartStoredData,
        ShapeKind::FlowchartDelay,
        ShapeKind::FlowchartSequentialData,
        ShapeKind::FlowchartMagneticDisk,
        ShapeKind::FlowchartDirectAccessStorage,
        ShapeKind::FlowchartDisplay,
        ShapeKind::Explosion1,
        ShapeKind::Explosion2,
        ShapeKind::Star4,
        ShapeKind::Star,
        ShapeKind::Star6,
        ShapeKind::Star7,
        ShapeKind::Star8,
        ShapeKind::Star10,
        ShapeKind::Star12,
        ShapeKind::Star16,
        ShapeKind::Star24,
        ShapeKind::Star32,
        ShapeKind::UpRibbon,
        ShapeKind::DownRibbon,
        ShapeKind::CurvedUpRibbon,
        ShapeKind::CurvedDownRibbon,
        ShapeKind::VerticalScroll,
        ShapeKind::HorizontalScroll,
        ShapeKind::Wave,
        ShapeKind::DoubleWave,
        ShapeKind::RectangularCallout,
        ShapeKind::RoundedRectangularCallout,
        ShapeKind::OvalCallout,
        ShapeKind::CloudCallout,
        ShapeKind::LineCallout1,
        ShapeKind::LineCallout2,
        ShapeKind::LineCallout3,
        ShapeKind::LineCallout4,
        ShapeKind::LineCallout1AccentBar,
        ShapeKind::LineCallout2AccentBar,
        ShapeKind::LineCallout3AccentBar,
        ShapeKind::LineCallout4AccentBar,
        ShapeKind::DrawingCanvas,
    ] {
        assert!(shape_path(k, &r).is_some(), "{k:?}");
    }
    assert!(shape_path(ShapeKind::Ellipse, &Rect::new(0.0, 0.0, f32::NAN, 1.0)).is_none());
    assert_eq!(parse_iso("2024-02-03T04:05:06Z"), Some((2024, 2, 3, 4, 5, 6)));
    assert_eq!(parse_iso("garbage"), None);
    assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
}
