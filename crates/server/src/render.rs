use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use rust_decimal::Decimal;
use typst::{
    Library, LibraryExt, World,
    diag::{FileError, FileResult, Severity, SourceDiagnostic},
    foundations::{Bytes, Datetime, Duration},
    layout::FrameItem,
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot, package::PackageSpec},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_layout::PagedDocument;
use typst_pdf::{PdfOptions, PdfStandard, PdfStandards};

use crate::{
    api::customers::Language,
    documents::{Data, Kind},
};

/// The helper library every layout imports as `/cc.typ`.
pub const LIBRARY: &str = include_str!("../typst/cc.typ");
const MAIN: &str = include_str!("../typst/main.typ");
pub const BUILT_IN: &str = include_str!("../typst/builtin.typ");

static FONTS: LazyLock<Vec<Font>> = LazyLock::new(|| {
    typst_assets::fonts()
        .flat_map(|data| Font::iter(Bytes::new(data)))
        .collect()
});

static LIB: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

/// A layout's Typst source with the files it may read: images under `/assets/`, its fonts, and
/// the approved packages it imports.
#[derive(Clone, Default)]
pub struct Layout {
    /// None for the built-in layout.
    pub name: Option<String>,
    pub source: String,
    pub assets: HashMap<String, Bytes>,
    pub fonts: Vec<Bytes>,
    pub packages: HashMap<PackageSpec, HashMap<String, Bytes>>,
}

impl Layout {
    pub fn built_in() -> Self {
        Self {
            source: BUILT_IN.to_owned(),
            ..Self::default()
        }
    }

    /// Reads `layout.typ` with the files in `assets/` and `fonts/` next to it.
    pub fn from_dir(dir: &std::path::Path) -> std::io::Result<Self> {
        let files = |sub: &str| -> std::io::Result<Vec<(String, Bytes)>> {
            let Ok(entries) = std::fs::read_dir(dir.join(sub)) else {
                return Ok(Vec::new());
            };
            entries
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    std::fs::read(e.path()).map(|bytes| (name, Bytes::new(bytes)))
                })
                .collect()
        };
        Ok(Self {
            source: std::fs::read_to_string(dir.join("layout.typ"))?,
            assets: files("assets")?.into_iter().collect(),
            fonts: files("fonts")?
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect(),
            packages: HashMap::new(),
            name: None,
        })
    }
}

/// An attachment embedded in the PDF, such as the e-invoice XML.
pub struct Attachment {
    pub name: String,
    pub description: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", .0.join("\n"))]
    Typst(Vec<String>),
    #[error("cannot serialize the data: {0}")]
    Data(#[from] serde_json::Error),
}

struct DocWorld {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: FileId,
    files: HashMap<FileId, Bytes>,
    today: Option<Datetime>,
}

fn project(path: &str) -> FileId {
    FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("app paths are valid"),
    ))
}

impl DocWorld {
    fn new(layout: &Layout, data: &Data, attachments: &[Attachment]) -> Result<Self, Error> {
        let mut files = HashMap::new();
        let mut text = |path: &str, text: String| {
            files.insert(project(path), Bytes::from_string(text));
        };
        text("/main.typ", MAIN.to_owned());
        text("/cc.typ", LIBRARY.to_owned());
        text("/layout.typ", layout.source.clone());
        text("/data.json", serde_json::to_string(data)?);
        let attached: Vec<_> = attachments
            .iter()
            .map(|a| serde_json::json!({ "name": a.name, "description": a.description }))
            .collect();
        text("/attachments.json", serde_json::to_string(&attached)?);
        for attachment in attachments {
            files.insert(
                project(&format!("/{}", attachment.name)),
                Bytes::new(attachment.bytes.clone()),
            );
        }
        for (name, bytes) in &layout.assets {
            if let Ok(path) = VirtualPath::new(format!("/assets/{name}")) {
                files.insert(
                    FileId::new(RootedPath::new(VirtualRoot::Project, path)),
                    bytes.clone(),
                );
            }
        }
        for (spec, package) in &layout.packages {
            for (name, bytes) in package {
                if let Ok(path) = VirtualPath::new(name) {
                    files.insert(
                        FileId::new(RootedPath::new(VirtualRoot::Package(spec.clone()), path)),
                        bytes.clone(),
                    );
                }
            }
        }
        let fonts: Vec<Font> = FONTS
            .iter()
            .cloned()
            .chain(
                layout
                    .fonts
                    .iter()
                    .flat_map(|data| Font::iter(data.clone())),
            )
            .collect();
        let today = data
            .issue_date
            .as_deref()
            .and_then(|d| d.parse::<jiff::civil::Date>().ok())
            .and_then(|d| {
                Datetime::from_ymd(
                    d.year().into(),
                    d.month().unsigned_abs(),
                    d.day().unsigned_abs(),
                )
            });
        Ok(Self {
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
            main: project("/main.typ"),
            files,
            today,
        })
    }
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIB
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let bytes = self.file(id)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text.to_owned()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.get().vpath().get_with_slash().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        self.today
    }
}

pub struct Rendered {
    document: PagedDocument,
    created: Datetime,
}

impl Rendered {
    /// All text on every page, without whitespace, for checking what a layout shows.
    pub fn text(&self) -> String {
        fn walk(frame: &typst::layout::Frame, out: &mut String) {
            for (_, item) in frame.items() {
                match item {
                    FrameItem::Group(group) => walk(&group.frame, out),
                    FrameItem::Text(text) => {
                        out.extend(text.text.chars().filter(|c| !c.is_whitespace()))
                    }
                    _ => {}
                }
            }
        }
        let mut out = String::new();
        for page in self.document.pages() {
            walk(&page.frame, &mut out);
        }
        out
    }

    /// PDF/A-3b, the standard e-invoices embed their XML in.
    pub fn pdf(&self) -> Result<Vec<u8>, Error> {
        let options = PdfOptions {
            standards: PdfStandards::new(&[PdfStandard::A_3b])
                .map_err(|e| Error::Typst(vec![e.message().to_string()]))?,
            timestamp: Some(typst_pdf::Timestamp::new_utc(self.created)),
            ..PdfOptions::default()
        };
        typst_pdf::pdf(&self.document, &options).map_err(messages)
    }
}

fn messages(diagnostics: impl IntoIterator<Item = SourceDiagnostic>) -> Error {
    Error::Typst(
        diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| {
                let hints: String = d
                    .hints
                    .iter()
                    .map(|h| format!(" (hint: {})", h.v))
                    .collect();
                format!("{}{hints}", d.message)
            })
            .collect(),
    )
}

/// Lays out `data` with `layout`; CPU-bound, so callers run it on a blocking thread.
pub fn render(layout: &Layout, data: &Data, attachments: &[Attachment]) -> Result<Rendered, Error> {
    let world = DocWorld::new(layout, data, attachments)?;
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(messages)?;
    let now = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC);
    let created = Datetime::from_ymd_hms(
        now.year().into(),
        now.month().unsigned_abs(),
        now.day().unsigned_abs(),
        now.hour().unsigned_abs(),
        now.minute().unsigned_abs(),
        now.second().unsigned_abs(),
    )
    .expect("the clock gives a valid date");
    Ok(Rendered { document, created })
}

/// What a document must show, by the field it comes from: § 14 Abs. 4 UStG for invoices and
/// § 35a GmbHG for every business letter of a UG or GmbH.
pub fn mandatory(data: &Data) -> Vec<(String, Vec<String>)> {
    let mut needles: Vec<(String, Vec<String>)> = Vec::new();
    let mut need = |key: &str, values: Vec<String>| {
        let values: Vec<String> = values
            .into_iter()
            .map(|v| v.chars().filter(|c| !c.is_whitespace()).collect::<String>())
            .filter(|v| !v.is_empty())
            .collect();
        if !values.is_empty() {
            needles.push((key.to_owned(), values));
        }
    };
    let b = &data.business;
    for (key, value) in [
        ("business.name", &b.name),
        ("business.street", &b.street),
        ("business.postcode", &b.postcode),
        ("business.city", &b.city),
        ("business.legal_form", &b.legal_form),
    ] {
        need(key, value.iter().cloned().collect());
    }
    if b.legal_form.is_some() {
        for (key, value) in [
            ("business.register_court", &b.register_court),
            ("business.register_number", &b.register_number),
            ("business.managing_directors", &b.managing_directors),
        ] {
            need(key, value.iter().cloned().collect());
        }
    }
    let c = &data.customer;
    need("customer.name", vec![c.name.clone()]);
    for (key, value) in [
        ("customer.street", &c.street),
        ("customer.postcode", &c.postcode),
        ("customer.city", &c.city),
    ] {
        need(key, value.iter().cloned().collect());
    }
    need("number", data.number.iter().cloned().collect());
    let lang = data.language;
    need(
        "issue_date",
        data.issue_date.iter().map(|d| date(d, lang)).collect(),
    );
    if !matches!(data.kind, Kind::Invoice | Kind::Cancellation) {
        return needles;
    }
    need(
        "business.tax_number",
        b.vat_id.iter().chain(&b.tax_number).cloned().collect(),
    );
    for (i, line) in data.lines.iter().enumerate() {
        let first = line.description.lines().next().unwrap_or_default();
        need(
            &format!("lines.{i}.description"),
            vec![first.chars().take(40).collect()],
        );
        need(
            &format!("lines.{i}.quantity"),
            vec![number(line.quantity, lang)],
        );
    }
    if let Some((from, to)) = data.service_from.as_deref().zip(data.service_to.as_deref()) {
        need("service_period", vec![date(from, lang)]);
        need("service_period", vec![date(to, lang)]);
    }
    if let Some(totals) = &data.totals {
        need("totals.net", vec![amount(totals.net, lang)]);
        need("totals.gross", vec![amount(totals.gross, lang)]);
    }
    if b.small_business {
        need("small_business", vec!["§ 19 UStG".to_owned()]);
    } else {
        for vat in &data.vat {
            need("vat.rate", vec![format!("{}%", number(vat.rate, lang))]);
            need("vat.tax", vec![amount(vat.tax, lang)]);
        }
    }
    needles
}

/// The fields of `data` that the rendered text does not show.
pub fn missing(data: &Data, rendered: &Rendered) -> Vec<String> {
    let text = rendered.text();
    let mut missing: Vec<String> = mandatory(data)
        .into_iter()
        .filter(|(_, values)| !values.iter().any(|v| text.contains(v.as_str())))
        .map(|(key, _)| key)
        .collect();
    missing.dedup();
    missing
}

/// Formats like `cc.amount` in `cc.typ`: two decimals, grouped thousands.
pub fn amount(value: Decimal, lang: Language) -> String {
    let cents = (value * Decimal::ONE_HUNDRED).round();
    let negative = cents.is_sign_negative() && !cents.is_zero();
    let cents = cents.abs().to_string();
    let cents = format!("{cents:0>3}");
    let (whole, fraction) = cents.split_at(cents.len() - 2);
    let (group, point) = separators(lang);
    format!(
        "{}{}{point}{fraction}",
        if negative { "-" } else { "" },
        grouped(whole, group)
    )
}

/// Formats like `cc.number` in `cc.typ`: no trailing zeros, grouped thousands.
pub fn number(value: Decimal, lang: Language) -> String {
    let text = value.normalize().abs().to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let (group, point) = separators(lang);
    let sign = if value.is_sign_negative() && !value.is_zero() {
        "-"
    } else {
        ""
    };
    if fraction.is_empty() {
        format!("{sign}{}", grouped(whole, group))
    } else {
        format!("{sign}{}{point}{fraction}", grouped(whole, group))
    }
}

fn separators(lang: Language) -> (char, char) {
    match lang {
        Language::De => ('.', ','),
        Language::En => (',', '.'),
    }
}

fn grouped(digits: &str, separator: char) -> String {
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// Formats like `cc.date` in `cc.typ`: `31.12.2026` or `31 December 2026`.
pub fn date(iso: &str, lang: Language) -> String {
    let Ok(date) = iso.parse::<jiff::civil::Date>() else {
        return iso.to_owned();
    };
    match lang {
        Language::De => format!("{:02}.{:02}.{}", date.day(), date.month(), date.year()),
        Language::En => {
            const MONTHS: [&str; 12] = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            let month = MONTHS[usize::try_from(date.month() - 1).expect("months are 1 to 12")];
            format!("{} {month} {}", date.day(), date.year())
        }
    }
}

/// Runs `render` on a blocking thread.
pub async fn render_blocking(
    layout: Arc<Layout>,
    data: Data,
    attachments: Vec<Attachment>,
) -> Result<(Rendered, Data), Error> {
    tokio::task::spawn_blocking(move || {
        render(&layout, &data, &attachments).map(|rendered| (rendered, data))
    })
    .await
    .expect("rendering does not panic")
}
