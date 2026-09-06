use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::Modifier;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextRole {
    Body,
    Muted,
    Title,
    Heading,
    Code,
    Link,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RichSpan {
    pub text: String,
    pub role: TextRole,
    pub modifiers: Modifier,
    pub link: Option<String>,
}

pub(crate) type RichLine = Vec<RichSpan>;

#[derive(Clone)]
struct InlineStyle {
    role: TextRole,
    modifiers: Modifier,
    link: Option<String>,
}

impl InlineStyle {
    fn new(role: TextRole) -> Self {
        Self {
            role,
            modifiers: Modifier::empty(),
            link: None,
        }
    }

    fn append(&self, line: &mut RichLine, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = line.last_mut() {
            if last.role == self.role && last.modifiers == self.modifiers && last.link == self.link
            {
                last.text.push_str(text);
                return;
            }
        }
        line.push(RichSpan {
            text: text.to_owned(),
            role: self.role,
            modifiers: self.modifiers,
            link: self.link.clone(),
        });
    }
}

fn sanitize(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' => clean.push('\n'),
            '\t' => clean.push_str("    "),
            ch if ch.is_control() => {}
            ch => clean.push(ch),
        }
    }
    clean
}

pub(crate) fn plain(text: &str, role: TextRole, width: usize) -> Vec<RichLine> {
    if width == 0 {
        return Vec::new();
    }
    let clean = sanitize(text);
    let style = InlineStyle::new(role);
    let mut rows = Vec::new();
    for text in clean.split('\n') {
        let mut line = Vec::new();
        style.append(&mut line, text);
        rows.extend(wrap(line, "", "", width, true));
    }
    rows
}

struct Prefix {
    first: Option<String>,
    continuation: String,
}

struct Markdown {
    rows: Vec<RichLine>,
    line: RichLine,
    style: InlineStyle,
    styles: Vec<InlineStyle>,
    prefixes: Vec<Prefix>,
    lists: Vec<Option<u64>>,
    code: bool,
    trailing_separator: bool,
    table_cell: usize,
    width: usize,
}

impl Markdown {
    fn flush(&mut self, blank: bool) {
        if self.line.is_empty() && !blank {
            return;
        }
        let mut first = String::new();
        let mut continuation = String::new();
        for prefix in &mut self.prefixes {
            first.push_str(
                prefix
                    .first
                    .take()
                    .as_deref()
                    .unwrap_or(&prefix.continuation),
            );
            continuation.push_str(&prefix.continuation);
        }
        self.rows.extend(wrap(
            std::mem::take(&mut self.line),
            &first,
            &continuation,
            self.width,
            self.code,
        ));
        self.trailing_separator = false;
    }

    fn space(&mut self) {
        self.flush(false);
        if self.rows.last().is_some_and(|line| !line.is_empty()) {
            self.rows.push(Vec::new());
            self.trailing_separator = true;
        }
    }

    fn text(&mut self, text: &str) {
        let clean = sanitize(text);
        for (index, part) in clean.split('\n').enumerate() {
            if index > 0 {
                self.flush(true);
            }
            self.style.append(&mut self.line, part);
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        self.styles.push(self.style.clone());
        match tag {
            Tag::Paragraph => self.flush(false),
            Tag::Heading { level, .. } => {
                self.space();
                self.style.role = if level == HeadingLevel::H1 {
                    TextRole::Title
                } else {
                    TextRole::Heading
                };
                self.style.modifiers |= Modifier::BOLD;
            }
            Tag::BlockQuote(_) => {
                self.space();
                self.prefixes.push(Prefix {
                    first: None,
                    continuation: "│ ".to_owned(),
                });
                self.style.role = TextRole::Muted;
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.space();
                } else {
                    self.flush(false);
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush(false);
                let marker = match self.lists.last_mut() {
                    Some(Some(next)) => {
                        let marker = format!("{next}. ");
                        *next = next.saturating_add(1);
                        marker
                    }
                    _ => "• ".to_owned(),
                };
                self.prefixes.push(Prefix {
                    continuation: " ".repeat(UnicodeWidthStr::width(marker.as_str())),
                    first: Some(marker),
                });
            }
            Tag::CodeBlock(kind) => {
                self.space();
                if let CodeBlockKind::Fenced(language) = kind {
                    if !language.is_empty() {
                        InlineStyle::new(TextRole::Muted)
                            .append(&mut self.line, &sanitize(&language));
                        self.flush(false);
                    }
                }
                self.code = true;
                self.style.role = TextRole::Code;
            }
            Tag::HtmlBlock => self.space(),
            Tag::Emphasis => self.style.modifiers |= Modifier::ITALIC,
            Tag::Strong => self.style.modifiers |= Modifier::BOLD,
            Tag::Strikethrough => self.style.modifiers |= Modifier::CROSSED_OUT,
            Tag::Link { dest_url, .. } => {
                self.style.role = TextRole::Link;
                self.style.modifiers |= Modifier::UNDERLINED;
                self.style.link = Some(sanitize(&dest_url));
            }
            Tag::Image { dest_url, .. } => {
                self.style.role = TextRole::Link;
                self.style.link = Some(sanitize(&dest_url));
                self.style.modifiers |= Modifier::UNDERLINED;
                self.text("[image: ");
            }
            Tag::Table(_) => self.space(),
            Tag::TableHead => {
                self.flush(false);
                self.table_cell = 0;
                self.style.modifiers |= Modifier::BOLD;
            }
            Tag::TableRow => {
                self.flush(false);
                self.table_cell = 0;
            }
            Tag::TableCell => {
                if self.table_cell > 0 {
                    self.text(" │ ");
                }
                self.table_cell += 1;
            }
            Tag::FootnoteDefinition(label) => {
                self.space();
                self.text(&format!("[{label}] "));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.space();
            }
            TagEnd::Heading(_) | TagEnd::HtmlBlock | TagEnd::FootnoteDefinition => self.space(),
            TagEnd::BlockQuote(_) => {
                self.flush(false);
                self.prefixes.pop();
                self.space();
            }
            TagEnd::List(_) => {
                self.flush(false);
                self.lists.pop();
                if self.lists.is_empty() {
                    self.space();
                }
            }
            TagEnd::Item => {
                let empty_item = self
                    .prefixes
                    .last()
                    .is_some_and(|prefix| prefix.first.is_some());
                self.flush(empty_item);
                self.prefixes.pop();
            }
            TagEnd::CodeBlock => {
                self.flush(false);
                self.code = false;
                self.space();
            }
            TagEnd::Image => self.text("]"),
            TagEnd::TableHead | TagEnd::TableRow => self.flush(false),
            TagEnd::Table => self.space(),
            _ => {}
        }
        if let Some(style) = self.styles.pop() {
            self.style = style;
        }
    }
}

pub(crate) fn markdown(text: &str, width: usize) -> Vec<RichLine> {
    if width == 0 {
        return Vec::new();
    }
    let mut output = Markdown {
        rows: Vec::new(),
        line: Vec::new(),
        style: InlineStyle::new(TextRole::Body),
        styles: Vec::new(),
        prefixes: Vec::new(),
        lists: Vec::new(),
        code: false,
        trailing_separator: false,
        table_cell: 0,
        width,
    };
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(tag) => output.start(tag),
            Event::End(tag) => output.end(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => output.text(&text),
            Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                let style = output.style.clone();
                output.style.role = TextRole::Code;
                output.text(&text);
                output.style = style;
            }
            Event::SoftBreak => output.text(" "),
            Event::HardBreak => output.flush(true),
            Event::Rule => {
                output.space();
                output.text(&"─".repeat(width.min(24)));
                output.space();
            }
            Event::TaskListMarker(checked) => output.text(if checked { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(label) => output.text(&format!("[{label}]")),
        }
    }
    output.flush(false);
    if output.trailing_separator {
        output.rows.pop();
    }
    output.rows
}

struct Grapheme<'a> {
    text: &'a str,
    width: usize,
    style: &'a RichSpan,
}

fn append_grapheme(line: &mut RichLine, grapheme: &Grapheme<'_>) {
    if let Some(last) = line.last_mut() {
        if last.role == grapheme.style.role
            && last.modifiers == grapheme.style.modifiers
            && last.link == grapheme.style.link
        {
            last.text.push_str(grapheme.text);
            return;
        }
    }
    line.push(RichSpan {
        text: grapheme.text.to_owned(),
        role: grapheme.style.role,
        modifiers: grapheme.style.modifiers,
        link: grapheme.style.link.clone(),
    });
}

fn prefix_line(prefix: &str, width: usize) -> (RichLine, usize) {
    let mut text = String::new();
    let mut columns = 0;
    for grapheme in prefix.graphemes(true) {
        let next = UnicodeWidthStr::width(grapheme);
        if columns + next > width.saturating_sub(2) {
            break;
        }
        text.push_str(grapheme);
        columns += next;
    }
    let mut line = Vec::new();
    InlineStyle::new(TextRole::Muted).append(&mut line, &text);
    (line, columns)
}

fn wrap(
    spans: RichLine,
    first_prefix: &str,
    continuation: &str,
    width: usize,
    preserve_space: bool,
) -> Vec<RichLine> {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let mut span_index = 0;
    let mut span_end = spans.first().map_or(0, |span| span.text.len());
    let graphemes: Vec<_> = text
        .grapheme_indices(true)
        .map(|(offset, text)| {
            while offset >= span_end {
                span_index += 1;
                span_end += spans[span_index].text.len();
            }
            Grapheme {
                text,
                width: UnicodeWidthStr::width(text),
                style: &spans[span_index],
            }
        })
        .collect();
    let mut rows = Vec::new();
    let (mut line, mut columns) = prefix_line(first_prefix, width);
    let mut prefix_columns = columns;
    let mut index = 0;
    let mut spaces = 0..0;
    while index < graphemes.len() {
        let start = index;
        let whitespace = graphemes[index].text.chars().all(char::is_whitespace);
        while index < graphemes.len()
            && graphemes[index].text.chars().all(char::is_whitespace) == whitespace
        {
            index += 1;
        }
        if whitespace {
            spaces = start..index;
            continue;
        }
        let word_width: usize = graphemes[start..index].iter().map(|g| g.width).sum();
        let space_width: usize = graphemes[spaces.clone()].iter().map(|g| g.width).sum();
        if columns > prefix_columns && columns + space_width + word_width > width {
            rows.push(line);
            (line, columns) = prefix_line(continuation, width);
            prefix_columns = columns;
            spaces = 0..0;
        }
        for grapheme in graphemes[spaces.clone()]
            .iter()
            .chain(&graphemes[start..index])
        {
            if columns + grapheme.width > width && columns > prefix_columns {
                rows.push(line);
                (line, columns) = prefix_line(continuation, width);
                prefix_columns = columns;
            }
            // A two-column grapheme cannot fit a one-column viewport without losing content.
            append_grapheme(&mut line, grapheme);
            columns += grapheme.width;
        }
        spaces = 0..0;
    }
    if preserve_space {
        for grapheme in &graphemes[spaces] {
            if columns + grapheme.width > width && columns > prefix_columns {
                rows.push(line);
                (line, columns) = prefix_line(continuation, width);
                prefix_columns = columns;
            }
            append_grapheme(&mut line, grapheme);
            columns += grapheme.width;
        }
    }
    rows.push(line);
    rows
}
