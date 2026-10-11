//! 2D layout for equations stored in linear form.
//!
//! The linear text is a small, forgiving subset of the notation Word's own editor produces:
//! `x^2 + y_1`, `\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}`, `\sum_{i=1}^{n} x_i`,
//! `\begin{pmatrix} a & b \\ c & d \end{pmatrix}`. Equations we can't parse (or that would
//! be absurdly large) are drawn as the plain text of the linear string, as before — never an
//! error, never a panic.
//!
//! Coordinates inside a [`MathBox`] are points with y down and the origin on the text baseline
//! at the left edge: `runs` carry `(glyph, dx, dy)` offsets, `rules` are straight lines
//! (fraction bars, radicals, overlines). The paragraph line builder turns one equation into a
//! single cluster of the box's width; the display layer emits the runs and rules as draw items.

use wordcraft_doc::resolve::ResolvedChar;
use wordcraft_fonts::FaceRef;

/// Hard limits: the linear text comes from documents (and agents), so cap the work.
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4096;
/// Beyond this the equation would dwarf the page: fall back to text.
const MAX_SIDE: f32 = 4000.0;

/// One positioned run of glyphs in the equation box.
#[derive(Clone, Debug)]
pub struct MathRun {
    pub face: FaceRef,
    pub size: f32,
    /// Slant the glyphs when drawing (synthesised italic for the text face).
    pub italic: bool,
    pub bold: bool,
    /// Left edge of the run, box coordinates (baseline y = 0).
    pub x: f32,
    /// Baseline of the run, box coordinates (negative = above the axis).
    pub baseline: f32,
    /// (glyph id, dx from `x`, dy from `baseline`).
    pub glyphs: Vec<(u32, f32, f32)>,
    /// The text these glyphs show (PDF text extraction).
    pub text: String,
}

/// A straight line in the equation box (fraction bar, radical vinculum, overline…).
#[derive(Clone, Copy, Debug)]
pub struct MathRule {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub width: f32,
}

/// A laid-out equation.
#[derive(Clone, Debug)]
pub struct MathBox {
    pub width: f32,
    /// Ink above the baseline.
    pub ascent: f32,
    /// Ink below the baseline.
    pub descent: f32,
    pub runs: Vec<MathRun>,
    pub rules: Vec<MathRule>,
    /// The equation is empty: the runs show the "Type equation here." placeholder.
    pub placeholder: bool,
}

/// Which face a run is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathFace {
    /// The maths face: operators, digits, Greek, delimiters.
    Math,
    /// The paragraph's text face: variables and function names.
    Text,
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Node {
    /// A literal run: identifier, number, operator, function name…
    Run {
        text: String,
        face: MathFace,
        italic: bool,
        bold: bool,
    },
    Seq(Vec<Node>),
    Space(f32),
    Frac {
        num: Box<Node>,
        den: Box<Node>,
    },
    Sqrt {
        degree: Option<Box<Node>>,
        radicand: Box<Node>,
    },
    /// A line drawn over (or under) the box.
    Bar {
        over: bool,
        inner: Box<Node>,
    },
    Script {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    /// A large operator (∑ ∫ ∏ …): limits stack above and below it.
    BigOp {
        op: char,
    },
    /// "lim" with limits underneath.
    Lim {
        arg: Box<Node>,
    },
    Accent {
        ch: char,
        base: Box<Node>,
    },
    Delimited {
        open: char,
        close: char,
        inner: Box<Node>,
    },
    Matrix {
        rows: Vec<Vec<Node>>,
        open: char,
        close: char,
    },
}

// ---------------------------------------------------------------------------
// Symbol table
// ---------------------------------------------------------------------------

/// `\name` → the character it stands for (and whether it is italic, like variables).
fn symbol(name: &str) -> Option<(char, bool)> {
    let v = match name {
        // Greek (lower case is italic in maths, like variables).
        "alpha" => ('α', true),
        "beta" => ('β', true),
        "gamma" => ('γ', true),
        "delta" => ('δ', true),
        "epsilon" => ('ε', true),
        "varepsilon" => ('ε', true),
        "zeta" => ('ζ', true),
        "eta" => ('η', true),
        "theta" => ('θ', true),
        "vartheta" => ('ϑ', true),
        "iota" => ('ι', true),
        "kappa" => ('κ', true),
        "lambda" => ('λ', true),
        "mu" => ('μ', true),
        "nu" => ('ν', true),
        "xi" => ('ξ', true),
        "pi" => ('π', true),
        "rho" => ('ρ', true),
        "varrho" => ('ϱ', true),
        "sigma" => ('σ', true),
        "varsigma" => ('ς', true),
        "tau" => ('τ', true),
        "upsilon" => ('υ', true),
        "phi" => ('ϕ', true),
        "varphi" => ('φ', true),
        "chi" => ('χ', true),
        "psi" => ('ψ', true),
        "omega" => ('ω', true),
        "Gamma" => ('Γ', false),
        "Delta" => ('Δ', false),
        "Theta" => ('Θ', false),
        "Lambda" => ('Λ', false),
        "Xi" => ('Ξ', false),
        "Pi" => ('Π', false),
        "Sigma" => ('Σ', false),
        "Upsilon" => ('Υ', false),
        "Phi" => ('Φ', false),
        "Psi" => ('Ψ', false),
        "Omega" => ('Ω', false),
        // Operators and relations.
        "times" => ('×', false),
        "div" => ('÷', false),
        "pm" => ('±', false),
        "mp" => ('∓', false),
        "cdot" => ('⋅', false),
        "ast" => ('∗', false),
        "star" => ('⋆', false),
        "circ" => ('∘', false),
        "bullet" => ('∙', false),
        "oplus" => ('⊕', false),
        "ominus" => ('⊖', false),
        "otimes" => ('⊗', false),
        "oslash" => ('⊘', false),
        "odot" => ('⊙', false),
        "cup" => ('∪', false),
        "cap" => ('∩', false),
        "setminus" => ('∖', false),
        "subset" => ('⊂', false),
        "supset" => ('⊃', false),
        "subseteq" => ('⊆', false),
        "supseteq" => ('⊇', false),
        "in" => ('∈', false),
        "ni" => ('∋', false),
        "notin" => ('∉', false),
        "propto" => ('∝', false),
        "leq" | "le" => ('≤', false),
        "geq" | "ge" => ('≥', false),
        "neq" | "ne" => ('≠', false),
        "approx" => ('≈', false),
        "equiv" => ('≡', false),
        "cong" => ('≅', false),
        "sim" => ('∼', false),
        "simeq" => ('≃', false),
        "ll" => ('≪', false),
        "gg" => ('≫', false),
        "to" | "rightarrow" => ('→', false),
        "gets" | "leftarrow" => ('←', false),
        "Rightarrow" | "implies" => ('⇒', false),
        "Leftarrow" => ('⇐', false),
        "leftrightarrow" | "iff" => ('↔', false),
        "Leftrightarrow" => ('⇔', false),
        "mapsto" => ('↦', false),
        "uparrow" => ('↑', false),
        "downarrow" => ('↓', false),
        "updownarrow" => ('↕', false),
        // Big operators (limits stack under them via _ and ^).
        "sum" => ('∑', false),
        "prod" => ('∏', false),
        "coprod" => ('∐', false),
        "int" => ('∫', false),
        "iint" => ('∬', false),
        "iiint" => ('∭', false),
        "oint" => ('∮', false),
        "bigcup" => ('⋃', false),
        "bigcap" => ('⋂', false),
        "bigvee" => ('⋁', false),
        "bigwedge" => ('⋀', false),
        "bigoplus" => ('⨁', false),
        "bigotimes" => ('⨂', false),
        "biguplus" => ('⨄', false),
        // Misc symbols.
        "infty" => ('∞', false),
        "partial" => ('∂', false),
        "nabla" => ('∇', false),
        "forall" => ('∀', false),
        "exists" => ('∃', false),
        "nexists" => ('∄', false),
        "neg" | "lnot" => ('¬', false),
        "land" | "wedge" => ('∧', false),
        "lor" | "vee" => ('∨', false),
        "emptyset" | "varnothing" => ('∅', false),
        "angle" => ('∠', false),
        "perp" => ('⊥', false),
        "parallel" => ('∥', false),
        "prime" => ('′', false),
        "ell" => ('ℓ', true),
        "hbar" => ('ℏ', false),
        "Re" => ('ℜ', false),
        "Im" => ('ℑ', false),
        "aleph" => ('ℵ', false),
        "cdots" => ('⋯', false),
        "ldots" | "dots" => ('…', false),
        "degree" => ('°', false),
        "triangle" => ('△', false),
        "square" => ('□', false),
        "langle" => ('⟨', false),
        "rangle" => ('⟩', false),
        "backslash" => ('\\', false),
        _ => return None,
    };
    Some(v)
}

/// Function names (upright, not italic) that may carry limits underneath.
fn function_name(name: &str) -> Option<(&'static str, bool)> {
    let v = match name {
        "sin" => ("sin", false),
        "cos" => ("cos", false),
        "tan" => ("tan", false),
        "csc" => ("csc", false),
        "sec" => ("sec", false),
        "cot" => ("cot", false),
        "arcsin" => ("arcsin", false),
        "arccos" => ("arccos", false),
        "arctan" => ("arctan", false),
        "sinh" => ("sinh", false),
        "cosh" => ("cosh", false),
        "tanh" => ("tanh", false),
        "ln" => ("ln", false),
        "log" => ("log", false),
        "exp" => ("exp", false),
        "det" => ("det", false),
        "gcd" => ("gcd", false),
        "deg" => ("deg", false),
        "dim" => ("dim", false),
        "arg" => ("arg", false),
        "min" => ("min", false),
        "max" => ("max", false),
        "lim" => ("lim", true),
        "sup" => ("sup", false),
        "inf" => ("inf", false),
        _ => return None,
    };
    Some(v)
}

/// Accent commands: the character drawn above the base.
fn accent(name: &str) -> Option<char> {
    Some(match name {
        "hat" => 'ˆ',
        "tilde" | "widetilde" => '˜',
        "bar" | "overline" => '¯',
        "vec" => '→',
        "dot" => '˙',
        "ddot" => '¨',
        "dddot" => '⃛',
        "check" => 'ˇ',
        "breve" => '˘',
        "acute" => '´',
        "grave" => '`',
        "mathring" => '˚',
        _ => return None,
    })
}

/// Spacing command widths, in em.
fn spacing(name: &str) -> Option<f32> {
    Some(match name {
        "," | ":" => 0.17,
        ";" => 0.28,
        " " | "quad" => 1.0,
        "qquad" => 2.0,
        "!" => -0.17,
        _ => return None,
    })
}

/// Delimiters a matrix or bracket environment is drawn with.
fn env_delims(env: &str) -> Option<(char, char, bool)> {
    Some(match env {
        "pmatrix" => ('(', ')', false),
        "bmatrix" => ('[', ']', false),
        "Bmatrix" => ('{', '}', false),
        "vmatrix" => ('|', '|', false),
        "matrix" => ('\0', '\0', false),
        "cases" => ('{', '\0', false),
        "aligned" | "align" | "align*" => ('\0', '\0', true),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    chars: Vec<char>,
    pos: usize,
    nodes: usize,
}

type PResult<T> = Result<T, ()>;

impl Parser {
    fn new(linear: &str) -> Self {
        Parser { chars: linear.chars().collect(), pos: 0, nodes: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn budget(&mut self) -> PResult<()> {
        self.nodes += 1;
        if self.nodes > MAX_NODES || self.pos > self.chars.len() {
            return Err(());
        }
        Ok(())
    }

    /// A sequence of nodes, up to one of `stop` (which is left unconsumed) or the end.
    fn seq(&mut self, stop: &[char]) -> PResult<Vec<Node>> {
        let mut out: Vec<Node> = Vec::new();
        while let Some(c) = self.peek() {
            if stop.contains(&c) {
                break;
            }
            match c {
                ' ' | '\t' | '\n' | '\r' => {
                    self.pos += 1;
                    // A run of whitespace is one space; the next non-space starts fresh.
                    if out.last().is_none_or(|n| !matches!(n, Node::Space(_))) {
                        out.push(Node::Space(0.25));
                    }
                }
                '^' | '_' => {
                    self.pos += 1;
                    let sup = c == '^';
                    let first = self.script_operand()?;
                    let (sub, sup_node) = if sup {
                        let sub = if self.peek() == Some('_') {
                            self.pos += 1;
                            Some(Box::new(self.script_operand()?))
                        } else {
                            None
                        };
                        (sub, Some(Box::new(first)))
                    } else {
                        let sup_node = if self.peek() == Some('^') {
                            self.pos += 1;
                            Some(Box::new(self.script_operand()?))
                        } else {
                            None
                        };
                        (Some(Box::new(first)), sup_node)
                    };
                    let base = match out.pop() {
                        Some(b) => b,
                        None => Node::Seq(Vec::new()),
                    };
                    out.push(Node::Script { base: Box::new(base), sub, sup: sup_node });
                    self.budget()?;
                }
                '{' => {
                    self.pos += 1;
                    let inner = self.seq(&['}'])?;
                    if !self.eat('}') {
                        return Err(());
                    }
                    self.budget()?;
                    out.push(Node::Seq(inner));
                }
                '\\' => {
                    self.pos += 1;
                    let node = self.command()?;
                    if let Some(n) = node {
                        out.push(n);
                    }
                }
                '(' | '[' => {
                    // A bracket around a {…} group stretches to its height.
                    self.pos += 1;
                    if self.peek() == Some('{') {
                        self.pos += 1;
                        let inner = self.seq(&['}'])?;
                        if !self.eat('}') {
                            return Err(());
                        }
                        let close = if c == '(' { ')' } else { ']' };
                        if !self.eat(close) {
                            return Err(());
                        }
                        self.budget()?;
                        out.push(Node::Delimited { open: c, close, inner: Box::new(Node::Seq(inner)) });
                    } else {
                        out.push(Self::math_char(c));
                    }
                }
                '|' => {
                    self.pos += 1;
                    if self.peek() == Some('{') {
                        self.pos += 1;
                        let inner = self.seq(&['}'])?;
                        if !self.eat('}') || !self.eat('|') {
                            return Err(());
                        }
                        self.budget()?;
                        out.push(Node::Delimited { open: '|', close: '|', inner: Box::new(Node::Seq(inner)) });
                    } else {
                        out.push(Self::math_char(c));
                    }
                }
                ')' | ']' | '}' | '&' => {
                    // Plain linear text has brackets and ampersands of its own.
                    self.pos += 1;
                    out.push(Self::math_char(c));
                }
                _ => {
                    self.pos += 1;
                    out.push(Self::math_char(c));
                }
            }
        }
        Ok(out)
    }

    /// What `^` or `_` applies to: one character, or a `{…}` group.
    fn script_operand(&mut self) -> PResult<Node> {
        self.budget()?;
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let inner = self.seq(&['}'])?;
                if !self.eat('}') {
                    return Err(());
                }
                Ok(Node::Seq(inner))
            }
            Some('\\') => {
                self.pos += 1;
                Ok(self.command()?.unwrap_or(Node::Seq(Vec::new())))
            }
            Some(c) if !c.is_whitespace() => {
                self.pos += 1;
                Ok(Self::math_char(c))
            }
            _ => Ok(Node::Seq(Vec::new())),
        }
    }

    /// A `\…` command. `None` for spacing (already emitted) and the row separator.
    fn command(&mut self) -> PResult<Option<Node>> {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.pos += 1;
                if name.len() > 24 {
                    return Err(());
                }
            } else {
                break;
            }
        }
        self.budget()?;
        if name.is_empty() {
            // \\ (row separator), \, \; \! \  (spacing), or an escaped symbol.
            return match self.bump() {
                Some('\\') => Ok(None),
                Some(c) => {
                    let n = Self::math_char(c);
                    Ok(Some(n))
                }
                None => Err(()),
            };
        }
        // Structures first.
        if matches!(name.as_str(), "frac" | "dfrac" | "tfrac" | "cfrac" | "binom") {
            let (num, den) = (self.group()?, self.group()?);
            if name == "binom" {
                return Ok(Some(Node::Delimited { open: '(', close: ')', inner: Box::new(Node::Frac { num: Box::new(num), den: Box::new(den) }) }));
            }
            return Ok(Some(Node::Frac { num: Box::new(num), den: Box::new(den) }));
        }
        if name == "sqrt" {
            let degree = if self.peek() == Some('[') {
                self.pos += 1;
                let d = self.seq(&[']'])?;
                if !self.eat(']') {
                    return Err(());
                }
                Some(Box::new(Node::Seq(d)))
            } else {
                None
            };
            let radicand = self.group()?;
            return Ok(Some(Node::Sqrt { degree, radicand: Box::new(radicand) }));
        }
        if name == "overline" || name == "underline" {
            let inner = self.group()?;
            return Ok(Some(Node::Bar { over: name == "overline", inner: Box::new(inner) }));
        }
        if let Some(ch) = accent(&name) {
            let base = self.group()?;
            return Ok(Some(Node::Accent { ch, base: Box::new(base) }));
        }
        if name == "begin" {
            let inner = self.group()?;
            let env = plain_text(&inner);
            let Some((open, close, _)) = env_delims(&env) else { return Err(()) };
            let rows = self.env_body()?;
            return Ok(Some(Node::Matrix { rows, open, close }));
        }
        if name == "text" || name == "mathrm" || name == "operatorname" || name == "mathbf" || name == "mathit" || name == "mathsf" {
            let inner = self.group()?;
            let italic = name == "mathit";
            let face = MathFace::Text;
            let text = plain_text(&inner);
            return Ok(Some(Node::Run { text, face, italic, bold: name == "mathbf" }));
        }
        if name == "left" || name == "right" {
            // Sizing the delimiter to its content is more than we do: draw it as itself.
            return match self.bump() {
                Some(c) => Ok(Some(Self::math_char(c))),
                None => Err(()),
            };
        }
        if let Some((fname, limits)) = function_name(&name) {
            if limits {
                // lim, with optional _{…} underneath (the generic script attachment stacks
                // limits for BigOp/Lim bases).
                let mut sub = None;
                let mut sup = None;
                if self.peek() == Some('_') {
                    self.pos += 1;
                    sub = Some(Box::new(self.script_operand()?));
                }
                if self.peek() == Some('^') {
                    self.pos += 1;
                    sup = Some(Box::new(self.script_operand()?));
                }
                let arg = Box::new(Node::Seq(vec![Node::Run { text: "lim".into(), face: MathFace::Text, italic: false, bold: false }]));
                let mut node = Node::Lim { arg };
                if sub.is_some() || sup.is_some() {
                    node = Node::Script { base: Box::new(node), sub, sup };
                }
                return Ok(Some(node));
            }
            return Ok(Some(Node::Run { text: fname.into(), face: MathFace::Text, italic: false, bold: false }));
        }
        if let Some((ch, italic)) = symbol(&name) {
            return Ok(Some(if is_big_op(ch) {
                Node::BigOp { op: ch }
            } else {
                Node::Run { text: ch.to_string(), face: MathFace::Math, italic, bold: false }
            }));
        }
        if let Some(w) = spacing(&name) {
            return Ok(Some(Node::Space(w)));
        }
        // Unknown command: fail, so the caller falls back to one-line text.
        Err(())
    }

    fn math_char(c: char) -> Node {
        if c.is_ascii_digit() {
            Node::Run { text: c.to_string(), face: MathFace::Math, italic: false, bold: false }
        } else if c.is_alphabetic() {
            Node::Run { text: c.to_string(), face: MathFace::Text, italic: true, bold: false }
        } else {
            Node::Run { text: c.to_string(), face: MathFace::Math, italic: false, bold: false }
        }
    }

    /// A mandatory `{…}` argument.
    fn group(&mut self) -> PResult<Node> {
        self.budget()?;
        if self.peek() != Some('{') {
            // Tolerate a single token: \sqrt x, \frac xy.
            return self.script_operand();
        }
        self.pos += 1;
        let inner = self.seq(&['}'])?;
        if !self.eat('}') {
            return Err(());
        }
        Ok(Node::Seq(inner))
    }

    /// The body of a matrix environment: rows of cells, ending at `\end{…}`.
    fn env_body(&mut self) -> PResult<Vec<Vec<Node>>> {
        let mut rows = Vec::new();
        loop {
            if rows.len() > 64 {
                return Err(());
            }
            let mut cells: Vec<Node> = Vec::new();
            let mut cell: Vec<Node> = Vec::new();
            loop {
                if cells.len() > 32 {
                    return Err(());
                }
                let mut part = self.seq(&['&', '\\'])?;
                cell.append(&mut part);
                if self.env_ended()? {
                    cells.push(Node::Seq(cell));
                    rows.push(cells);
                    return Ok(rows);
                }
                if self.eat('&') {
                    cells.push(Node::Seq(std::mem::take(&mut cell)));
                    continue;
                }
                if self.peek() == Some('\\') && self.peek_at(1) == Some('\\') {
                    self.pos += 2;
                    cells.push(Node::Seq(std::mem::take(&mut cell)));
                    break;
                }
                if self.peek() == Some('\\') {
                    // A command inside a cell (\frac, \sqrt, a nested \begin…).
                    self.pos += 1;
                    if let Some(n) = self.command()? {
                        cell.push(n);
                    }
                    continue;
                }
                // The input ended without \end{…}.
                return Err(());
            }
            rows.push(cells);
        }
    }

    /// At `\end{…}`? Consumes it and returns true.
    fn env_ended(&mut self) -> PResult<bool> {
        if self.peek() != Some('\\') || self.peek_at(1) != Some('e') {
            return Ok(false);
        }
        let save = self.pos;
        self.pos += 1;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if name != "end" {
            self.pos = save;
            return Ok(false);
        }
        let _ = self.group()?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

struct Ctx {
    math: FaceRef,
    text: FaceRef,
    text_italic: FaceRef,
    /// Current size, points.
    em: f32,
    depth: usize,
}

impl Ctx {
    fn at(&self, k: f32) -> Ctx {
        Ctx { math: self.math, text: self.text, text_italic: self.text_italic, em: self.em * k, depth: self.depth + 1 }
    }
}

/// A laid-out box, before it becomes a [`MathBox`]: baseline at y = 0, y down.
#[derive(Clone, Default)]
struct Inner {
    width: f32,
    ascent: f32,
    descent: f32,
    runs: Vec<MathRun>,
    rules: Vec<MathRule>,
    /// Sub/superscripts on this box stack above and below (∑, lim) instead of beside it.
    stack_limits: bool,
}

impl Inner {
    fn empty() -> Self {
        Inner::default()
    }

    fn grow(&mut self, w: f32, asc: f32, desc: f32) {
        self.width = self.width.max(w);
        self.ascent = self.ascent.max(asc);
        self.descent = self.descent.max(desc);
    }
}

/// Place `child` inside `parent`'s coordinate space: shift it by (dx, dy) and merge.
fn place(parent: &mut Inner, child: Inner, dx: f32, dy: f32) {
    for mut r in child.runs {
        r.x += dx;
        r.baseline += dy;
        parent.runs.push(r);
    }
    for mut r in child.rules {
        r.x0 += dx;
        r.x1 += dx;
        r.y0 += dy;
        r.y1 += dy;
        parent.rules.push(r);
    }
    parent.grow(dx + child.width, child.ascent - dy, child.descent + dy);
    parent.stack_limits = parent.stack_limits || child.stack_limits;
}

/// A box from literal text, shaped in `face` (or the paragraph face), with per-character
/// fallback for glyphs the face lacks.
fn text_box(ctx: &Ctx, text: &str, face: MathFace, italic: bool, bold: bool) -> Inner {
    let mut b = Inner::empty();
    if text.is_empty() {
        return b;
    }
    let db = wordcraft_fonts::FontDb::global();
    let primary = match face {
        MathFace::Math => ctx.math,
        MathFace::Text => {
            if italic {
                ctx.text_italic
            } else {
                ctx.text
            }
        }
    };
    // Split into runs of one face; characters no face covers become gaps.
    let mut i = 0usize;
    let chars: Vec<char> = text.chars().collect();
    while i < chars.len() {
        let mut face_now: Option<FaceRef> = None;
        let mut j = i;
        let mut seg = String::new();
        let mut gap = 0.0f32;
        while j < chars.len() {
            let c = chars[j];
            let f = if primary.covers(c) || c.is_whitespace() || c.is_control() {
                Some(primary)
            } else {
                db.fallback_for(c, primary.id()).map(|f| FaceRef::of(&f))
            };
            match (f, face_now) {
                (Some(f), Some(cur)) if cur.id() == f.id() => {
                    seg.push(c);
                    j += 1;
                }
                (Some(f), None) => {
                    face_now = Some(f);
                    seg.push(c);
                    j += 1;
                }
                (None, _) => {
                    if face_now.is_some() || !seg.is_empty() {
                        break;
                    }
                    gap += ctx.em * 0.5;
                    j += 1;
                    if face_now.is_none() && gap > 0.0 {
                        // Flush the gap run.
                        b.grow(gap, 0.0, 0.0);
                        gap = 0.0;
                    }
                }
                (Some(_), Some(_)) => break,
            }
        }
        // A face is only chosen together with the segment's first character, so `seg` is
        // non-empty exactly when `face_now` is set.
        if let Some(f) = face_now {
            let shaped = wordcraft_fonts::shape(&f, &seg, &[], |c| c);
            let k = ctx.em / f.upem.max(1.0) as f32;
            let (a, d) = wordcraft_fonts::word::line_metrics(&f);
            let (a, d) = (a as f32 * k, d as f32 * k);
            let mut x = 0.0f32;
            let mut glyphs = Vec::with_capacity(shaped.len());
            for g in &shaped {
                glyphs.push((g.gid, x + g.x_offset as f32 * k, g.y_offset as f32 * k));
                x += g.x_advance as f32 * k;
            }
            let (a, d) = if glyphs.is_empty() { (0.0, 0.0) } else { (a.max(ctx.em * 0.75), d.max(ctx.em * 0.2)) };
            b.runs.push(MathRun { face: f, size: ctx.em, italic, bold, x: b.width, baseline: 0.0, glyphs, text: seg.clone() });
            b.grow(b.width + x, a, d);
        }
        i = j.max(i + 1);
    }
    b
}

fn layout(ctx: &Ctx, node: &Node) -> Inner {
    if ctx.depth > MAX_DEPTH {
        return Inner::empty();
    }
    match node {
        Node::Run { text, face, italic, bold } => text_box(ctx, text, *face, *italic, *bold),
        Node::Seq(v) => {
            let mut b = Inner::empty();
            for n in v {
                let child = layout(ctx, n);
                let dx = b.width;
                place(&mut b, child, dx, 0.0);
            }
            b
        }
        Node::Space(w) => {
            let mut b = Inner::empty();
            b.width = (w * ctx.em).max(0.0);
            b
        }
        Node::Frac { num, den } => {
            let n = layout(ctx, num);
            let d = layout(ctx, den);
            let (nw, na, nd) = (n.width, n.ascent, n.descent);
            let (dw, dd) = (d.width, d.descent);
            let gap = ctx.em * 0.16;
            let th = (ctx.em * 0.06).max(0.5);
            let pad = ctx.em * 0.12;
            let den_baseline = nd + gap + th + gap;
            let mut b = Inner::empty();
            place(&mut b, n, pad, 0.0);
            place(&mut b, d, pad, den_baseline);
            b.width = nw.max(dw) + 2.0 * pad;
            b.ascent = na;
            b.descent = den_baseline + dd;
            b.rules.push(MathRule { x0: 0.0, y0: nd + gap, x1: b.width, y1: nd + gap + th, width: th });
            b
        }
        Node::Sqrt { degree, radicand } => {
            let rad = layout(ctx, radicand);
            let sign = text_box(ctx, "\u{221A}", MathFace::Math, false, false);
            let (rw, ra, rd) = (rad.width, rad.ascent, rad.descent);
            let sign_d = sign.descent;
            let gap = ctx.em * 0.14;
            let th = (ctx.em * 0.05).max(0.4);
            let rx = sign.width.max(ctx.em * 0.8);
            let mut b = Inner::empty();
            place(&mut b, sign, 0.0, 0.0);
            place(&mut b, rad, rx, 0.0);
            if let Some(dg) = degree {
                let deg = layout(&ctx.at(0.55), dg);
                let dy = -(ra * 0.5) - ctx.em * 0.06;
                place(&mut b, deg, 0.0, dy);
            }
            let bar_y = -(ra + gap);
            b.ascent = ra + gap + th;
            b.descent = rd.max(sign_d);
            b.width = rx + rw;
            b.rules.push(MathRule { x0: rx, y0: bar_y, x1: rx + rw, y1: bar_y + th, width: th });
            b
        }
        Node::Bar { over, inner } => {
            let child = layout(ctx, inner);
            let (cw, ca, cd) = (child.width, child.ascent, child.descent);
            let gap = ctx.em * 0.12;
            let th = (ctx.em * 0.05).max(0.4);
            let bar_y = if *over { -(ca + gap) } else { cd + gap };
            let mut b = Inner::empty();
            place(&mut b, child, 0.0, 0.0);
            b.width = cw;
            b.ascent = if *over { ca + gap + th } else { ca };
            b.descent = if *over { cd } else { cd + gap + th };
            b.rules.push(MathRule { x0: 0.0, y0: bar_y, x1: cw, y1: bar_y + th, width: th });
            b
        }
        Node::Script { base, sub, sup } => {
            let bx = layout(ctx, base);
            let small = ctx.at(0.7);
            let mut b = Inner::empty();
            if bx.stack_limits {
                // Limits stack above and below (∑, lim): centre them on the base.
                let mut top = bx.ascent;
                let mut bottom = bx.descent;
                if let Some(s) = sup {
                    let sb = layout(&small, s);
                    let gap = ctx.em * 0.14;
                    place(&mut b, sb.clone(), 0.0, -(top + gap) - sb.ascent);
                    top = top + gap + sb.ascent + sb.descent;
                }
                if let Some(s) = sub {
                    let sb = layout(&small, s);
                    let gap = ctx.em * 0.14;
                    place(&mut b, sb.clone(), 0.0, bottom + gap + sb.ascent);
                    bottom = bottom + gap + sb.ascent + sb.descent;
                }
                place(&mut b, bx.clone(), 0.0, 0.0);
                b.ascent = top.max(bx.ascent);
                b.descent = bottom.max(bx.descent);
                b.width = b.width.max(bx.width);
                return b;
            }
            place(&mut b, bx.clone(), 0.0, 0.0);
            let sb = sub.as_ref().map(|s| layout(&small, s));
            let sp = sup.as_ref().map(|s| layout(&small, s));
            let mut right = bx.width;
            let (mut a, mut d) = (bx.ascent, bx.descent);
            if let Some(sb) = &sb {
                let y = bx.descent.max(ctx.em * 0.1) + ctx.em * 0.08 + sb.ascent;
                place(&mut b, sb.clone(), right, y);
                d = d.max(y + sb.descent);
                right += sb.width;
                if sp.is_some() {
                    right += ctx.em * 0.08;
                }
            }
            if let Some(sp) = &sp {
                let y = -(bx.ascent.max(ctx.em * 0.5) + ctx.em * 0.1) - sp.descent;
                place(&mut b, sp.clone(), right, y);
                a = a.max(-y + sp.ascent);
                right += sp.width;
            }
            b.ascent = a;
            b.descent = d;
            b.width = b.width.max(right);
            b
        }
        Node::BigOp { op } => {
            let mut b = text_box(&ctx.at(1.35), &op.to_string(), MathFace::Math, false, false);
            b.stack_limits = true;
            b
        }
        Node::Lim { arg } => {
            let mut b = text_box(ctx, "lim", MathFace::Text, false, false);
            let space = layout(ctx, &Node::Space(0.25));
            let dx = b.width;
            place(&mut b, space, dx, 0.0);
            let inner = layout(ctx, arg);
            let dx = b.width;
            place(&mut b, inner, dx, 0.0);
            b.stack_limits = true;
            b
        }
        Node::Accent { ch, base } => {
            let bx = layout(ctx, base);
            let acc = text_box(&ctx.at(0.8), &ch.to_string(), MathFace::Math, false, false);
            let mut b = Inner::empty();
            place(&mut b, bx.clone(), 0.0, 0.0);
            let y = -(bx.ascent + ctx.em * 0.12) - acc.descent;
            place(&mut b, acc.clone(), ((bx.width - acc.width) / 2.0).max(0.0), y);
            b.ascent = b.ascent.max(-y + acc.ascent);
            b.descent = b.descent.max(bx.descent);
            b.width = b.width.max(bx.width);
            b
        }
        Node::Delimited { open, inner, close } => {
            let child = layout(ctx, inner);
            let h = child.ascent + child.descent;
            let ob = if *open != '\0' { Some(delimiter(ctx, *open, h)) } else { None };
            let cb = if *close != '\0' { Some(delimiter(ctx, *close, h)) } else { None };
            let lw = ob.as_ref().map_or(0.0, |o| o.width);
            let rw = cb.as_ref().map_or(0.0, |c| c.width);
            let (cw, ca, cd) = (child.width, child.ascent, child.descent);
            let mut b = Inner::empty();
            if let Some(o) = ob {
                place(&mut b, o, 0.0, 0.0);
            }
            place(&mut b, child, lw, 0.0);
            if let Some(c) = cb {
                place(&mut b, c, lw + cw, 0.0);
            }
            b.width = lw + cw + rw;
            b.ascent = b.ascent.max(ca);
            b.descent = b.descent.max(cd);
            b
        }
        Node::Matrix { rows, open, close } => {
            let cells: Vec<Vec<Inner>> = rows.iter().take(64).map(|r| r.iter().take(32).map(|c| layout(ctx, c)).collect()).collect();
            let cols = cells.iter().map(|r| r.len()).max().unwrap_or(0).min(32);
            let hpad = ctx.em * 0.25;
            let vpad = ctx.em * 0.12;
            let mut col_w = vec![0.0f32; cols];
            for r in &cells {
                for (i, c) in r.iter().enumerate() {
                    if let Some(w) = col_w.get_mut(i) {
                        *w = w.max(c.width);
                    }
                }
            }
            let mut col_x = Vec::with_capacity(cols);
            let mut x = 0.0;
            for w in &col_w {
                col_x.push(x);
                x += w + 2.0 * hpad;
            }
            let grid_w = x;
            let row_h: Vec<f32> = cells.iter().map(|r| r.iter().map(|c| c.ascent + c.descent).fold(0.0f32, f32::max) + 2.0 * vpad).collect();
            let grid_h: f32 = row_h.iter().sum();
            let ob = if *open != '\0' { Some(delimiter(ctx, *open, grid_h)) } else { None };
            let cb = if *close != '\0' { Some(delimiter(ctx, *close, grid_h)) } else { None };
            let lw = ob.as_ref().map_or(0.0, |o| o.width.max(ctx.em * 0.35));
            let rw = cb.as_ref().map_or(0.0, |c| c.width.max(ctx.em * 0.35));
            let mut b = Inner::empty();
            if let Some(o) = ob {
                place(&mut b, o, 0.0, 0.0);
            }
            let mut y = 0.0f32;
            for (ri, r) in cells.iter().enumerate() {
                for (i, c) in r.iter().enumerate() {
                    let cx = lw + col_x.get(i).copied().unwrap_or(0.0) + hpad;
                    place(&mut b, c.clone(), cx, y + vpad + c.ascent);
                }
                y += row_h.get(ri).copied().unwrap_or(0.0);
            }
            if let Some(c) = cb {
                place(&mut b, c, lw + grid_w, 0.0);
            }
            b.width = lw + grid_w + rw;
            // The grid straddles the axis.
            b.ascent = (grid_h / 2.0).max(b.ascent);
            b.descent = (grid_h / 2.0).max(b.descent);
            b
        }
    }
}

/// A delimiter sized to `height`: same glyph, scaled (and shifted onto the axis).
fn delimiter(ctx: &Ctx, ch: char, height: f32) -> Inner {
    let base = text_box(ctx, &ch.to_string(), MathFace::Math, false, false);
    let h = (base.ascent + base.descent).max(ctx.em * 0.5);
    let k = ((height + ctx.em * 0.3) / h).clamp(1.0, 2.6);
    if k <= 1.02 {
        return base;
    }
    let big = text_box(&ctx.at(k), &ch.to_string(), MathFace::Math, false, false);
    // Centre the delimiter on the axis, then extend it to cover the content.
    let shift = (big.ascent - big.descent) * 0.5 + (height - (big.ascent + big.descent)) * 0.5;
    let mut b = Inner::empty();
    place(&mut b, big, 0.0, shift);
    b
}

fn is_big_op(c: char) -> bool {
    matches!(c, '∑' | '∏' | '∐' | '∫' | '∬' | '∭' | '∮' | '⋃' | '⋂' | '⋁' | '⋀' | '⨁' | '⨂' | '⨄')
}

/// The literal text of a node tree (for `\text{…}`).
fn plain_text(n: &Node) -> String {
    match n {
        Node::Run { text, .. } => text.clone(),
        Node::Seq(v) => v.iter().map(plain_text).collect(),
        Node::Space(_) => " ".into(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Lay out an equation in linear form for drawing inside a line of `rc`'s style.
///
/// `None` when the text can't be parsed or would be unreasonably large: the caller then draws
/// the linear text as ordinary text (the old behaviour).
pub fn layout_math(linear: &str, rc: &ResolvedChar) -> Option<MathBox> {
    let em = if rc.size.is_finite() && rc.size > 0.0 { rc.size.clamp(1.0, 4096.0) } else { 11.0 };
    let text = wordcraft_fonts::word::resolve(&rc.font, rc.bold, false).face;
    let text_italic = wordcraft_fonts::word::resolve(&rc.font, rc.bold, true).face;
    let ctx = Ctx { math: wordcraft_fonts::word::math_face(), text, text_italic, em, depth: 0 };
    if linear.trim().is_empty() {
        let mut b = text_box(&ctx, "Type equation here.", MathFace::Text, true, false);
        return Some(MathBox {
            width: b.width.max(1.0),
            ascent: b.ascent.max(em * 0.75),
            descent: b.descent.max(em * 0.25),
            runs: std::mem::take(&mut b.runs),
            rules: Vec::new(),
            placeholder: true,
        });
    }
    let node = match Parser::new(linear).seq(&[]) {
        Ok(v) => Node::Seq(v),
        Err(()) => return None,
    };
    let inner = layout(&ctx, &node);
    let (w, a, d) = (inner.width, inner.ascent, inner.descent);
    if !w.is_finite() || !a.is_finite() || !d.is_finite() {
        return None;
    }
    if w > MAX_SIDE || a + d > MAX_SIDE || inner.runs.len() > 8192 || inner.rules.len() > 4096 {
        return None;
    }
    Some(MathBox { width: w.max(1.0), ascent: a, descent: d, runs: inner.runs, rules: inner.rules, placeholder: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_doc::resolve::ResolvedChar;

    fn rc() -> ResolvedChar {
        ResolvedChar {
            font: "Source Serif 4".into(),
            size: 11.0,
            bold: false,
            italic: false,
            underline: Default::default(),
            underline_color: None,
            strike: false,
            double_strike: false,
            color: Default::default(),
            highlight: None,
            shading: None,
            vert_align: Default::default(),
            caps: false,
            small_caps: false,
            hidden: false,
            spacing: 0.0,
            scale: 100.0,
            position: 0.0,
            outline: false,
            shadow: false,
            emboss: false,
            engrave: false,
            link: None,
            ins: None,
            del: None,
            no_proof: false,
            lang: None,
        }
    }

    fn runs_text(b: &MathBox) -> String {
        b.runs.iter().map(|r| r.text.clone()).collect()
    }

    #[test]
    fn plain_linear_text_lays_out() {
        let b = layout_math("x=(-b±√(b^2-4ac))/2a", &rc()).expect("parses");
        assert!(b.width > 0.0 && b.ascent > 0.0);
        assert!(b.runs.iter().all(|r| r.glyphs.iter().all(|g| g.0 > 0)), "all glyphs exist");
    }

    #[test]
    fn script_raises_and_shrinks() {
        let b = layout_math("x^{2}", &rc()).expect("parses");
        let sup = b.runs.iter().find(|r| r.text == "2").expect("the superscript run");
        assert!(sup.baseline < 0.0, "superscript sits above the baseline: {}", sup.baseline);
        assert!(sup.size < 11.0, "superscript is smaller: {}", sup.size);
        let sub = layout_math("x_{1}", &rc()).expect("parses");
        let s = sub.runs.iter().find(|r| r.text == "1").expect("the subscript run");
        assert!(s.baseline > 0.0, "subscript sits below the baseline: {}", s.baseline);
    }

    #[test]
    fn fraction_has_a_bar_and_stacks() {
        let b = layout_math("\\frac{x}{y}", &rc()).expect("parses");
        assert_eq!(b.rules.len(), 1, "one fraction bar");
        let num = b.runs.iter().find(|r| r.text == "x").expect("numerator");
        let den = b.runs.iter().find(|r| r.text == "y").expect("denominator");
        assert!(num.baseline <= 0.01 && den.baseline > 0.0, "stacked: {num:?} {den:?}");
    }

    #[test]
    fn radical_and_accent_draw_rules_or_marks() {
        let b = layout_math("\\sqrt{x}", &rc()).expect("parses");
        assert_eq!(b.rules.len(), 1, "the vinculum");
        assert!(runs_text(&b).contains('\u{221A}'), "the radical sign: {}", runs_text(&b));
        let v = layout_math("\\vec{x}", &rc()).expect("parses");
        assert!(runs_text(&v).contains('→'), "the vector accent: {}", runs_text(&v));
    }

    #[test]
    fn big_operator_limits_stack() {
        let b = layout_math("\\sum_{i=1}^{n} x_i", &rc()).expect("parses");
        assert!(b.runs.iter().any(|r| r.baseline < -5.0), "limits above the operator");
        assert!(b.runs.iter().any(|r| r.baseline > 5.0), "limits below the operator");
    }

    #[test]
    fn matrix_lays_out_a_grid() {
        let b = layout_math("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", &rc()).expect("parses");
        let t = runs_text(&b);
        assert!(t.contains('a') && t.contains('b') && t.contains('c') && t.contains('d'), "{t}");
        assert!(t.contains('(') && t.contains(')'), "brackets: {t}");
        let a = b.runs.iter().find(|r| r.text == "a").expect("cell a");
        let c = b.runs.iter().find(|r| r.text == "c").expect("cell c");
        assert!(a.baseline < c.baseline, "the second row is below the first");
    }

    #[test]
    fn functions_stay_upright_and_greek_is_italic() {
        let b = layout_math("\\sin \\theta", &rc()).expect("parses");
        let sin = b.runs.iter().find(|r| r.text == "sin").expect("sin");
        assert!(!sin.italic, "function names are upright");
        let th = b.runs.iter().find(|r| r.text.contains('θ')).expect("theta");
        assert!(th.italic, "Greek variables are italic");
    }

    #[test]
    fn placeholder_for_empty_equations() {
        let b = layout_math("", &rc()).expect("placeholder");
        assert!(b.placeholder);
        assert!(runs_text(&b).contains("Type equation here."));
    }

    #[test]
    fn hostile_input_never_panics_and_falls_back() {
        // Unparseable commands fall back to text.
        assert!(layout_math("\\notacommand{x}", &rc()).is_none());
        // Deep nesting and huge inputs are refused, not stacked.
        let deep = "{".repeat(5000) + &"}".repeat(5000);
        assert!(layout_math(&deep, &rc()).is_none());
        let huge: String = std::iter::repeat_n('x', 100_000).collect();
        assert!(layout_math(&huge, &rc()).is_none());
        let runaway = "\\frac{a}{b}".repeat(5000);
        assert!(layout_math(&runaway, &rc()).is_none());
        // Missing arguments.
        assert!(layout_math("\\frac{", &rc()).is_none());
        assert!(layout_math("\\sqrt{", &rc()).is_none());
        assert!(layout_math("\\begin{pmatrix} a", &rc()).is_none());
        // Whatever happens, nothing is NaN.
        let mut size = wordcraft_doc::resolve::ResolvedChar { size: 1e6, ..rc() };
        if let Some(b) = layout_math("\\frac{\\sqrt{x}}{\\sum_{i}^{n}}", &size) {
            assert!(b.width.is_finite() && b.ascent.is_finite() && b.descent.is_finite());
        }
        size.size = f32::NAN;
        if let Some(b) = layout_math("x", &size) {
            assert!(b.width.is_finite());
        }
    }
}
