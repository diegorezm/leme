//! Readable text from a web page: a small, lenient HTML reader that keeps
//! headings, paragraphs, list items, and bold/italic/struck-through runs,
//! and drops scripts, menus, forms, and the like. Pure and UI-free.
//!
//! It is not a browser: pages that build their text with JavaScript come
//! out empty, and the caller says so.

/// A run of text in one style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Struck through: revoked text in Brazilian law pages.
    pub strike: bool,
    /// Where it links to: an address, or `#id` within the page.
    pub link: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `h1`–`h6`, as 1–6.
    Heading(u8),
    Paragraph,
    ListItem,
    Quote,
    /// Preformatted: line breaks and spaces kept.
    Code,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: Kind,
    pub runs: Vec<Run>,
    /// Ids (`id`, `<a name>`) in or just before it, for `#id` links.
    pub anchors: Vec<String>,
}

impl Block {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Article {
    pub title: String,
    pub blocks: Vec<Block>,
}

impl Article {
    /// Words of text, to tell a real page from an empty shell.
    pub fn word_count(&self) -> usize {
        self.blocks
            .iter()
            .flat_map(|b| &b.runs)
            .map(|r| r.text.split_whitespace().count())
            .sum()
    }
}

/// Bytes as text, in the charset the page declares (UTF-8 or
/// Windows-1252/Latin-1, which older Brazilian government pages use).
pub fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_lowercase();
    let latin = [
        "charset=iso-8859-1",
        "charset=windows-1252",
        "charset=\"iso-8859-1",
        "charset=\"windows-1252",
        "charset=latin1",
    ]
    .iter()
    .any(|c| head.contains(c));
    match std::str::from_utf8(bytes) {
        Ok(text) if !latin => text.to_owned(),
        _ => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[usize::from(b - 0x80)],
        _ => char::from(b),
    }
}

// ----- Tokens ---------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Text(&'a str),
    /// Lowercase name, raw attributes.
    Start(String, &'a str),
    End(String),
}

/// Elements whose content is never text to read.
const RAW: [&str; 2] = ["script", "style"];

fn tokens(html: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Token::Text(rest));
            break;
        };
        if lt > 0 {
            out.push(Token::Text(&rest[..lt]));
        }
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |i| &after[i + 3..]);
            continue;
        }
        let opens_tag = rest[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?');
        if !opens_tag {
            // A lone "<" in text.
            out.push(Token::Text("<"));
            rest = &rest[1..];
            continue;
        }
        let Some(gt) = rest.find('>') else {
            out.push(Token::Text(rest));
            break;
        };
        let inner = &rest[1..gt];
        rest = &rest[gt + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Token::End(tag_name(name)));
            continue;
        }
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        let name = tag_name(inner);
        let attrs = &inner[inner.len().min(name.len())..];
        if RAW.contains(&name.as_str()) {
            let close = format!("</{name}");
            let end = find_ignore_case(rest, &close).unwrap_or(rest.len());
            rest = &rest[end..];
            rest = rest.find('>').map_or("", |i| &rest[i + 1..]);
            continue;
        }
        out.push(Token::Start(name, attrs));
    }
    out
}

fn tag_name(s: &str) -> String {
    s.trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(&needle.to_ascii_lowercase())
}

// ----- Extraction -----------------------------------------------------------

/// Containers of everything but the reading: menus, forms, and so on.
const SKIP: [&str; 13] = [
    "head", "nav", "header", "footer", "aside", "form", "button", "select", "svg", "iframe",
    "template", "noscript", "textarea",
];
const VOID: [&str; 14] = [
    "br", "img", "hr", "meta", "link", "input", "area", "base", "col", "embed", "param", "source",
    "track", "wbr",
];

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    kind: Option<Kind>,
    runs: Vec<Run>,
    bold: usize,
    italic: usize,
    strike: usize,
    pre: usize,
    /// The open link's href, and how many `<a>` are open.
    link: Option<String>,
    links: usize,
    /// Ids seen since the last block, for the next block with text.
    anchors: Vec<String>,
}

impl Builder {
    fn text(&mut self, raw: &str) {
        let text = entities(raw);
        let text = if self.pre > 0 { text } else { collapse(&text) };
        if text.is_empty() {
            return;
        }
        let (bold, italic, strike) = (self.bold > 0, self.italic > 0, self.strike > 0);
        let link = self.link.clone().filter(|_| self.links > 0);
        match self.runs.last_mut() {
            Some(run)
                if run.bold == bold
                    && run.italic == italic
                    && run.strike == strike
                    && run.link == link =>
            {
                run.text.push_str(&text)
            }
            _ => self.runs.push(Run {
                text,
                bold,
                italic,
                strike,
                link,
            }),
        }
    }

    /// Ends the block being built, if it has any text, and starts `next`.
    fn flush(&mut self, next: Option<Kind>) {
        let runs = std::mem::take(&mut self.runs);
        let kind = self.kind.unwrap_or(Kind::Paragraph);
        self.kind = next;
        let mut runs = tidy(runs, kind == Kind::Code);
        if runs.is_empty() {
            return;
        }
        // A heading-like paragraph that is all bold stays a paragraph; the
        // typesetter shows it bold anyway.
        if let Some(first) = runs.first_mut() {
            first.text = first.text.trim_start().to_owned();
        }
        let anchors = std::mem::take(&mut self.anchors);
        self.blocks.push(Block {
            kind,
            runs,
            anchors,
        });
    }
}

/// Merges spaces across runs and drops empty ones.
fn tidy(runs: Vec<Run>, code: bool) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for mut run in runs {
        if !code {
            let ends_space = out.last().is_none_or(|r: &Run| r.text.ends_with(' '));
            if ends_space {
                run.text = run.text.trim_start().to_owned();
            }
        }
        if !run.text.is_empty() {
            out.push(run);
        }
    }
    if let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
    }
    out.retain(|r| !r.text.is_empty());
    out
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(if c == '\u{a0}' { ' ' } else { c });
            space = false;
        }
    }
    out
}

fn block_kind(name: &str) -> Option<Kind> {
    Some(match name {
        "h1" => Kind::Heading(1),
        "h2" => Kind::Heading(2),
        "h3" => Kind::Heading(3),
        "h4" => Kind::Heading(4),
        "h5" => Kind::Heading(5),
        "h6" => Kind::Heading(6),
        "li" | "dt" | "dd" => Kind::ListItem,
        "blockquote" => Kind::Quote,
        "pre" => Kind::Code,
        "p" | "div" | "section" | "article" | "main" | "tr" | "table" | "ul" | "ol" | "dl"
        | "center" | "figure" | "figcaption" | "caption" | "address" | "body" => Kind::Paragraph,
        _ => return None,
    })
}

/// The readable text of an HTML page.
pub fn extract(html: &str) -> Article {
    let tokens = tokens(html);
    let title = page_title(&tokens);

    // Prefer the page's <main> or <article> when it has one.
    let scope = ["main", "article"].iter().find_map(|tag| {
        let start = tokens
            .iter()
            .position(|t| matches!(t, Token::Start(n, _) if n == tag))?;
        let end = tokens
            .iter()
            .rposition(|t| matches!(t, Token::End(n) if n == tag))
            .filter(|&e| e > start)?;
        Some(start..end + 1)
    });
    let tokens = match scope {
        Some(range) => &tokens[range],
        None => &tokens[..],
    };

    let mut b = Builder::default();
    let mut skipping: Vec<String> = Vec::new();
    for token in tokens {
        match token {
            Token::Start(name, attrs) => {
                let hidden = attrs.to_ascii_lowercase().contains("display:none")
                    || attrs.to_ascii_lowercase().contains("display: none");
                if !skipping.is_empty() || SKIP.contains(&name.as_str()) || hidden {
                    if !VOID.contains(&name.as_str()) {
                        skipping.push(name.clone());
                    }
                    continue;
                }
                if let Some(id) = attr(attrs, "id")
                    .or_else(|| (name == "a").then(|| attr(attrs, "name")).flatten())
                {
                    b.anchors.push(id);
                }
                match name.as_str() {
                    "a" => {
                        if let Some(href) = attr(attrs, "href") {
                            b.link = Some(href);
                            b.links = 1;
                        }
                    }
                    "b" | "strong" => b.bold += 1,
                    "i" | "em" | "cite" => b.italic += 1,
                    "s" | "strike" | "del" => b.strike += 1,
                    "br" => b.flush(b.kind),
                    "td" | "th" => b.text(" "),
                    "pre" => {
                        b.flush(Some(Kind::Code));
                        b.pre += 1;
                    }
                    other => {
                        if let Some(kind) = block_kind(other) {
                            b.flush(Some(kind));
                        }
                    }
                }
            }
            Token::End(name) => {
                if let Some(open) = skipping.last() {
                    if open == name {
                        skipping.pop();
                    } else if !skipping.iter().any(|s| s == name) {
                        // A stray end tag inside a skipped element.
                    } else {
                        while skipping.pop().is_some_and(|s| s != *name) {}
                    }
                    continue;
                }
                match name.as_str() {
                    "a" => b.links = 0,
                    "b" | "strong" => b.bold = b.bold.saturating_sub(1),
                    "i" | "em" | "cite" => b.italic = b.italic.saturating_sub(1),
                    "s" | "strike" | "del" => b.strike = b.strike.saturating_sub(1),
                    "pre" => {
                        b.flush(None);
                        b.pre = b.pre.saturating_sub(1);
                    }
                    other => {
                        if block_kind(other).is_some() {
                            b.flush(None);
                        }
                    }
                }
            }
            Token::Text(text) if skipping.is_empty() => b.text(text),
            Token::Text(_) => {}
        }
    }
    b.flush(None);

    let title = if title.is_empty() {
        b.blocks
            .iter()
            .find(|bl| matches!(bl.kind, Kind::Heading(_)))
            .map(Block::text)
            .unwrap_or_default()
    } else {
        title
    };
    Article {
        title,
        blocks: b.blocks,
    }
}

fn page_title(tokens: &[Token]) -> String {
    let Some(start) = tokens
        .iter()
        .position(|t| matches!(t, Token::Start(n, _) if n == "title"))
    else {
        return String::new();
    };
    let text: String = tokens[start + 1..]
        .iter()
        .take_while(|t| matches!(t, Token::Text(_)))
        .map(|t| match t {
            Token::Text(s) => *s,
            _ => "",
        })
        .collect();
    collapse(&entities(&text)).trim().to_owned()
}

/// The value of attribute `name` in a tag's raw attributes, entities
/// decoded.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name).map(|i| i + from) {
        from = i + name.len();
        let boundary = i == 0 || !matches!(bytes[i - 1], b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_');
        let rest = attrs[from..].trim_start();
        let Some(value) = rest.strip_prefix('=').filter(|_| boundary) else {
            continue;
        };
        let value = value.trim_start();
        let raw = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or_default(),
            _ => value
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or_default(),
        };
        return Some(entities(raw.trim()));
    }
    None
}

/// `href` as an absolute address from the page at `base`, or `#id` when
/// it points within that page. `None` for scripts.
pub fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let lower = href.to_ascii_lowercase();
    if href.is_empty() || lower.starts_with("javascript:") || lower.starts_with("data:") {
        return None;
    }
    if href.starts_with('#') {
        return (href.len() > 1).then(|| href.to_owned());
    }
    if lower.starts_with("mailto:") {
        return Some(href.to_owned());
    }
    let base = base.split('#').next().unwrap_or(base);
    let (scheme, rest) = base.split_once("://")?;
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    let absolute = if lower.starts_with("http://") || lower.starts_with("https://") {
        href.to_owned()
    } else if let Some(path) = href.strip_prefix("//") {
        format!("{scheme}://{path}")
    } else if href.starts_with('/') {
        format!("{scheme}://{host}{href}")
    } else {
        let path = rest[host.len()..].split('?').next().unwrap_or("");
        let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
        let (target, tail) = match href.find(['?', '#']) {
            Some(i) => href.split_at(i),
            None => (href, ""),
        };
        for part in target.split('/') {
            match part {
                ".." => {
                    parts.pop();
                }
                "." | "" => {}
                p => parts.push(p),
            }
        }
        let slash = if target.ends_with('/') { "/" } else { "" };
        format!("{scheme}://{host}/{}{slash}{tail}", parts.join("/"))
    };
    // A link to this same page, at an anchor.
    match absolute.split_once('#') {
        Some((page, id)) if same_page(page, base) && !id.is_empty() => Some(format!("#{id}")),
        _ => Some(absolute),
    }
}

fn same_page(a: &str, b: &str) -> bool {
    let strip = |u: &str| {
        u.trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .to_ascii_lowercase()
    };
    strip(a) == strip(b)
}

// ----- Entities -------------------------------------------------------------

/// Named entities beyond Latin-1 that pages commonly use.
const NAMED: [(&str, char); 20] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("ndash", '–'),
    ("mdash", '—'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("sbquo", '‚'),
    ("bdquo", '„'),
    ("hellip", '…'),
    ("bull", '•'),
    ("middot", '·'),
    ("euro", '€'),
    ("trade", '™'),
    ("shy", '\u{ad}'),
];

/// Latin-1 entity names for U+00A1..=U+00FD, in order.
const LATIN1: [&str; 93] = [
    "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo",
    "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para",
    "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave",
    "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc",
    "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute", "Ocirc",
    "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave",
    "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve",
    "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml",
    "yacute",
];

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        // Numeric references in 0x80..0x9F mean Windows-1252.
        return match code {
            0x80..=0x9f => Some(windows_1252(code as u8)),
            _ => char::from_u32(code),
        };
    }
    if let Some((_, c)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(*c);
    }
    if name == "thorn" {
        return Some('þ');
    }
    if name == "yuml" {
        return Some('ÿ');
    }
    let i = LATIN1.iter().position(|n| *n == name)?;
    char::from_u32(0xa1 + i as u32)
}

fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |i| i + 1);
        match entity(&rest[1..end]) {
            Some(c) => {
                out.push(c);
                rest = &rest[end..];
                rest = rest.strip_prefix(';').unwrap_or(rest);
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace('\u{ad}', "")
}

// ----- Pages ----------------------------------------------------------------

/// Reads a downloaded page: its text, cleaned up for the site it came
/// from. Struck-through text (revoked law) is dropped unless `revoked`.
pub fn read_page(bytes: &[u8], url: &str, revoked: bool) -> Article {
    let mut article = extract(&decode(bytes));
    for run in article.blocks.iter_mut().flat_map(|b| &mut b.runs) {
        run.link = run.link.take().and_then(|href| resolve(url, &href));
    }
    if is_planalto(url) {
        planalto(&mut article);
    }
    if !revoked {
        drop_struck(&mut article);
    }
    article
}

pub fn is_planalto(url: &str) -> bool {
    url.trim_start_matches("https://")
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|host| host == "planalto.gov.br" || host.ends_with(".planalto.gov.br"))
}

/// Drops struck-through runs, and blocks left empty.
pub fn drop_struck(article: &mut Article) {
    for block in &mut article.blocks {
        block.runs.retain(|r| !r.strike);
        let runs = std::mem::take(&mut block.runs);
        block.runs = tidy(runs, block.kind == Kind::Code);
    }
    article.blocks.retain(|b| !b.runs.is_empty());
}

/// Divisions of a Brazilian law, outermost first, as they start a line.
const DIVISIONS: [&str; 6] = ["PARTE", "LIVRO", "TÍTULO", "CAPÍTULO", "SEÇÃO", "SUBSEÇÃO"];

/// The outline level of a division heading ("TÍTULO II", "Seção I",
/// "CAPÍTULO ÚNICO"), 1 for the outermost.
fn division(text: &str) -> Option<u8> {
    let mut words = text.split_whitespace();
    let first = words.next()?.to_uppercase();
    let level = DIVISIONS.iter().position(|d| *d == first)?;
    let number = words
        .next()?
        .trim_end_matches(['.', ':', '-', '–'])
        .to_uppercase();
    let numbered = number.chars().all(|c| "IVXLCDM".contains(c))
        || number.chars().all(|c| c.is_ascii_digit())
        || number == "ÚNICO"
        || number == "ÚNICA"
        || number == "GERAL"
        || number == "ESPECIAL";
    (numbered && text.chars().count() < 120).then(|| level as u8 + 1)
}

/// Names of laws that open a Planalto page, after its letterhead.
const LAW_NAMES: [&str; 9] = [
    "CONSTITUIÇÃO",
    "LEI",
    "DECRETO",
    "CÓDIGO",
    "MEDIDA PROVISÓRIA",
    "EMENDA CONSTITUCIONAL",
    "ATO",
    "CONSOLIDAÇÃO",
    "ESTATUTO",
];

/// Planalto pages: drop the letterhead and the "Vide…" links above the
/// law, name the document after the law, and mark its divisions
/// (Título, Capítulo, Seção) as headings, each with its name.
fn planalto(article: &mut Article) {
    let blocks = std::mem::take(&mut article.blocks);
    let start = blocks.iter().take(40).position(|b| {
        let text = b.text().to_uppercase();
        LAW_NAMES.iter().any(|n| text.starts_with(n)) && text.chars().count() < 200
    });
    let mut blocks = blocks.into_iter();
    if let Some(start) = start {
        let law = blocks.nth(start).map(|b| b.text()).unwrap_or_default();
        article.title = law.clone();
        article.blocks.push(Block {
            kind: Kind::Heading(1),
            runs: vec![Run {
                text: law,
                bold: true,
                ..Run::default()
            }],
            anchors: Vec::new(),
        });
    }
    let mut header = start.is_some();
    let mut pending: Option<(u8, String)> = None;
    let mut pending_anchors: Vec<String> = Vec::new();
    for block in blocks {
        let text = block.text();
        if header {
            // Short lines between the law's name and its text are links:
            // "Vide…", "Texto compilado", "Mensagem de veto", indexes.
            let body = text.chars().count() >= 100
                || text.starts_with("Art.")
                || text.to_uppercase() == "PREÂMBULO"
                || division(&text).is_some();
            if !body {
                continue;
            }
            header = false;
        }
        if let Some((level, name)) = pending.take() {
            // The line after "TÍTULO I" is its name, unless it's text.
            let is_name = text.chars().count() < 120
                && !text.starts_with("Art.")
                && division(&text).is_none()
                && !block.runs.iter().all(|r| r.strike);
            let heading = if is_name {
                format!("{name} – {text}")
            } else {
                name
            };
            let mut anchors = std::mem::take(&mut pending_anchors);
            if is_name {
                anchors.extend(block.anchors.iter().cloned());
            }
            article.blocks.push(heading_block(level, heading, anchors));
            if is_name {
                continue;
            }
        }
        if let Some(level) = division(&text) {
            if block.runs.iter().all(|r| r.strike) {
                article.blocks.push(block);
            } else {
                pending = Some((level, text));
                pending_anchors = block.anchors;
            }
            continue;
        }
        article.blocks.push(block);
    }
    if let Some((level, name)) = pending {
        article
            .blocks
            .push(heading_block(level, name, pending_anchors));
    }
}

/// Division headings sit below the law's title (level 1).
fn heading_block(level: u8, text: String, anchors: Vec<String>) -> Block {
    Block {
        kind: Kind::Heading((level + 1).min(6)),
        runs: vec![Run {
            text,
            bold: true,
            ..Run::default()
        }],
        anchors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_declared_latin1() {
        let page = b"<meta charset=\"iso-8859-1\"><p>Constitui\xe7\xe3o</p>";
        assert!(decode(page).contains("Constituição"));
        assert_eq!(decode("ação".as_bytes()), "ação");
        // Not UTF-8 and nothing declared: Windows-1252.
        assert_eq!(decode(b"\x93a\x94"), "“a”");
    }

    #[test]
    fn entities_resolve() {
        assert_eq!(
            entities("Art. 5&ordm; &sect; 1&#186; &amp; &#x41;"),
            "Art. 5º § 1º & A"
        );
        assert_eq!(entities("a &unknown; b & c"), "a &unknown; b & c");
        assert_eq!(entities("&Ccedil;&atilde;&eacute;"), "Çãé");
    }

    #[test]
    fn keeps_reading_drops_chrome() {
        let page = r#"<html><head><title>Lei   8.112</title><style>p{}</style></head>
            <body><nav><a>Menu</a></nav>
            <script>var x = "<p>no</p>";</script>
            <h1>Título I</h1>
            <p>Art. 1º Esta Lei <b>institui</b> o regime.</p>
            <p><strike>Revogado.</strike> Texto&nbsp;novo.</p>
            <ul><li>um</li><li>dois</li></ul>
            <footer>rodapé</footer></body></html>"#;
        let a = extract(page);
        assert_eq!(a.title, "Lei 8.112");
        let texts: Vec<String> = a.blocks.iter().map(Block::text).collect();
        assert_eq!(
            texts,
            [
                "Título I",
                "Art. 1º Esta Lei institui o regime.",
                "Revogado. Texto novo.",
                "um",
                "dois"
            ]
        );
        assert_eq!(a.blocks[0].kind, Kind::Heading(1));
        assert_eq!(a.blocks[3].kind, Kind::ListItem);
        assert!(a.blocks[1]
            .runs
            .iter()
            .any(|r| r.bold && r.text == "institui"));
        assert!(a.blocks[2].runs[0].strike);
        assert!(!a.blocks[2].runs[1].strike);
    }

    #[test]
    fn prefers_main_content() {
        let page = "<div>sidebar text</div><main><p>the article</p></main><div>more</div>";
        let a = extract(page);
        assert_eq!(a.blocks.len(), 1);
        assert_eq!(a.blocks[0].text(), "the article");
        assert_eq!(a.word_count(), 2);
    }

    #[test]
    fn line_breaks_split_blocks() {
        let a = extract("<p>Art. 1º<br>I - um;<br/>II - dois.</p>");
        let texts: Vec<String> = a.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Art. 1º", "I - um;", "II - dois."]);
    }

    #[test]
    fn survives_broken_markup() {
        let a = extract("<p>a < b <b>bold <i>both</b> tail</p><!-- unclosed");
        assert!(a.blocks[0].text().starts_with("a < b bold both"));
        assert_eq!(extract("<p>x").blocks.len(), 1);
        assert_eq!(extract("").blocks.len(), 0);
    }

    #[test]
    fn divisions() {
        assert_eq!(division("TÍTULO II"), Some(3));
        assert_eq!(division("Seção I"), Some(5));
        assert_eq!(division("CAPÍTULO ÚNICO"), Some(4));
        assert_eq!(division("Título VIII - Da Ordem Social"), Some(3));
        assert_eq!(division("Títulos de crédito são documentos"), None);
        assert_eq!(division("Capítulo seguinte trata"), None);
    }

    #[test]
    fn planalto_pages() {
        let page = "<head><title>L8112</title></head>
            <p>Presidência da República</p><p>Casa Civil</p>
            <p>LEI Nº 8.112, DE 11 DE DEZEMBRO DE 1990</p>
            <p>Mensagem de veto</p><p>Texto compilado</p>
            <p>Dispõe sobre o regime jurídico dos servidores públicos civis da União, das autarquias e das fundações públicas federais.</p>
            <p>TÍTULO I</p><p>Capítulo Único</p><p>Das Disposições Preliminares</p>
            <p>Art. 1º Esta Lei institui o regime.</p>
            <p><strike>Art. 2º Revogado.</strike></p>
            <p>Art. 3º Cargo <strike>antigo</strike> é o conjunto.</p>";
        let hidden = read_page(
            page.as_bytes(),
            "https://www.planalto.gov.br/ccivil_03/leis/l8112cons.htm",
            false,
        );
        assert_eq!(hidden.title, "LEI Nº 8.112, DE 11 DE DEZEMBRO DE 1990");
        let texts: Vec<String> = hidden.blocks.iter().map(Block::text).collect();
        assert_eq!(
            texts,
            [
                "LEI Nº 8.112, DE 11 DE DEZEMBRO DE 1990",
                "Dispõe sobre o regime jurídico dos servidores públicos civis da União, das autarquias e das fundações públicas federais.",
                "TÍTULO I",
                "Capítulo Único – Das Disposições Preliminares",
                "Art. 1º Esta Lei institui o regime.",
                "Art. 3º Cargo é o conjunto.",
            ]
        );
        assert_eq!(hidden.blocks[2].kind, Kind::Heading(4));
        assert_eq!(hidden.blocks[3].kind, Kind::Heading(5));

        let shown = read_page(page.as_bytes(), "https://www.planalto.gov.br/x", true);
        assert!(shown.blocks.iter().any(|b| b.text() == "Art. 2º Revogado."));
        // Other sites keep their letterhead.
        let other = read_page(page.as_bytes(), "https://example.com/x", false);
        assert_eq!(other.blocks[0].text(), "Presidência da República");
    }

    #[test]
    fn attributes() {
        assert_eq!(
            attr(r#" href="a.htm" id='x'"#, "href").as_deref(),
            Some("a.htm")
        );
        assert_eq!(attr(" id='x' NAME=art5", "name").as_deref(), Some("art5"));
        assert_eq!(
            attr(r#" data-href="no" href="yes""#, "href").as_deref(),
            Some("yes")
        );
        assert_eq!(
            attr(r#" href="a?b=1&amp;c=2""#, "href").as_deref(),
            Some("a?b=1&c=2")
        );
        assert_eq!(attr(" class=x", "href"), None);
    }

    #[test]
    fn resolves_links() {
        let base = "https://www.planalto.gov.br/ccivil_03/constituicao/constituicao.htm";
        assert_eq!(resolve(base, "#art5").as_deref(), Some("#art5"));
        assert_eq!(
            resolve(base, "../leis/l8112cons.htm").as_deref(),
            Some("https://www.planalto.gov.br/ccivil_03/leis/l8112cons.htm")
        );
        assert_eq!(
            resolve(base, "/ccivil_03/leis/L9784.htm").as_deref(),
            Some("https://www.planalto.gov.br/ccivil_03/leis/L9784.htm")
        );
        assert_eq!(
            resolve(
                base,
                "http://www.planalto.gov.br/ccivil_03/constituicao/constituicao.htm#art37"
            )
            .as_deref(),
            Some("#art37")
        );
        assert_eq!(
            resolve(base, "//example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(resolve(base, "javascript:void(0)"), None);
        assert_eq!(
            resolve(base, "mailto:a@b.c").as_deref(),
            Some("mailto:a@b.c")
        );
    }

    #[test]
    fn links_and_anchors() {
        let page = r##"<p><a name="art1"></a>Art. 1º Ver <a href="../leis/l1.htm">Lei 1</a> e <a href="#art2">art. 2</a>.</p>
            <p id="art2">Art. 2º Texto.</p>"##;
        let a = read_page(page.as_bytes(), "https://example.com/x/lei.htm", false);
        assert_eq!(a.blocks[0].anchors, ["art1"]);
        assert_eq!(a.blocks[1].anchors, ["art2"]);
        let links: Vec<(&str, Option<&str>)> = a.blocks[0]
            .runs
            .iter()
            .map(|r| (r.text.as_str(), r.link.as_deref()))
            .collect();
        assert_eq!(
            links,
            [
                ("Art. 1º Ver ", None),
                ("Lei 1", Some("https://example.com/leis/l1.htm")),
                (" e ", None),
                ("art. 2", Some("#art2")),
                (".", None),
            ]
        );
    }
}
