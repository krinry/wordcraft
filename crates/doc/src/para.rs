//! Paragraphs: text with formatting runs and inline objects.
//!
//! A paragraph's text is one `String`. Formatting is a list of [`Run`]s whose byte lengths sum to
//! the text length. Inline objects (pictures, fields, note references, bookmarks, comment
//! anchors…) sit in the text as U+FFFC; the k-th U+FFFC is `objects[k]`. Special characters:
//! `\t` tab, `\n` manual line break, U+000C page break, U+000E column break.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::props::{CharProps, ParaProps};
use crate::section::SectionProps;
use crate::{DocError, Result};

/// Object replacement character: marks an inline object in paragraph text.
pub const OBJ: char = '\u{FFFC}';
/// Manual line break (Shift+Enter).
pub const LINE_BREAK: char = '\n';
/// Page break (Ctrl+Enter).
pub const PAGE_BREAK: char = '\u{000C}';
/// Column break (Ctrl+Shift+Enter).
pub const COLUMN_BREAK: char = '\u{000E}';
/// Optional hyphen (Ctrl+-).
pub const SOFT_HYPHEN: char = '\u{00AD}';
/// Nonbreaking hyphen (Ctrl+Shift+-).
pub const NB_HYPHEN: char = '\u{2011}';
/// Nonbreaking space (Ctrl+Shift+Space).
pub const NBSP: char = '\u{00A0}';

static REV: AtomicU64 = AtomicU64::new(1);

/// A fresh revision number (process-unique), used by layout caches to detect edits.
pub fn next_rev() -> u64 {
    REV.fetch_add(1, Ordering::Relaxed)
}

/// A formatting run: `len` bytes of text sharing `props`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub len: usize,
    #[serde(default, skip_serializing_if = "CharProps::is_empty")]
    pub props: CharProps,
}

/// How a floating object is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Wrap {
    /// In line with text (not floating).
    #[default]
    Inline,
    Square,
    Tight,
    Through,
    TopAndBottom,
    BehindText,
    InFrontOfText,
}

/// What a floating object's offset is relative to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    #[default]
    Column,
    Margin,
    Page,
    Paragraph,
}

/// Floating placement (ignored when `wrap` is `Inline`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Float {
    pub wrap: Wrap,
    pub h_rel: Anchor,
    pub v_rel: Anchor,
    /// Offsets, points.
    pub x: f32,
    pub y: f32,
    /// Distance from surrounding text (points).
    pub dist: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    #[default]
    #[serde(alias = "rect")]
    Rectangle,
    #[serde(alias = "roundRect")]
    RoundedRectangle,
    SingleCornerSnipped,
    TopCornersSnipped,
    DiagonalCornersSnipped,
    SnippedAndRounded,
    RoundSingleCorner,
    RoundSameSideCorner,
    RoundDiagonalCorner,

    // Basic shapes
    #[serde(alias = "oval")]
    Ellipse,
    #[serde(alias = "isoscelesTriangle")]
    Triangle,
    RightTriangle,
    Parallelogram,
    Trapezoid,
    Diamond,
    Pentagon,
    Hexagon,
    Heptagon,
    Octagon,
    Decagon,
    Dodecagon,
    Pie,
    Chord,
    Teardrop,
    Frame,
    HalfFrame,
    LShape,
    DiagonalStripe,
    Cross,
    Plaque,
    Cylinder,
    Cube,
    Bevel,
    Donut,
    NoSymbol,
    BlockArc,
    FoldedCorner,
    SmileyFace,
    Heart,
    LightningBolt,
    Sun,
    Moon,
    Cloud,
    Arc,
    DoubleBracket,
    DoubleBrace,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    /// A text box (rectangle with a text story).
    TextBox,

    // Lines
    Line,
    #[serde(alias = "rightArrow")]
    Arrow,
    DoubleArrow,
    ElbowConnector,
    ElbowArrow,
    ElbowDoubleArrow,
    CurvedConnector,
    CurvedArrow,
    CurvedDoubleArrow,
    Curve,
    Freeform,
    Scribble,

    // Block arrows
    LeftArrow,
    UpArrow,
    DownArrow,
    LeftRightArrow,
    UpDownArrow,
    QuadArrow,
    LeftRightUpArrow,
    BentArrow,
    UTurnArrow,
    LeftUpArrow,
    BentUpArrow,
    StripedRightArrow,
    NotchedRightArrow,
    PentagonArrow,
    Chevron,
    RightArrowCallout,
    DownArrowCallout,
    LeftArrowCallout,
    UpArrowCallout,
    LeftRightArrowCallout,
    QuadArrowCallout,
    CircularArrow,

    // Equation shapes
    MathPlus,
    MathMinus,
    MathMultiply,
    MathDivide,
    MathEqual,
    MathNotEqual,

    // Flowchart
    FlowchartProcess,
    FlowchartAlternateProcess,
    FlowchartDecision,
    FlowchartData,
    FlowchartPredefinedProcess,
    FlowchartInternalStorage,
    FlowchartDocument,
    FlowchartMultidocument,
    FlowchartTerminator,
    FlowchartPreparation,
    FlowchartManualInput,
    FlowchartManualOperation,
    FlowchartConnector,
    FlowchartOffpageConnector,
    FlowchartCard,
    FlowchartPunchedTape,
    FlowchartSummingJunction,
    FlowchartOr,
    FlowchartCollate,
    FlowchartSort,
    FlowchartExtract,
    FlowchartMerge,
    FlowchartStoredData,
    FlowchartDelay,
    FlowchartSequentialData,
    FlowchartMagneticDisk,
    FlowchartDirectAccessStorage,
    FlowchartDisplay,

    // Stars and Banners
    Explosion1,
    Explosion2,
    Star4,
    #[serde(alias = "star5")]
    Star,
    Star6,
    Star7,
    Star8,
    Star10,
    Star12,
    Star16,
    Star24,
    Star32,
    UpRibbon,
    DownRibbon,
    CurvedUpRibbon,
    CurvedDownRibbon,
    VerticalScroll,
    HorizontalScroll,
    Wave,
    DoubleWave,

    // Callouts
    RectangularCallout,
    RoundedRectangularCallout,
    OvalCallout,
    CloudCallout,
    LineCallout1,
    LineCallout2,
    LineCallout3,
    LineCallout4,
    LineCallout1AccentBar,
    LineCallout2AccentBar,
    LineCallout3AccentBar,
    LineCallout4AccentBar,

    // Drawing Canvas
    DrawingCanvas,
}

impl ShapeKind {
    /// True if the shape is fundamentally a 1D stroke/line rather than a 2D filled polygon.
    pub fn is_line_like(&self) -> bool {
        matches!(
            self,
            ShapeKind::Line
                | ShapeKind::DoubleArrow
                | ShapeKind::ElbowConnector
                | ShapeKind::ElbowArrow
                | ShapeKind::ElbowDoubleArrow
                | ShapeKind::CurvedConnector
                | ShapeKind::CurvedArrow
                | ShapeKind::CurvedDoubleArrow
                | ShapeKind::Curve
                | ShapeKind::Freeform
                | ShapeKind::Scribble
                | ShapeKind::Arc
                | ShapeKind::LeftBracket
                | ShapeKind::RightBracket
                | ShapeKind::LeftBrace
                | ShapeKind::RightBrace
                | ShapeKind::DoubleBracket
                | ShapeKind::DoubleBrace
                | ShapeKind::LineCallout1
                | ShapeKind::LineCallout2
                | ShapeKind::LineCallout3
                | ShapeKind::LineCallout4
                | ShapeKind::LineCallout1AccentBar
                | ShapeKind::LineCallout2AccentBar
                | ShapeKind::LineCallout3AccentBar
                | ShapeKind::LineCallout4AccentBar
        )
    }

    /// English display name.
    pub fn label(&self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::RoundedRectangle => "Rounded Rectangle",
            ShapeKind::SingleCornerSnipped => "Single Corner Snipped",
            ShapeKind::TopCornersSnipped => "Top Corners Snipped",
            ShapeKind::DiagonalCornersSnipped => "Diagonal Corners Snipped",
            ShapeKind::SnippedAndRounded => "Snipped and Single Corner Rounded",
            ShapeKind::RoundSingleCorner => "Round Single Corner",
            ShapeKind::RoundSameSideCorner => "Round Same Side Corner",
            ShapeKind::RoundDiagonalCorner => "Round Diagonal Corner",

            ShapeKind::Ellipse => "Oval",
            ShapeKind::Triangle => "Isosceles Triangle",
            ShapeKind::RightTriangle => "Right Triangle",
            ShapeKind::Parallelogram => "Parallelogram",
            ShapeKind::Trapezoid => "Trapezoid",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Pentagon => "Regular Pentagon",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::Heptagon => "Heptagon",
            ShapeKind::Octagon => "Octagon",
            ShapeKind::Decagon => "Decagon",
            ShapeKind::Dodecagon => "Dodecagon",
            ShapeKind::Pie => "Pie",
            ShapeKind::Chord => "Chord",
            ShapeKind::Teardrop => "Teardrop",
            ShapeKind::Frame => "Frame",
            ShapeKind::HalfFrame => "Half Frame",
            ShapeKind::LShape => "L-Shape",
            ShapeKind::DiagonalStripe => "Diagonal Stripe",
            ShapeKind::Cross => "Cross",
            ShapeKind::Plaque => "Plaque",
            ShapeKind::Cylinder => "Can",
            ShapeKind::Cube => "Cube",
            ShapeKind::Bevel => "Bevel",
            ShapeKind::Donut => "Donut",
            ShapeKind::NoSymbol => "\"No\" Symbol",
            ShapeKind::BlockArc => "Block Arc",
            ShapeKind::FoldedCorner => "Folded Corner",
            ShapeKind::SmileyFace => "Smiley Face",
            ShapeKind::Heart => "Heart",
            ShapeKind::LightningBolt => "Lightning Bolt",
            ShapeKind::Sun => "Sun",
            ShapeKind::Moon => "Moon",
            ShapeKind::Cloud => "Cloud",
            ShapeKind::Arc => "Arc",
            ShapeKind::DoubleBracket => "Double Bracket",
            ShapeKind::DoubleBrace => "Double Brace",
            ShapeKind::LeftBracket => "Left Bracket",
            ShapeKind::RightBracket => "Right Bracket",
            ShapeKind::LeftBrace => "Left Brace",
            ShapeKind::RightBrace => "Right Brace",
            ShapeKind::TextBox => "Text Box",

            ShapeKind::Line => "Line",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::DoubleArrow => "Double Arrow",
            ShapeKind::ElbowConnector => "Elbow Connector",
            ShapeKind::ElbowArrow => "Elbow Arrow",
            ShapeKind::ElbowDoubleArrow => "Elbow Double Arrow",
            ShapeKind::CurvedConnector => "Curved Connector",
            ShapeKind::CurvedArrow => "Curved Arrow",
            ShapeKind::CurvedDoubleArrow => "Curved Double Arrow",
            ShapeKind::Curve => "Curve",
            ShapeKind::Freeform => "Freeform: Shape",
            ShapeKind::Scribble => "Freeform: Scribble",

            ShapeKind::LeftArrow => "Left Arrow",
            ShapeKind::UpArrow => "Up Arrow",
            ShapeKind::DownArrow => "Down Arrow",
            ShapeKind::LeftRightArrow => "Left-Right Arrow",
            ShapeKind::UpDownArrow => "Up-Down Arrow",
            ShapeKind::QuadArrow => "Quad Arrow",
            ShapeKind::LeftRightUpArrow => "Left-Right-Up Arrow",
            ShapeKind::BentArrow => "Bent Arrow",
            ShapeKind::UTurnArrow => "U-Turn Arrow",
            ShapeKind::LeftUpArrow => "Left-Up Arrow",
            ShapeKind::BentUpArrow => "Bent-Up Arrow",
            ShapeKind::StripedRightArrow => "Striped Right Arrow",
            ShapeKind::NotchedRightArrow => "Notched Right Arrow",
            ShapeKind::PentagonArrow => "Pentagon",
            ShapeKind::Chevron => "Chevron",
            ShapeKind::RightArrowCallout => "Right Arrow Callout",
            ShapeKind::DownArrowCallout => "Down Arrow Callout",
            ShapeKind::LeftArrowCallout => "Left Arrow Callout",
            ShapeKind::UpArrowCallout => "Up Arrow Callout",
            ShapeKind::LeftRightArrowCallout => "Left-Right Arrow Callout",
            ShapeKind::QuadArrowCallout => "Quad Arrow Callout",
            ShapeKind::CircularArrow => "Circular Arrow",

            ShapeKind::MathPlus => "Equation: Plus",
            ShapeKind::MathMinus => "Equation: Minus",
            ShapeKind::MathMultiply => "Equation: Multiply",
            ShapeKind::MathDivide => "Equation: Divide",
            ShapeKind::MathEqual => "Equation: Equal",
            ShapeKind::MathNotEqual => "Equation: Not Equal",

            ShapeKind::FlowchartProcess => "Process",
            ShapeKind::FlowchartAlternateProcess => "Alternate Process",
            ShapeKind::FlowchartDecision => "Decision",
            ShapeKind::FlowchartData => "Data",
            ShapeKind::FlowchartPredefinedProcess => "Predefined Process",
            ShapeKind::FlowchartInternalStorage => "Internal Storage",
            ShapeKind::FlowchartDocument => "Document",
            ShapeKind::FlowchartMultidocument => "Multidocument",
            ShapeKind::FlowchartTerminator => "Terminator",
            ShapeKind::FlowchartPreparation => "Preparation",
            ShapeKind::FlowchartManualInput => "Manual Input",
            ShapeKind::FlowchartManualOperation => "Manual Operation",
            ShapeKind::FlowchartConnector => "Connector",
            ShapeKind::FlowchartOffpageConnector => "Off-page Connector",
            ShapeKind::FlowchartCard => "Card",
            ShapeKind::FlowchartPunchedTape => "Punched Tape",
            ShapeKind::FlowchartSummingJunction => "Summing Junction",
            ShapeKind::FlowchartOr => "Or",
            ShapeKind::FlowchartCollate => "Collate",
            ShapeKind::FlowchartSort => "Sort",
            ShapeKind::FlowchartExtract => "Extract",
            ShapeKind::FlowchartMerge => "Merge",
            ShapeKind::FlowchartStoredData => "Stored Data",
            ShapeKind::FlowchartDelay => "Delay",
            ShapeKind::FlowchartSequentialData => "Sequential Data",
            ShapeKind::FlowchartMagneticDisk => "Magnetic Disk",
            ShapeKind::FlowchartDirectAccessStorage => "Direct Access Storage",
            ShapeKind::FlowchartDisplay => "Display",

            ShapeKind::Explosion1 => "Explosion 1",
            ShapeKind::Explosion2 => "Explosion 2",
            ShapeKind::Star4 => "4-Point Star",
            ShapeKind::Star => "5-Point Star",
            ShapeKind::Star6 => "6-Point Star",
            ShapeKind::Star7 => "7-Point Star",
            ShapeKind::Star8 => "8-Point Star",
            ShapeKind::Star10 => "10-Point Star",
            ShapeKind::Star12 => "12-Point Star",
            ShapeKind::Star16 => "16-Point Star",
            ShapeKind::Star24 => "24-Point Star",
            ShapeKind::Star32 => "32-Point Star",
            ShapeKind::UpRibbon => "Up Ribbon",
            ShapeKind::DownRibbon => "Down Ribbon",
            ShapeKind::CurvedUpRibbon => "Curved Up Ribbon",
            ShapeKind::CurvedDownRibbon => "Curved Down Ribbon",
            ShapeKind::VerticalScroll => "Vertical Scroll",
            ShapeKind::HorizontalScroll => "Horizontal Scroll",
            ShapeKind::Wave => "Wave",
            ShapeKind::DoubleWave => "Double Wave",

            ShapeKind::RectangularCallout => "Rectangular Callout",
            ShapeKind::RoundedRectangularCallout => "Rounded Rectangular Callout",
            ShapeKind::OvalCallout => "Oval Callout",
            ShapeKind::CloudCallout => "Cloud Callout",
            ShapeKind::LineCallout1 => "Line Callout 1",
            ShapeKind::LineCallout2 => "Line Callout 2",
            ShapeKind::LineCallout3 => "Line Callout 3",
            ShapeKind::LineCallout4 => "Line Callout 4",
            ShapeKind::LineCallout1AccentBar => "Line Callout 1 (Accent Bar)",
            ShapeKind::LineCallout2AccentBar => "Line Callout 2 (Accent Bar)",
            ShapeKind::LineCallout3AccentBar => "Line Callout 3 (Accent Bar)",
            ShapeKind::LineCallout4AccentBar => "Line Callout 4 (Accent Bar)",

            ShapeKind::DrawingCanvas => "New Drawing Canvas",
        }
    }

    /// Preset geometry identifier in DrawingML (ECMA-376 / ISO 29500 `a:prstGeom prst="..."`).
    pub fn ooxml_prst(&self) -> &'static str {
        match self {
            ShapeKind::Rectangle | ShapeKind::TextBox | ShapeKind::DrawingCanvas => "rect",
            ShapeKind::RoundedRectangle => "roundRect",
            ShapeKind::SingleCornerSnipped => "snip1Rect",
            ShapeKind::TopCornersSnipped => "snip2SameRect",
            ShapeKind::DiagonalCornersSnipped => "snip2DiagRect",
            ShapeKind::SnippedAndRounded => "snipRoundRect",
            ShapeKind::RoundSingleCorner => "round1Rect",
            ShapeKind::RoundSameSideCorner => "round2SameRect",
            ShapeKind::RoundDiagonalCorner => "round2DiagRect",

            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Triangle => "triangle",
            ShapeKind::RightTriangle => "rtTriangle",
            ShapeKind::Parallelogram => "parallelogram",
            ShapeKind::Trapezoid => "trapezoid",
            ShapeKind::Diamond => "diamond",
            ShapeKind::Pentagon => "pentagon",
            ShapeKind::Hexagon => "hexagon",
            ShapeKind::Heptagon => "heptagon",
            ShapeKind::Octagon => "octagon",
            ShapeKind::Decagon => "decagon",
            ShapeKind::Dodecagon => "dodecagon",
            ShapeKind::Pie => "pie",
            ShapeKind::Chord => "chord",
            ShapeKind::Teardrop => "teardrop",
            ShapeKind::Frame => "frame",
            ShapeKind::HalfFrame => "halfFrame",
            ShapeKind::LShape => "lShape",
            ShapeKind::DiagonalStripe => "diagStripe",
            ShapeKind::Cross => "cross",
            ShapeKind::Plaque => "plaque",
            ShapeKind::Cylinder => "can",
            ShapeKind::Cube => "cube",
            ShapeKind::Bevel => "bevel",
            ShapeKind::Donut => "donut",
            ShapeKind::NoSymbol => "noSmoking",
            ShapeKind::BlockArc => "blockArc",
            ShapeKind::FoldedCorner => "foldedCorner",
            ShapeKind::SmileyFace => "smileyFace",
            ShapeKind::Heart => "heart",
            ShapeKind::LightningBolt => "lightningBolt",
            ShapeKind::Sun => "sun",
            ShapeKind::Moon => "moon",
            ShapeKind::Cloud => "cloud",
            ShapeKind::Arc => "arc",
            ShapeKind::DoubleBracket => "bracketPair",
            ShapeKind::DoubleBrace => "bracePair",
            ShapeKind::LeftBracket => "leftBracket",
            ShapeKind::RightBracket => "rightBracket",
            ShapeKind::LeftBrace => "leftBrace",
            ShapeKind::RightBrace => "rightBrace",

            ShapeKind::Line => "line",
            ShapeKind::Arrow => "rightArrow",
            ShapeKind::DoubleArrow => "straightConnector1",
            ShapeKind::ElbowConnector => "bentConnector2",
            ShapeKind::ElbowArrow => "bentConnector3",
            ShapeKind::ElbowDoubleArrow => "bentConnector4",
            ShapeKind::CurvedConnector => "curvedConnector2",
            ShapeKind::CurvedArrow => "curvedConnector3",
            ShapeKind::CurvedDoubleArrow => "curvedConnector4",
            ShapeKind::Curve => "curvedConnector5",
            ShapeKind::Freeform => "freeform",
            ShapeKind::Scribble => "scribble",

            ShapeKind::LeftArrow => "leftArrow",
            ShapeKind::UpArrow => "upArrow",
            ShapeKind::DownArrow => "downArrow",
            ShapeKind::LeftRightArrow => "leftRightArrow",
            ShapeKind::UpDownArrow => "upDownArrow",
            ShapeKind::QuadArrow => "quadArrow",
            ShapeKind::LeftRightUpArrow => "leftRightUpArrow",
            ShapeKind::BentArrow => "bentArrow",
            ShapeKind::UTurnArrow => "uturnArrow",
            ShapeKind::LeftUpArrow => "leftUpArrow",
            ShapeKind::BentUpArrow => "bentUpArrow",
            ShapeKind::StripedRightArrow => "stripedRightArrow",
            ShapeKind::NotchedRightArrow => "notchedRightArrow",
            ShapeKind::PentagonArrow => "homePlate",
            ShapeKind::Chevron => "chevron",
            ShapeKind::RightArrowCallout => "rightArrowCallout",
            ShapeKind::DownArrowCallout => "downArrowCallout",
            ShapeKind::LeftArrowCallout => "leftArrowCallout",
            ShapeKind::UpArrowCallout => "upArrowCallout",
            ShapeKind::LeftRightArrowCallout => "leftRightArrowCallout",
            ShapeKind::QuadArrowCallout => "quadArrowCallout",
            ShapeKind::CircularArrow => "circularArrow",

            ShapeKind::MathPlus => "mathPlus",
            ShapeKind::MathMinus => "mathMinus",
            ShapeKind::MathMultiply => "mathMultiply",
            ShapeKind::MathDivide => "mathDivide",
            ShapeKind::MathEqual => "mathEqual",
            ShapeKind::MathNotEqual => "mathNotEqual",

            ShapeKind::FlowchartProcess => "flowChartProcess",
            ShapeKind::FlowchartAlternateProcess => "flowChartAlternateProcess",
            ShapeKind::FlowchartDecision => "flowChartDecision",
            ShapeKind::FlowchartData => "flowChartInputOutput",
            ShapeKind::FlowchartPredefinedProcess => "flowChartPredefinedProcess",
            ShapeKind::FlowchartInternalStorage => "flowChartInternalStorage",
            ShapeKind::FlowchartDocument => "flowChartDocument",
            ShapeKind::FlowchartMultidocument => "flowChartMultidocument",
            ShapeKind::FlowchartTerminator => "flowChartTerminator",
            ShapeKind::FlowchartPreparation => "flowChartPreparation",
            ShapeKind::FlowchartManualInput => "flowChartManualInput",
            ShapeKind::FlowchartManualOperation => "flowChartManualOperation",
            ShapeKind::FlowchartConnector => "flowChartConnector",
            ShapeKind::FlowchartOffpageConnector => "flowChartOffpageConnector",
            ShapeKind::FlowchartCard => "flowChartPunchedCard",
            ShapeKind::FlowchartPunchedTape => "flowChartPunchedTape",
            ShapeKind::FlowchartSummingJunction => "flowChartSummingJunction",
            ShapeKind::FlowchartOr => "flowChartOr",
            ShapeKind::FlowchartCollate => "flowChartCollate",
            ShapeKind::FlowchartSort => "flowChartSort",
            ShapeKind::FlowchartExtract => "flowChartExtract",
            ShapeKind::FlowchartMerge => "flowChartMerge",
            ShapeKind::FlowchartStoredData => "flowChartOnlineStorage",
            ShapeKind::FlowchartDelay => "flowChartDelay",
            ShapeKind::FlowchartSequentialData => "flowChartMagneticTape",
            ShapeKind::FlowchartMagneticDisk => "flowChartMagneticDisk",
            ShapeKind::FlowchartDirectAccessStorage => "flowChartMagneticDrum",
            ShapeKind::FlowchartDisplay => "flowChartDisplay",

            ShapeKind::Explosion1 => "star8",
            ShapeKind::Explosion2 => "star14",
            ShapeKind::Star4 => "star4",
            ShapeKind::Star => "star5",
            ShapeKind::Star6 => "star6",
            ShapeKind::Star7 => "star7",
            ShapeKind::Star8 => "star8",
            ShapeKind::Star10 => "star10",
            ShapeKind::Star12 => "star12",
            ShapeKind::Star16 => "star16",
            ShapeKind::Star24 => "star24",
            ShapeKind::Star32 => "star32",
            ShapeKind::UpRibbon => "ribbon",
            ShapeKind::DownRibbon => "ribbon2",
            ShapeKind::CurvedUpRibbon => "ellipseRibbon",
            ShapeKind::CurvedDownRibbon => "ellipseRibbon2",
            ShapeKind::VerticalScroll => "verticalScroll",
            ShapeKind::HorizontalScroll => "horizontalScroll",
            ShapeKind::Wave => "wave",
            ShapeKind::DoubleWave => "doubleWave",

            ShapeKind::RectangularCallout => "wedgeRectCallout",
            ShapeKind::RoundedRectangularCallout => "wedgeRoundRectCallout",
            ShapeKind::OvalCallout => "wedgeEllipseCallout",
            ShapeKind::CloudCallout => "cloudCallout",
            ShapeKind::LineCallout1 => "lineCallout1",
            ShapeKind::LineCallout2 => "lineCallout2",
            ShapeKind::LineCallout3 => "lineCallout3",
            ShapeKind::LineCallout4 => "lineCallout4",
            ShapeKind::LineCallout1AccentBar => "lineCallout1AccentBar",
            ShapeKind::LineCallout2AccentBar => "lineCallout2AccentBar",
            ShapeKind::LineCallout3AccentBar => "lineCallout3AccentBar",
            ShapeKind::LineCallout4AccentBar => "lineCallout4AccentBar",
        }
    }

    /// Resolve an OOXML DrawingML preset shape name to `ShapeKind`.
    pub fn from_ooxml_prst(prst: &str) -> Self {
        match prst {
            "roundRect" => ShapeKind::RoundedRectangle,
            "snip1Rect" => ShapeKind::SingleCornerSnipped,
            "snip2SameRect" => ShapeKind::TopCornersSnipped,
            "snip2DiagRect" => ShapeKind::DiagonalCornersSnipped,
            "snipRoundRect" => ShapeKind::SnippedAndRounded,
            "round1Rect" => ShapeKind::RoundSingleCorner,
            "round2SameRect" => ShapeKind::RoundSameSideCorner,
            "round2DiagRect" => ShapeKind::RoundDiagonalCorner,

            "ellipse" => ShapeKind::Ellipse,
            "triangle" => ShapeKind::Triangle,
            "rtTriangle" => ShapeKind::RightTriangle,
            "parallelogram" => ShapeKind::Parallelogram,
            "trapezoid" => ShapeKind::Trapezoid,
            "diamond" => ShapeKind::Diamond,
            "pentagon" => ShapeKind::Pentagon,
            "hexagon" => ShapeKind::Hexagon,
            "heptagon" => ShapeKind::Heptagon,
            "octagon" => ShapeKind::Octagon,
            "decagon" => ShapeKind::Decagon,
            "dodecagon" => ShapeKind::Dodecagon,
            "pie" => ShapeKind::Pie,
            "chord" => ShapeKind::Chord,
            "teardrop" => ShapeKind::Teardrop,
            "frame" => ShapeKind::Frame,
            "halfFrame" => ShapeKind::HalfFrame,
            "lShape" => ShapeKind::LShape,
            "diagStripe" => ShapeKind::DiagonalStripe,
            "cross" => ShapeKind::Cross,
            "plaque" => ShapeKind::Plaque,
            "can" => ShapeKind::Cylinder,
            "cube" => ShapeKind::Cube,
            "bevel" => ShapeKind::Bevel,
            "donut" => ShapeKind::Donut,
            "noSmoking" => ShapeKind::NoSymbol,
            "blockArc" => ShapeKind::BlockArc,
            "foldedCorner" => ShapeKind::FoldedCorner,
            "smileyFace" => ShapeKind::SmileyFace,
            "heart" => ShapeKind::Heart,
            "lightningBolt" => ShapeKind::LightningBolt,
            "sun" => ShapeKind::Sun,
            "moon" => ShapeKind::Moon,
            "cloud" => ShapeKind::Cloud,
            "arc" => ShapeKind::Arc,
            "bracketPair" => ShapeKind::DoubleBracket,
            "bracePair" => ShapeKind::DoubleBrace,
            "leftBracket" => ShapeKind::LeftBracket,
            "rightBracket" => ShapeKind::RightBracket,
            "leftBrace" => ShapeKind::LeftBrace,
            "rightBrace" => ShapeKind::RightBrace,

            "line" => ShapeKind::Line,
            "rightArrow" => ShapeKind::Arrow,
            "straightConnector1" => ShapeKind::DoubleArrow,
            "bentConnector2" => ShapeKind::ElbowConnector,
            "bentConnector3" => ShapeKind::ElbowArrow,
            "bentConnector4" => ShapeKind::ElbowDoubleArrow,
            "curvedConnector2" => ShapeKind::CurvedConnector,
            "curvedConnector3" => ShapeKind::CurvedArrow,
            "curvedConnector4" => ShapeKind::CurvedDoubleArrow,
            "curvedConnector5" => ShapeKind::Curve,
            "freeform" => ShapeKind::Freeform,
            "scribble" => ShapeKind::Scribble,

            "leftArrow" => ShapeKind::LeftArrow,
            "upArrow" => ShapeKind::UpArrow,
            "downArrow" => ShapeKind::DownArrow,
            "leftRightArrow" => ShapeKind::LeftRightArrow,
            "upDownArrow" => ShapeKind::UpDownArrow,
            "quadArrow" => ShapeKind::QuadArrow,
            "leftRightUpArrow" => ShapeKind::LeftRightUpArrow,
            "bentArrow" => ShapeKind::BentArrow,
            "uturnArrow" => ShapeKind::UTurnArrow,
            "leftUpArrow" => ShapeKind::LeftUpArrow,
            "bentUpArrow" => ShapeKind::BentUpArrow,
            "stripedRightArrow" => ShapeKind::StripedRightArrow,
            "notchedRightArrow" => ShapeKind::NotchedRightArrow,
            "homePlate" => ShapeKind::PentagonArrow,
            "chevron" => ShapeKind::Chevron,
            "rightArrowCallout" => ShapeKind::RightArrowCallout,
            "downArrowCallout" => ShapeKind::DownArrowCallout,
            "leftArrowCallout" => ShapeKind::LeftArrowCallout,
            "upArrowCallout" => ShapeKind::UpArrowCallout,
            "leftRightArrowCallout" => ShapeKind::LeftRightArrowCallout,
            "quadArrowCallout" => ShapeKind::QuadArrowCallout,
            "circularArrow" => ShapeKind::CircularArrow,

            "mathPlus" => ShapeKind::MathPlus,
            "mathMinus" => ShapeKind::MathMinus,
            "mathMultiply" => ShapeKind::MathMultiply,
            "mathDivide" => ShapeKind::MathDivide,
            "mathEqual" => ShapeKind::MathEqual,
            "mathNotEqual" => ShapeKind::MathNotEqual,

            "flowChartProcess" => ShapeKind::FlowchartProcess,
            "flowChartAlternateProcess" => ShapeKind::FlowchartAlternateProcess,
            "flowChartDecision" => ShapeKind::FlowchartDecision,
            "flowChartInputOutput" => ShapeKind::FlowchartData,
            "flowChartPredefinedProcess" => ShapeKind::FlowchartPredefinedProcess,
            "flowChartInternalStorage" => ShapeKind::FlowchartInternalStorage,
            "flowChartDocument" => ShapeKind::FlowchartDocument,
            "flowChartMultidocument" => ShapeKind::FlowchartMultidocument,
            "flowChartTerminator" => ShapeKind::FlowchartTerminator,
            "flowChartPreparation" => ShapeKind::FlowchartPreparation,
            "flowChartManualInput" => ShapeKind::FlowchartManualInput,
            "flowChartManualOperation" => ShapeKind::FlowchartManualOperation,
            "flowChartConnector" => ShapeKind::FlowchartConnector,
            "flowChartOffpageConnector" => ShapeKind::FlowchartOffpageConnector,
            "flowChartPunchedCard" => ShapeKind::FlowchartCard,
            "flowChartPunchedTape" => ShapeKind::FlowchartPunchedTape,
            "flowChartSummingJunction" => ShapeKind::FlowchartSummingJunction,
            "flowChartOr" => ShapeKind::FlowchartOr,
            "flowChartCollate" => ShapeKind::FlowchartCollate,
            "flowChartSort" => ShapeKind::FlowchartSort,
            "flowChartExtract" => ShapeKind::FlowchartExtract,
            "flowChartMerge" => ShapeKind::FlowchartMerge,
            "flowChartOnlineStorage" => ShapeKind::FlowchartStoredData,
            "flowChartDelay" => ShapeKind::FlowchartDelay,
            "flowChartMagneticTape" => ShapeKind::FlowchartSequentialData,
            "flowChartMagneticDisk" => ShapeKind::FlowchartMagneticDisk,
            "flowChartMagneticDrum" => ShapeKind::FlowchartDirectAccessStorage,
            "flowChartDisplay" => ShapeKind::FlowchartDisplay,

            "star4" => ShapeKind::Star4,
            "star5" => ShapeKind::Star,
            "star6" => ShapeKind::Star6,
            "star7" => ShapeKind::Star7,
            "star8" => ShapeKind::Star8,
            "star10" => ShapeKind::Star10,
            "star12" => ShapeKind::Star12,
            "star16" => ShapeKind::Star16,
            "star24" => ShapeKind::Star24,
            "star32" => ShapeKind::Star32,
            "ribbon" => ShapeKind::UpRibbon,
            "ribbon2" => ShapeKind::DownRibbon,
            "ellipseRibbon" => ShapeKind::CurvedUpRibbon,
            "ellipseRibbon2" => ShapeKind::CurvedDownRibbon,
            "verticalScroll" => ShapeKind::VerticalScroll,
            "horizontalScroll" => ShapeKind::HorizontalScroll,
            "wave" => ShapeKind::Wave,
            "doubleWave" => ShapeKind::DoubleWave,

            "wedgeRectCallout" => ShapeKind::RectangularCallout,
            "wedgeRoundRectCallout" => ShapeKind::RoundedRectangularCallout,
            "wedgeEllipseCallout" => ShapeKind::OvalCallout,
            "cloudCallout" => ShapeKind::CloudCallout,
            "lineCallout1" => ShapeKind::LineCallout1,
            "lineCallout2" => ShapeKind::LineCallout2,
            "lineCallout3" => ShapeKind::LineCallout3,
            "lineCallout4" => ShapeKind::LineCallout4,
            "lineCallout1AccentBar" => ShapeKind::LineCallout1AccentBar,
            "lineCallout2AccentBar" => ShapeKind::LineCallout2AccentBar,
            "lineCallout3AccentBar" => ShapeKind::LineCallout3AccentBar,
            "lineCallout4AccentBar" => ShapeKind::LineCallout4AccentBar,

            _ => ShapeKind::Rectangle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    #[default]
    Footnote,
    Endnote,
}

/// An inline object anchored at a U+FFFC in the paragraph text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InlineObject {
    Image {
        /// Key in `Document::media`.
        media: String,
        /// Display size, points.
        w: f32,
        h: f32,
        #[serde(default)]
        alt: String,
        #[serde(default)]
        float: Float,
        /// Crop (left, top, right, bottom) fractions 0..1.
        #[serde(default)]
        crop: [f32; 4],
    },
    Shape {
        kind: ShapeKind,
        w: f32,
        h: f32,
        fill: Option<crate::props::Rgb>,
        stroke: Option<crate::props::Rgb>,
        #[serde(default)]
        stroke_width: f32,
        #[serde(default)]
        float: Float,
        /// Text box content: `Document::parts` id.
        #[serde(default)]
        story: Option<u32>,
    },
    /// A field: `instr` is the field code (`PAGE`, `NUMPAGES`, `DATE \@ "M/d/yyyy"`, `TOC \o "1-3"`…);
    /// `result` the cached display text.
    Field {
        instr: String,
        #[serde(default)]
        result: String,
        #[serde(default)]
        locked: bool,
    },
    NoteRef {
        kind: NoteKind,
        /// `Document::parts` id of the note's story.
        id: u32,
        /// Custom mark (empty = automatic number).
        #[serde(default)]
        custom: String,
    },
    BookmarkStart {
        name: String,
    },
    BookmarkEnd {
        name: String,
    },
    CommentStart {
        id: u32,
    },
    CommentEnd {
        id: u32,
    },
    /// An equation in linear format (`x=(-b±√(b^2-4ac))/2a`).
    Equation {
        linear: String,
        #[serde(default)]
        display: bool,
    },
    /// Something we don't model, kept for round-trip (raw XML of the source format).
    Opaque {
        format: String,
        xml: String,
        #[serde(default)]
        text: String,
    },
}

impl InlineObject {
    /// Zero-width markers don't take part in layout.
    pub fn is_marker(&self) -> bool {
        matches!(
            self,
            InlineObject::BookmarkStart { .. }
                | InlineObject::BookmarkEnd { .. }
                | InlineObject::CommentStart { .. }
                | InlineObject::CommentEnd { .. }
        )
    }
    pub fn is_floating(&self) -> bool {
        match self {
            InlineObject::Image { float, .. } | InlineObject::Shape { float, .. } => float.wrap != Wrap::Inline,
            _ => false,
        }
    }
    /// The text this object contributes to plain-text extraction.
    pub fn plain_text(&self) -> &str {
        match self {
            InlineObject::Field { result, .. } => result,
            InlineObject::Equation { linear, .. } => linear,
            InlineObject::Opaque { text, .. } => text,
            _ => "",
        }
    }
}

/// A paragraph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Paragraph {
    pub text: String,
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "ParaProps::is_empty")]
    pub props: ParaProps,
    /// Formatting of the paragraph mark (used when typing into an empty paragraph).
    #[serde(default, skip_serializing_if = "CharProps::is_empty")]
    pub mark: CharProps,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<InlineObject>,
    /// Set when this paragraph ends a section (OOXML keeps `w:sectPr` on the last paragraph).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Box<SectionProps>>,
    /// Change counter for caches (not part of equality).
    #[serde(skip, default = "next_rev")]
    pub rev: u64,
}

impl PartialEq for Paragraph {
    fn eq(&self, o: &Self) -> bool {
        self.text == o.text
            && self.runs == o.runs
            && self.props == o.props
            && self.mark == o.mark
            && self.objects == o.objects
            && self.section == o.section
    }
}

impl Default for Paragraph {
    fn default() -> Self {
        Paragraph::new()
    }
}

impl Paragraph {
    pub fn new() -> Self {
        Paragraph {
            text: String::new(),
            runs: Vec::new(),
            props: ParaProps::default(),
            mark: CharProps::default(),
            objects: Vec::new(),
            section: None,
            rev: next_rev(),
        }
    }
    /// A paragraph of plain text in one run.
    pub fn with_text(text: &str, props: CharProps) -> Self {
        let mut p = Paragraph::new();
        let clean: String = text.chars().filter(|c| *c != OBJ).collect();
        if !clean.is_empty() {
            p.runs.push(Run { len: clean.len(), props: props.clone() });
        }
        p.text = clean;
        p.mark = props;
        p
    }
    pub fn styled(mut self, style: &str) -> Self {
        self.props.style = Some(style.to_string());
        self
    }
    pub fn touch(&mut self) {
        self.rev = next_rev();
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    /// The text with objects removed or replaced by their text (fields → result).
    pub fn plain_text(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        let mut k = 0;
        for c in self.text.chars() {
            if c == OBJ {
                if let Some(o) = self.objects.get(k) {
                    out.push_str(o.plain_text());
                }
                k += 1;
            } else {
                out.push(c);
            }
        }
        out
    }

    fn check(&self, off: usize) -> Result<()> {
        if off > self.text.len() || !self.text.is_char_boundary(off) {
            return Err(DocError::BadOffset(off));
        }
        Ok(())
    }

    /// Clamp `off` to the text and back to a char boundary.
    pub fn clamp(&self, off: usize) -> usize {
        let mut o = off.min(self.text.len());
        while o > 0 && !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    /// Index of the run containing byte `off` and the offset where that run starts. At a run
    /// boundary the earlier run wins (typing continues the formatting to the left).
    fn run_at(&self, off: usize) -> Option<(usize, usize)> {
        let mut start = 0;
        for (i, r) in self.runs.iter().enumerate() {
            let end = start + r.len;
            if off < end || (off == end && off > start) {
                return Some((i, start));
            }
            start = end;
        }
        None
    }

    /// Formatting in effect at `off` (what typing there would get): the run to the left, or the
    /// first run at offset 0, or the paragraph mark when empty.
    pub fn props_at(&self, off: usize) -> &CharProps {
        if off == 0 {
            return self.runs.first().map(|r| &r.props).unwrap_or(&self.mark);
        }
        self.run_at(off).and_then(|(i, _)| self.runs.get(i)).map(|r| &r.props).unwrap_or(&self.mark)
    }

    /// Formatting of the character starting at `off`.
    pub fn props_of_char(&self, off: usize) -> &CharProps {
        let mut start = 0;
        for r in &self.runs {
            if off < start + r.len {
                return &r.props;
            }
            start += r.len;
        }
        &self.mark
    }

    /// Runs as (byte range, props).
    pub fn run_ranges(&self) -> impl Iterator<Item = (std::ops::Range<usize>, &CharProps)> {
        let mut s = 0;
        self.runs.iter().map(move |r| {
            let a = s;
            s += r.len;
            (a..s, &r.props)
        })
    }

    /// Number of objects in `text[..off]`.
    fn objects_before(&self, off: usize) -> usize {
        self.text.get(..off).map(|t| t.chars().filter(|c| *c == OBJ).count()).unwrap_or(0)
    }

    /// The object at byte `off` (which must hold U+FFFC).
    pub fn object_at(&self, off: usize) -> Option<&InlineObject> {
        if !self.text.get(off..)?.starts_with(OBJ) {
            return None;
        }
        self.objects.get(self.objects_before(off))
    }
    pub fn object_at_mut(&mut self, off: usize) -> Option<&mut InlineObject> {
        if !self.text.get(off..)?.starts_with(OBJ) {
            return None;
        }
        let k = self.objects_before(off);
        self.objects.get_mut(k)
    }
    /// Byte offsets of all objects, in order.
    pub fn object_offsets(&self) -> Vec<usize> {
        self.text.char_indices().filter(|(_, c)| *c == OBJ).map(|(i, _)| i).collect()
    }

    /// Insert plain text (U+FFFC is dropped) at `off` with `props`. Returns the inserted length.
    pub fn insert_text(&mut self, off: usize, s: &str, props: &CharProps) -> Result<usize> {
        self.check(off)?;
        let clean: std::borrow::Cow<str> = if s.contains(OBJ) { s.chars().filter(|c| *c != OBJ).collect::<String>().into() } else { s.into() };
        if clean.is_empty() {
            return Ok(0);
        }
        self.text.insert_str(off, &clean);
        self.insert_run(off, clean.len(), props.clone());
        self.touch();
        Ok(clean.len())
    }

    /// Insert an object at `off`.
    pub fn insert_object(&mut self, off: usize, obj: InlineObject, props: &CharProps) -> Result<()> {
        self.check(off)?;
        let k = self.objects_before(off);
        self.text.insert(off, OBJ);
        self.objects.insert(k.min(self.objects.len()), obj);
        self.insert_run(off, OBJ.len_utf8(), props.clone());
        self.touch();
        Ok(())
    }

    fn insert_run(&mut self, off: usize, len: usize, props: CharProps) {
        // Split the run at `off`, insert, merge neighbours.
        let mut start = 0;
        let mut idx = self.runs.len();
        for (i, r) in self.runs.iter().enumerate() {
            if off <= start {
                idx = i;
                break;
            }
            if off < start + r.len {
                // split
                let left = off - start;
                let right = r.len - left;
                let p = r.props.clone();
                if let Some(r) = self.runs.get_mut(i) {
                    r.len = left;
                }
                self.runs.insert(i + 1, Run { len: right, props: p });
                idx = i + 1;
                break;
            }
            start += r.len;
        }
        self.runs.insert(idx.min(self.runs.len()), Run { len, props });
        self.normalize();
    }

    /// Merge adjacent equal runs and drop empty ones.
    pub fn normalize(&mut self) {
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for r in self.runs.drain(..) {
            if r.len == 0 {
                continue;
            }
            match out.last_mut() {
                Some(l) if l.props == r.props => l.len += r.len,
                _ => out.push(r),
            }
        }
        self.runs = out;
        // Repair a run list that doesn't cover the text exactly (never expected, but files lie).
        let total: usize = self.runs.iter().map(|r| r.len).sum();
        if total != self.text.len() {
            if total < self.text.len() {
                let p = self.runs.last().map(|r| r.props.clone()).unwrap_or_else(|| self.mark.clone());
                self.runs.push(Run { len: self.text.len() - total, props: p });
            } else {
                let mut keep = self.text.len();
                for r in &mut self.runs {
                    r.len = r.len.min(keep);
                    keep -= r.len;
                }
                self.runs.retain(|r| r.len > 0);
            }
        }
    }

    /// Delete `a..b` (bytes). Objects inside are removed.
    pub fn delete(&mut self, a: usize, b: usize) -> Result<()> {
        let (a, b) = (a.min(b), a.max(b));
        self.check(a)?;
        self.check(b)?;
        if a == b {
            return Ok(());
        }
        if self.runs.is_empty() || a == 0 {
            // Keep the formatting of what's deleted for the mark when emptying.
            if b == self.text.len() {
                self.mark = self.props_of_char(a).clone();
            }
        }
        let k0 = self.objects_before(a);
        let k1 = self.objects_before(b);
        if k1 > k0 && k1 <= self.objects.len() {
            self.objects.drain(k0..k1);
        }
        self.text.replace_range(a..b, "");
        let mut start = 0;
        for r in &mut self.runs {
            let (rs, re) = (start, start + r.len);
            start = re;
            let ov = re.min(b).saturating_sub(rs.max(a));
            r.len -= ov.min(r.len);
        }
        self.normalize();
        self.touch();
        Ok(())
    }

    /// Apply `f` to the formatting of `a..b`.
    pub fn format(&mut self, a: usize, b: usize, f: &dyn Fn(&mut CharProps)) -> Result<()> {
        let (a, b) = (a.min(b), a.max(b));
        self.check(a)?;
        self.check(b)?;
        if a == b {
            return Ok(());
        }
        let mut out = Vec::with_capacity(self.runs.len() + 2);
        let mut start = 0;
        for r in self.runs.drain(..) {
            let (rs, re) = (start, start + r.len);
            start = re;
            let cut = [rs, a.clamp(rs, re), b.clamp(rs, re), re];
            for w in cut.windows(2) {
                let (x, y) = (w[0], w[1]);
                if y <= x {
                    continue;
                }
                let mut p = r.props.clone();
                if x >= a && y <= b {
                    f(&mut p);
                }
                out.push(Run { len: y - x, props: p });
            }
        }
        self.runs = out;
        self.normalize();
        self.touch();
        Ok(())
    }

    /// Split at `off`: `self` keeps `..off`, the returned paragraph gets `off..` with the same
    /// paragraph properties (the section break, if any, moves to the second half).
    pub fn split_off(&mut self, off: usize) -> Result<Paragraph> {
        self.check(off)?;
        let k = self.objects_before(off);
        let tail_text = self.text.split_off(off);
        let tail_objs = if k <= self.objects.len() { self.objects.split_off(k) } else { Vec::new() };
        let mut tail_runs = Vec::new();
        let mut start = 0;
        let mut keep = Vec::new();
        for r in self.runs.drain(..) {
            let (rs, re) = (start, start + r.len);
            start = re;
            if re <= off {
                keep.push(r);
            } else if rs >= off {
                tail_runs.push(r);
            } else {
                keep.push(Run { len: off - rs, props: r.props.clone() });
                tail_runs.push(Run { len: re - off, props: r.props });
            }
        }
        self.runs = keep;
        let carry = self.props_at(off).clone();
        let mut tail = Paragraph {
            text: tail_text,
            runs: tail_runs,
            props: self.props.clone(),
            mark: if self.text.is_empty() && off == 0 { self.mark.clone() } else { carry },
            objects: tail_objs,
            section: self.section.take(),
            rev: next_rev(),
        };
        if tail.text.is_empty() {
            tail.mark = self.props_at(off).clone();
        }
        self.normalize();
        tail.normalize();
        self.touch();
        Ok(tail)
    }

    /// Append `other`'s content (its paragraph properties are dropped; its section break kept).
    pub fn append(&mut self, other: Paragraph) {
        let Paragraph { text, runs, objects, section, .. } = other;
        self.text.push_str(&text);
        self.runs.extend(runs);
        self.objects.extend(objects);
        if section.is_some() {
            self.section = section;
        }
        self.normalize();
        self.touch();
    }

    /// Previous grapheme boundary before `off` (0 at start).
    pub fn prev_boundary(&self, off: usize) -> usize {
        use unicode_segmentation::GraphemeCursor;
        let off = self.clamp(off);
        let mut c = GraphemeCursor::new(off, self.text.len(), true);
        c.prev_boundary(&self.text, 0).ok().flatten().unwrap_or(0)
    }
    /// Next grapheme boundary after `off` (len at end).
    pub fn next_boundary(&self, off: usize) -> usize {
        use unicode_segmentation::GraphemeCursor;
        let off = self.clamp(off);
        let mut c = GraphemeCursor::new(off, self.text.len(), true);
        c.next_boundary(&self.text, 0).ok().flatten().unwrap_or(self.text.len())
    }
    /// Start of the word at or before `off` (Ctrl+Left).
    pub fn word_start(&self, off: usize) -> usize {
        let off = self.clamp(off);
        let Some(before) = self.text.get(..off) else { return 0 };
        let mut idx = off;
        let mut seen_word = false;
        for (i, c) in before.char_indices().rev() {
            let w = c.is_alphanumeric() || c == '_' || c == '\'';
            if w {
                seen_word = true;
                idx = i;
            } else if seen_word {
                break;
            } else {
                idx = i;
            }
        }
        idx
    }
    /// End of the word after `off`, including trailing spaces (Ctrl+Right, like Word).
    pub fn word_end(&self, off: usize) -> usize {
        let off = self.clamp(off);
        let Some(after) = self.text.get(off..) else { return self.text.len() };
        let it = after.char_indices().peekable();
        let mut end = self.text.len();
        let first_word = after.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
        let mut in_space = false;
        for (i, c) in it {
            let w = c.is_alphanumeric() || c == '_' || c == '\'';
            if c == ' ' || c == NBSP {
                in_space = true;
                continue;
            }
            if in_space || (first_word && !w) || (!first_word && i > 0) {
                end = off + i;
                break;
            }
        }
        end
    }
    /// The word around `off` (double-click): (start, end) without trailing space.
    pub fn word_at(&self, off: usize) -> (usize, usize) {
        let off = self.clamp(off);
        let is_w = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
        let mut a = off;
        for (i, c) in self.text.get(..off).unwrap_or("").char_indices().rev() {
            if !is_w(c) {
                break;
            }
            a = i;
        }
        let mut b = off;
        for (i, c) in self.text.get(off..).unwrap_or("").char_indices() {
            if !is_w(c) {
                if i == 0 && a == off {
                    b = off + c.len_utf8();
                }
                break;
            }
            b = off + i + c.len_utf8();
        }
        // Word also selects the trailing spaces.
        let mut e = b;
        for (i, c) in self.text.get(b..).unwrap_or("").char_indices() {
            if c != ' ' {
                break;
            }
            e = b + i + 1;
        }
        (a, e)
    }
    /// Sentence around `off` (Ctrl+click): ends after `.`, `!`, `?` and following spaces.
    pub fn sentence_at(&self, off: usize) -> (usize, usize) {
        let off = self.clamp(off);
        let t = &self.text;
        let mut a = 0;
        let mut prev_end = false;
        for (i, c) in t.char_indices() {
            if i >= off {
                break;
            }
            if prev_end && c != ' ' {
                a = i;
            }
            if matches!(c, '.' | '!' | '?') {
                prev_end = true;
            } else if c != ' ' {
                prev_end = false;
            }
        }
        let mut b = t.len();
        let mut ended = false;
        for (i, c) in t.get(off..).unwrap_or("").char_indices() {
            if ended && c != ' ' {
                b = off + i;
                break;
            }
            if matches!(c, '.' | '!' | '?') {
                ended = true;
            }
        }
        (a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> CharProps {
        CharProps { bold: Some(true), ..Default::default() }
    }

    #[test]
    fn insert_and_runs() {
        let mut p = Paragraph::with_text("Hello world", CharProps::default());
        p.insert_text(5, ",", &CharProps::default()).unwrap();
        assert_eq!(p.text, "Hello, world");
        assert_eq!(p.runs.len(), 1);
        p.insert_text(0, "Oh ", &bold()).unwrap();
        assert_eq!(p.runs.len(), 2);
        assert_eq!(p.runs[0].len, 3);
        assert!(p.insert_text(100, "x", &bold()).is_err());
    }

    #[test]
    fn insert_rejects_mid_char() {
        let mut p = Paragraph::with_text("é", CharProps::default());
        assert!(p.insert_text(1, "x", &bold()).is_err());
    }

    #[test]
    fn format_splits_runs() {
        let mut p = Paragraph::with_text("abcdef", CharProps::default());
        p.format(2, 4, &|c| c.bold = Some(true)).unwrap();
        assert_eq!(p.runs.iter().map(|r| r.len).collect::<Vec<_>>(), vec![2, 2, 2]);
        assert_eq!(p.props_of_char(2).bold, Some(true));
        p.format(0, 6, &|c| c.bold = Some(true)).unwrap();
        assert_eq!(p.runs.len(), 1);
    }

    #[test]
    fn delete_merges() {
        let mut p = Paragraph::with_text("abcdef", CharProps::default());
        p.format(2, 4, &|c| c.bold = Some(true)).unwrap();
        p.delete(2, 4).unwrap();
        assert_eq!(p.text, "abef");
        assert_eq!(p.runs.len(), 1);
        p.delete(0, 4).unwrap();
        assert!(p.runs.is_empty());
    }

    #[test]
    fn objects_track_text() {
        let mut p = Paragraph::with_text("ab", CharProps::default());
        p.insert_object(1, InlineObject::Field { instr: "PAGE".into(), result: "1".into(), locked: false }, &CharProps::default()).unwrap();
        p.insert_object(0, InlineObject::BookmarkStart { name: "x".into() }, &CharProps::default()).unwrap();
        assert_eq!(p.objects.len(), 2);
        assert!(matches!(p.object_at(0), Some(InlineObject::BookmarkStart { .. })));
        let off = p.object_offsets()[1];
        assert!(matches!(p.object_at(off), Some(InlineObject::Field { .. })));
        assert_eq!(p.plain_text(), "a1b");
        p.delete(0, 3).unwrap();
        assert_eq!(p.objects.len(), 1);
        let tail = p.split_off(p.object_offsets()[0]).unwrap();
        assert!(p.objects.is_empty());
        assert_eq!(tail.objects.len(), 1);
    }

    #[test]
    fn split_and_append() {
        let mut p = Paragraph::with_text("Hello world", bold());
        let t = p.split_off(5).unwrap();
        assert_eq!(p.text, "Hello");
        assert_eq!(t.text, " world");
        assert_eq!(t.runs[0].props.bold, Some(true));
        p.append(t);
        assert_eq!(p.text, "Hello world");
        assert_eq!(p.runs.len(), 1);
        let t = p.split_off(p.len()).unwrap();
        assert!(t.is_empty());
        assert_eq!(t.mark.bold, Some(true));
    }

    #[test]
    fn words() {
        let p = Paragraph::with_text("The quick  brown fox.", CharProps::default());
        assert_eq!(p.word_end(0), 4);
        assert_eq!(p.word_end(4), 11);
        assert_eq!(p.word_start(10), 4);
        assert_eq!(p.word_start(11), 4);
        assert_eq!(p.word_at(5), (4, 11));
        assert_eq!(p.sentence_at(3), (0, 21));
        assert_eq!(p.next_boundary(0), 1);
        assert_eq!(p.prev_boundary(0), 0);
    }

    proptest::proptest! {
        #[test]
        fn edits_keep_runs_consistent(ops in proptest::collection::vec((0u8..4, 0usize..40, 0usize..40, "[a-zé ]{0,5}"), 0..40)) {
            let mut p = Paragraph::with_text("seed text", CharProps::default());
            for (k, a, b, s) in ops {
                let a = p.clamp(a);
                let b = p.clamp(b);
                let _ = match k {
                    0 => p.insert_text(a, &s, &bold()).map(|_| ()),
                    1 => p.delete(a, b),
                    2 => p.format(a, b, &|c| c.italic = Some(true)),
                    _ => p.split_off(a).map(|t| p.append(t)),
                };
                let total: usize = p.runs.iter().map(|r| r.len).sum();
                proptest::prop_assert_eq!(total, p.text.len());
                proptest::prop_assert!(p.runs.iter().all(|r| r.len > 0));
                proptest::prop_assert_eq!(p.text.matches(OBJ).count(), p.objects.len());
            }
        }
    }
}
