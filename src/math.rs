//! LaTeX math rendering for assistant markdown (`$…$` / `$$…$$`).
//!
//! Pipeline: mitex (LaTeX → Typst) → typst compile → typst-render → egui texture.
//! On mitex/typst failure, a local repair pass fixes collapsed matrix `\\` row
//! breaks and retries once before showing the raw fallback.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use egui::{Color32, ColorImage, TextureHandle, TextureOptions, Ui, Vec2};
use typst::text::Font;
use typst_as_lib::TypstEngine;

/// Maps mitex helper names onto Typst math primitives.
const MITEX_PREAMBLE: &str = r#"
#let mitexmathbf(it) = math.bold(math.upright(it))
#let mitexsqrt(..args) = {
  if args.pos().len() == 1 { $sqrt(#args.pos().at(0))$ }
  else if args.pos().len() == 2 { $root(#args.pos().at(0), #args.pos().at(1))$ }
}
#let mitexdisplay(it) = math.display(it)
#let mitexinline(it) = math.inline(it)
#let mitexscript(it) = math.script(it)
#let mitexsscript(it) = math.sscript(it)
#let mitexbold(it) = math.bold(math.upright(it))
#let mitexupright(it) = math.upright(it)
#let mitexitalic(it) = math.italic(it)
#let mitexsans(it) = math.sans(it)
#let mitexnot(it) = math.cancel(angle: 20deg, it)
#let mitexlabel(it) = none
#let mitexcaption(it) = none
#let pmatrix = math.mat.with(delim: "(")
#let bmatrix = math.mat.with(delim: "[")
#let Bmatrix = math.mat.with(delim: "{")
#let vmatrix = math.mat.with(delim: "|")
#let Vmatrix = math.mat.with(delim: "||")
#let aligned(..args) = args.pos().first()
#let gathered(..args) = args.pos().first()
#let mitexunderbrace(it) = math.underbrace(it)
#let mitexoverbrace(it) = math.overbrace(it)
#let stackrel(top, base) = math.attach(base, t: top)
#let overset(top, base) = math.attach(base, t: top)
#let textmath(it) = text(it)
#let textbf(it) = text(weight: "bold", it)
#let textit(it) = text(style: "italic", it)
#let textrm(it) = text(it)
#let tfrac(num, denom) = math.inline(math.frac(num, denom))
#let dfrac(num, denom) = math.display(math.frac(num, denom))
#let boxed(it) = box(stroke: 0.6pt, inset: (x: 4pt, y: 3pt), $it$)
#let negthinspace = h(-0.16667em)
#let xrightarrow(label) = $attach(arrow.r.long, t: #label)$
#let xleftarrow(label) = $attach(arrow.l.long, t: #label)$
"#;

const PIXEL_PER_PT: f32 = 2.5;

enum Entry {
    Ready { texture: TextureHandle, size: Vec2 },
    Failed(String),
}

#[derive(Default)]
pub struct MathCache {
    entries: HashMap<u64, Entry>,
}

impl MathCache {
    pub fn show(&mut self, ui: &mut Ui, latex: &str, inline: bool) {
        let dark = ui.visuals().dark_mode;
        let fg = ui.visuals().text_color();
        let bg = ui.visuals().panel_fill;
        let key = cache_key(latex, inline, dark, fg, bg);

        if !self.entries.contains_key(&key) {
            match render_formula(latex, inline, fg, bg) {
                Ok((image, size)) => {
                    let texture = ui.ctx().load_texture(
                        format!("math_{key:x}"),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.entries.insert(key, Entry::Ready { texture, size });
                }
                Err(err) => {
                    self.entries.insert(key, Entry::Failed(err));
                }
            }
        }

        match self.entries.get(&key) {
            Some(Entry::Ready { texture, size }) => {
                let sized = egui::load::SizedTexture::new(texture.id(), *size);
                let img = egui::Image::new(egui::ImageSource::Texture(sized));
                if inline {
                    ui.add(img);
                } else {
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        ui.add(img);
                    });
                    ui.add_space(6.0);
                }
            }
            Some(Entry::Failed(err)) => {
                let fallback = if inline {
                    format!("${latex}$")
                } else {
                    format!("$$\n{latex}\n$$")
                };
                ui.colored_label(ui.visuals().warn_fg_color, &fallback)
                    .on_hover_text(err);
            }
            None => {}
        }
    }
}

fn cache_key(latex: &str, inline: bool, dark: bool, fg: Color32, bg: Color32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    latex.hash(&mut h);
    inline.hash(&mut h);
    dark.hash(&mut h);
    fg.to_array().hash(&mut h);
    bg.to_array().hash(&mut h);
    h.finish()
}

fn math_fonts() -> &'static [Font] {
    static FONTS: OnceLock<Vec<Font>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut searcher = typst_kit::fonts::Fonts::searcher();
        searcher
            .include_system_fonts(false)
            .include_embedded_fonts(true);
        searcher
            .search()
            .fonts
            .iter()
            .filter_map(|slot| slot.get())
            .collect()
    })
}

fn render_formula(
    latex: &str,
    inline: bool,
    fg: Color32,
    bg: Color32,
) -> Result<(ColorImage, Vec2), String> {
    match render_formula_once(latex, inline, fg, bg) {
        Ok(v) => Ok(v),
        Err(err) => {
            let decoded = decode_entities(latex);
            match repair_latex(&decoded) {
                Some(fixed) => render_formula_once(&fixed, inline, fg, bg),
                None => Err(err),
            }
        }
    }
}

fn render_formula_once(
    latex: &str,
    inline: bool,
    fg: Color32,
    bg: Color32,
) -> Result<(ColorImage, Vec2), String> {
    let latex = decode_entities(latex);
    let typst_math = mitex::convert_math(&latex, None).map_err(|e| format!("mitex: {e}"))?;

    let margin = if inline {
        "(x: 0pt, y: 2pt)"
    } else {
        "(x: 8pt, y: 6pt)"
    };
    // No surrounding spaces → inline; spaces → display.
    let equation = if inline {
        format!("${typst_math}$")
    } else {
        format!("$ {typst_math} $")
    };

    let source = format!(
        "{preamble}\n#set page(width: auto, height: auto, margin: {margin}, fill: none)\n\
         #set text(size: 16pt, fill: black)\n{equation}",
        preamble = MITEX_PREAMBLE,
        margin = margin,
        equation = equation,
    );

    let engine = TypstEngine::builder()
        .main_file(source)
        .fonts(math_fonts().iter().cloned())
        .build();

    let doc: typst::layout::PagedDocument = engine
        .compile()
        .output
        .map_err(|e| format!("typst: {e}"))?;
    let page = doc.pages.first().ok_or_else(|| "typst: no pages".to_string())?;

    let pixmap = typst_render::render(page, PIXEL_PER_PT);
    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    if w == 0 || h == 0 {
        return Err("empty math render".into());
    }

    // typst-render is premultiplied RGBA; composite fg over bg via alpha.
    let mut lut = [bg; 256];
    for (a, slot) in lut.iter_mut().enumerate() {
        if a >= 3 {
            let boosted = (a as f32 / 255.0).powf(0.6).min(1.0);
            let inv = 1.0 - boosted;
            *slot = Color32::from_rgb(
                (fg.r() as f32 * boosted + bg.r() as f32 * inv) as u8,
                (fg.g() as f32 * boosted + bg.g() as f32 * inv) as u8,
                (fg.b() as f32 * boosted + bg.b() as f32 * inv) as u8,
            );
        }
    }
    let pixels: Vec<Color32> = pixmap
        .data()
        .chunks_exact(4)
        .map(|c| lut[c[3] as usize])
        .collect();

    let image = ColorImage {
        size: [w, h],
        pixels,
    };
    let size = Vec2::new(w as f32 / PIXEL_PER_PT, h as f32 / PIXEL_PER_PT);
    Ok((image, size))
}

/// Environments where `\\` separates rows; LLMs often collapse that to a lone `\`.
fn is_row_env(name: &str) -> bool {
    matches!(
        name,
        "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "matrix"
            | "smallmatrix"
            | "array"
            | "cases"
    )
}

/// Real TeX/mitex control words we must not rewrite as row breaks.
fn known_tex_command(name: &str) -> bool {
    matches!(
        name,
        // Greek
        "alpha" | "beta" | "gamma" | "delta" | "epsilon" | "varepsilon" | "zeta"
            | "eta" | "theta" | "vartheta" | "iota" | "kappa" | "lambda" | "mu"
            | "nu" | "xi" | "pi" | "varpi" | "rho" | "varrho" | "sigma" | "varsigma"
            | "tau" | "upsilon" | "phi" | "varphi" | "chi" | "psi" | "omega"
            | "Gamma" | "Delta" | "Theta" | "Lambda" | "Xi" | "Pi" | "Sigma"
            | "Upsilon" | "Phi" | "Psi" | "Omega"
        // Relations / ops
            | "cdot" | "times" | "div" | "pm" | "mp" | "circ" | "bullet" | "oplus"
            | "ominus" | "otimes" | "oslash" | "odot" | "star" | "ast" | "dagger"
            | "ddagger" | "amalg" | "cap" | "cup" | "uplus" | "sqcap" | "sqcup"
            | "vee" | "wedge" | "wr" | "land" | "lor" | "lnot" | "neg"
            | "le" | "leq" | "ge" | "geq" | "ne" | "neq" | "approx" | "equiv"
            | "sim" | "simeq" | "cong" | "propto" | "prec" | "succ" | "preceq"
            | "succeq" | "subset" | "supset" | "subseteq" | "supseteq" | "in"
            | "ni" | "notin" | "mid" | "parallel" | "perp" | "models" | "vdash"
            | "dashv" | "bowtie" | "smile" | "frown"
        // Arrows
            | "to" | "mapsto" | "rightarrow" | "leftarrow" | "Rightarrow"
            | "Leftarrow" | "leftrightarrow" | "Leftrightarrow" | "longrightarrow"
            | "longleftarrow" | "longmapsto" | "uparrow" | "downarrow"
            | "updownarrow" | "nearrow" | "searrow" | "swarrow" | "nwarrow"
            | "hookrightarrow" | "hookleftarrow" | "rightleftharpoons"
            | "iff" | "implies"
        // Delimiters / sizing / accents
            | "left" | "right" | "big" | "Big" | "bigg" | "Bigg"
            | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr"
            | "hat" | "widehat" | "tilde" | "widetilde" | "bar" | "vec"
            | "dot" | "ddot" | "overline" | "underline" | "overbrace"
            | "underbrace"
        // Functions / layout
            | "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin"
            | "arccos" | "arctan" | "sinh" | "cosh" | "tanh" | "log" | "ln"
            | "exp" | "det" | "dim" | "ker" | "deg" | "max" | "min" | "sup"
            | "inf" | "lim" | "limsup" | "liminf" | "Pr" | "gcd" | "hom"
            | "arg" | "frac" | "dfrac" | "tfrac" | "sqrt" | "binom" | "choose"
            | "overset" | "underset" | "stackrel"
            | "sum" | "prod" | "coprod" | "int" | "iint" | "iiint" | "oint"
            | "bigcup" | "bigcap" | "bigvee" | "bigwedge" | "bigoplus"
            | "bigotimes" | "bigodot" | "biguplus"
        // Misc symbols
            | "infty" | "partial" | "nabla" | "ell" | "hbar" | "Re" | "Im"
            | "wp" | "emptyset" | "varnothing" | "exists" | "forall" | "nexists"
            | "aleph" | "prime" | "backslash" | "angle" | "triangle"
            | "square" | "diamond" | "clubsuit" | "diamondsuit" | "heartsuit"
            | "spadesuit" | "S" | "P" | "dag" | "ddag"
        // Spacing / text
            | "quad" | "qquad" | "hspace" | "vspace" | "text" | "mbox"
            | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathcal"
            | "mathbb" | "mathfrak" | "operatorname" | "boldsymbol"
        // Structure (also tracked for env depth)
            | "begin" | "end" | "not" | "mod" | "bmod" | "pmod" | "pod"
            | "ldots" | "cdots" | "vdots" | "ddots" | "dots" | "dotsc"
            | "dotsb" | "dotsm" | "dotso"
    )
}

/// If `s` starts with `{name}`, return `name`.
fn braced_env_name(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('{')?;
    let end = inner.find('}')?;
    Some(&inner[..end])
}

/// Fix collapsed matrix row breaks: `\y_1` → `\\y_1` inside row environments.
/// Returns `Some(fixed)` only when the rewrite differs.
fn repair_latex(latex: &str) -> Option<String> {
    let mut out = String::with_capacity(latex.len() + 8);
    let mut changed = false;
    let mut depth: u32 = 0;
    let mut rest = latex;

    while !rest.is_empty() {
        let Some(after_bs) = rest.strip_prefix('\\') else {
            let mut chars = rest.chars();
            out.push(chars.next().unwrap());
            rest = chars.as_str();
            continue;
        };

        // Already a row break (or escaped backslash).
        if let Some(after_row) = after_bs.strip_prefix('\\') {
            out.push_str("\\\\");
            rest = after_row;
            continue;
        }

        let mut chars = after_bs.chars();
        let Some(first) = chars.next() else {
            out.push('\\');
            break;
        };

        // Control symbol: `\,` `\;` `\!` `\{` `\%` etc. — leave alone.
        if !first.is_ascii_alphabetic() {
            out.push('\\');
            out.push(first);
            rest = chars.as_str();
            continue;
        }

        let word_end = after_bs
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(after_bs.len());
        let cmd = &after_bs[..word_end];
        let after_cmd = &after_bs[word_end..];

        if cmd == "begin" || cmd == "end" {
            if let Some(name) = braced_env_name(after_cmd) {
                if is_row_env(name) {
                    if cmd == "begin" {
                        depth = depth.saturating_add(1);
                    } else {
                        depth = depth.saturating_sub(1);
                    }
                }
            }
            out.push('\\');
            out.push_str(cmd);
            rest = after_cmd;
            continue;
        }

        // Inside a matrix/array/cases: unknown control word is almost always a
        // collapsed `\\` before the next cell (`\y_1` → unknown `\y`).
        if depth > 0 && !known_tex_command(cmd) {
            out.push_str("\\\\");
            out.push_str(cmd);
            changed = true;
            rest = after_cmd;
            continue;
        }

        out.push('\\');
        out.push_str(cmd);
        rest = after_cmd;
    }

    changed.then_some(out)
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#x26;", "&")
        .replace("&#38;", "&")
}

/// Rewrite `\(...\)` / `\[...\]` (common LLM forms) to `$…$` / `$$…$$` outside
/// fenced code blocks so pulldown-cmark's math extension can see them.
/// Also strips invisible format chars and normalizes exotic spaces that egui
/// would otherwise paint as tofu boxes.
pub fn normalize_math_delimiters(md: &str) -> String {
    let md = sanitize_unicode(md);
    let mut out = String::with_capacity(md.len());
    let mut rest = md.as_str();
    while let Some(fence_start) = rest.find("```") {
        let (before, after_fence) = rest.split_at(fence_start);
        out.push_str(&rewrite_tex_delimiters(before));
        out.push_str("```");
        let after = &after_fence[3..];
        if let Some(fence_end) = after.find("```") {
            out.push_str(&after[..fence_end]);
            out.push_str("```");
            rest = &after[fence_end + 3..];
        } else {
            out.push_str(after);
            return out;
        }
    }
    out.push_str(&rewrite_tex_delimiters(rest));
    out
}

/// Drop Cf format chars (ZWSP, bidi marks, invisible times, …) and fold exotic
/// Unicode spaces to ASCII space so missing glyphs never become tofu.
fn sanitize_unicode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // Soft hyphen, ZWSP/ZWNJ/ZWJ, LRM/RLM, bidi embeddings/overrides,
            // word joiner, invisible math ops, bidi isolates, BOM.
            '\u{00AD}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}' => {}
            // Variation selectors (e.g. U+FE0F after 🖱️). egui has no glyph and
            // paints them as tofu boxes next to the base emoji.
            '\u{FE00}'..='\u{FE0F}' => {}
            // En/em/thin/hair/figure spaces, NNBSP, medium math space, …
            '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' => out.push(' '),
            // Double-struck holes in the SMP block → BMP letterlike originals.
            '\u{1D53A}' => out.push('ℂ'),
            '\u{1D53F}' => out.push('ℍ'),
            '\u{1D545}' => out.push('ℕ'),
            '\u{1D547}' => out.push('ℙ'),
            '\u{1D548}' => out.push('ℚ'),
            '\u{1D549}' => out.push('ℝ'),
            '\u{1D551}' => out.push('ℤ'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_tex_delimiters_outside_fences() {
        let md = r#"See \(a^2+b^2=c^2\) and
\[
E=mc^2
\]
```
\(keep\)
```
done"#;
        let out = normalize_math_delimiters(md);
        assert!(out.contains("$a^2+b^2=c^2$"));
        assert!(out.contains("$$E=mc^2$$"));
        assert!(out.contains(r"\(keep\)"));
    }

    #[test]
    fn sanitizes_invisible_and_exotic_spaces() {
        let md = "numbers\u{200B}\u{2061} ℂ and\u{2009}ℝ";
        let out = normalize_math_delimiters(md);
        assert!(!out.contains('\u{200B}'));
        assert!(!out.contains('\u{2061}'));
        assert!(out.contains("numbers"));
        assert!(out.contains('ℂ'));
        assert!(out.contains('ℝ'));
        // thin space folded to regular space
        assert!(out.contains("and ℝ") || out.contains("and  ℝ"));
    }

    #[test]
    fn strips_emoji_variation_selectors() {
        // THREE BUTTON MOUSE + VS16 — VS16 must not survive (tofu in egui).
        let out = normalize_math_delimiters("\u{1F5B1}\u{FE0F} Step");
        assert!(out.contains('\u{1F5B1}'));
        assert!(!out.contains('\u{FE0F}'));
        assert!(out.contains("Step"));
    }

    #[test]
    fn maps_math_double_struck_holes() {
        let out = normalize_math_delimiters("\u{1D53A} \u{1D549}");
        assert!(out.contains('ℂ'));
        assert!(out.contains('ℝ'));
        assert!(!out.contains('\u{1D53A}'));
    }

    #[test]
    fn renders_simple_latex() {
        let (img, size) = render_formula("x^2", true, Color32::WHITE, Color32::BLACK).unwrap();
        assert!(img.size[0] > 0 && img.size[1] > 0);
        assert!(size.x > 0.0 && size.y > 0.0);
    }

    #[test]
    fn repair_collapses_matrix_row_breaks() {
        let broken = r"\begin{pmatrix}x_1 & -y_1\y_1 & x_1\end{pmatrix}";
        let fixed = repair_latex(broken).expect("should rewrite");
        assert_eq!(
            fixed,
            r"\begin{pmatrix}x_1 & -y_1\\y_1 & x_1\end{pmatrix}"
        );
    }

    #[test]
    fn repair_preserves_known_commands_in_matrix() {
        let ok = r"\begin{pmatrix}\alpha & \cdot\\\mapsto & \beta\end{pmatrix}";
        assert!(repair_latex(ok).is_none());
    }

    #[test]
    fn repair_ignores_outside_matrix() {
        let outside = r"x\y + z";
        assert!(repair_latex(outside).is_none());
    }

    #[test]
    fn renders_broken_pmatrix_after_repair() {
        let broken = r"\begin{pmatrix}x_1 \ y_1\end{pmatrix}\cdot \begin{pmatrix}x_2 \ y_2\end{pmatrix} \mapsto \begin{pmatrix}x_1 & -y_1\y_1 & x_1 \end{pmatrix}\begin{pmatrix}x_2 \ y_2\end{pmatrix}=\begin{pmatrix} x_1x_2-y_1y_2\x_1y_2+x_2y_1 \end{pmatrix}";
        let (img, size) =
            render_formula(broken, false, Color32::WHITE, Color32::BLACK).unwrap();
        assert!(img.size[0] > 0 && img.size[1] > 0);
        assert!(size.x > 0.0 && size.y > 0.0);
    }

    #[test]
    fn renders_correct_pmatrix_unchanged() {
        let good = r"\begin{pmatrix}x_1 & -y_1 \\ y_1 & x_1\end{pmatrix}";
        assert!(repair_latex(good).is_none());
        let (img, size) = render_formula(good, false, Color32::WHITE, Color32::BLACK).unwrap();
        assert!(img.size[0] > 0 && img.size[1] > 0);
        assert!(size.x > 0.0 && size.y > 0.0);
    }
}

fn rewrite_tex_delimiters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("\\[") {
            if let Some(end) = stripped.find("\\]") {
                out.push_str("$$");
                out.push_str(stripped[..end].trim());
                out.push_str("$$");
                rest = &stripped[end + 2..];
                continue;
            }
        }
        if let Some(stripped) = rest.strip_prefix("\\(") {
            if let Some(end) = stripped.find("\\)") {
                out.push('$');
                out.push_str(stripped[..end].trim());
                out.push('$');
                rest = &stripped[end + 2..];
                continue;
            }
        }
        let mut chars = rest.chars();
        out.push(chars.next().unwrap());
        rest = chars.as_str();
    }
    out
}
