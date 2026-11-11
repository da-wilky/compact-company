use quick_xml::{XmlVersion, events::Event};
use rust_decimal::Decimal;

/// What an incoming e-invoice says, as a receipt needs it; amounts in cents of its currency, a
/// credit note's negative.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub format: Format,
    pub supplier: Party,
    pub number: Option<String>,
    /// ISO dates.
    pub date: Option<String>,
    pub due_date: Option<String>,
    pub currency: String,
    /// One per VAT rate, from the document's tax breakdown so that allowances and charges are in.
    pub groups: Vec<Group>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    utoipa::ToSchema,
    sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Format {
    /// Factur-X / ZUGFeRD: CII inside a PDF.
    Zugferd,
    /// XRechnung or another EN 16931 invoice as CII XML.
    Cii,
    /// XRechnung or another EN 16931 invoice as UBL XML.
    Ubl,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Party {
    pub name: Option<String>,
    pub street: Option<String>,
    pub postcode: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub vat_id: Option<String>,
    pub iban: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Percent.
    pub rate: Decimal,
    pub net: i64,
    pub tax: i64,
    /// The names of the invoice lines at this rate.
    pub description: String,
}

/// The names e-invoices are attached to PDFs under (Factur-X, ZUGFeRD 1 and 2, XRechnung).
const ATTACHMENTS: &[&str] = &[
    "factur-x.xml",
    "zugferd-invoice.xml",
    "ZUGFeRD-invoice.xml",
    "xrechnung.xml",
];

/// Reads an e-invoice from a PDF with an attached invoice or from CII or UBL XML; `None` for
/// anything else, such as a scanned receipt.
pub fn read(bytes: &[u8]) -> Option<Incoming> {
    if bytes.starts_with(b"%PDF") {
        let xml = attachment(bytes)?;
        let mut incoming = read_xml(&xml)?;
        incoming.format = Format::Zugferd;
        Some(incoming)
    } else {
        read_xml(bytes)
    }
}

fn attachment(pdf: &[u8]) -> Option<Vec<u8>> {
    let document = lopdf::Document::load_mem(pdf).ok()?;
    document.objects.values().find_map(|object| {
        let spec = object.as_dict().ok()?;
        let name = [b"UF".as_slice(), b"F"]
            .iter()
            .find_map(|key| spec.get(key).ok()?.as_str().ok())?;
        if !ATTACHMENTS.iter().any(|a| a.as_bytes() == name) {
            return None;
        }
        let files = spec.get_deref(b"EF", &document).ok()?.as_dict().ok()?;
        let stream = files.get_deref(b"F", &document).ok()?.as_stream().ok()?;
        stream
            .decompressed_content()
            .ok()
            .or_else(|| Some(stream.content.clone()))
    })
}

/// An XML element by its local name, without namespaces.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The node at a `/`-separated path of local names.
    fn at(&self, path: &str) -> Option<&Node> {
        path.split('/')
            .try_fold(self, |node, name| node.child(name))
    }

    fn text(&self, path: &str) -> Option<String> {
        self.at(path)
            .map(|n| n.text.trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or_default().to_owned()
}

fn parse(xml: &[u8]) -> Option<Node> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut stack = vec![Node::default()];
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let node = element(&e);
                stack.last_mut()?.children.push(node);
            }
            Event::End(_) => {
                let node = stack.pop()?;
                stack.last_mut()?.children.push(node);
            }
            Event::Text(t) => stack
                .last_mut()?
                .text
                .push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Event::GeneralRef(r) => {
                let resolved = match r.into_inner().as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                };
                stack.last_mut()?.text.push_str(resolved);
            }
            Event::CData(t) => stack.last_mut()?.text.push_str(&t.into_inner()),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let mut root = stack.pop()?;
    (stack.is_empty() && root.children.len() == 1).then(|| root.children.remove(0))
}

fn element(e: &quick_xml::events::BytesStart) -> Node {
    Node {
        name: local(e.name().as_ref()),
        attributes: e
            .attributes()
            .flatten()
            .map(|a| {
                (
                    local(a.key.as_ref()),
                    a.normalized_value(XmlVersion::Implicit1_0)
                        .map(|v| v.into_owned())
                        .unwrap_or_default(),
                )
            })
            .collect(),
        ..Node::default()
    }
}

fn read_xml(xml: &[u8]) -> Option<Incoming> {
    let root = parse(xml)?;
    match root.name.as_str() {
        "CrossIndustryInvoice" => cii(&root),
        "Invoice" | "CreditNote" => ubl(&root),
        _ => None,
    }
}

fn amount(text: Option<String>) -> Option<i64> {
    let value: Decimal = text?.parse().ok()?;
    (value * Decimal::ONE_HUNDRED).round().try_into().ok()
}

/// CII dates come as `YYYYMMDD` (format 102).
fn cii_date(text: Option<String>) -> Option<String> {
    let t = text?;
    (t.len() == 8 && t.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("{}-{}-{}", &t[..4], &t[4..6], &t[6..]))
}

fn names(lines: &[(Decimal, String)], rate: Decimal) -> String {
    let names: Vec<&str> = lines
        .iter()
        .filter(|(r, _)| *r == rate)
        .map(|(_, n)| n.as_str())
        .collect();
    let joined = names.join(", ");
    if joined.chars().count() > 200 {
        joined.chars().take(199).collect::<String>() + "…"
    } else {
        joined
    }
}

fn cii(root: &Node) -> Option<Incoming> {
    let transaction = root.child("SupplyChainTradeTransaction")?;
    let seller = transaction.at("ApplicableHeaderTradeAgreement/SellerTradeParty")?;
    let settlement = transaction.child("ApplicableHeaderTradeSettlement")?;
    let credit = root.text("ExchangedDocument/TypeCode").as_deref() == Some("381");
    let sign = if credit { -1 } else { 1 };
    let lines: Vec<(Decimal, String)> = transaction
        .all("IncludedSupplyChainTradeLineItem")
        .filter_map(|line| {
            let rate = line
                .text("SpecifiedLineTradeSettlement/ApplicableTradeTax/RateApplicablePercent")
                .and_then(|r| r.parse().ok())
                .unwrap_or_default();
            Some((rate, line.text("SpecifiedTradeProduct/Name")?))
        })
        .collect();
    let groups = settlement
        .all("ApplicableTradeTax")
        .filter_map(|tax| {
            let rate: Decimal = tax
                .text("RateApplicablePercent")
                .and_then(|r| r.parse().ok())
                .unwrap_or_default();
            Some(Group {
                rate: rate.normalize(),
                net: sign * amount(tax.text("BasisAmount"))?,
                tax: sign * amount(tax.text("CalculatedAmount")).unwrap_or_default(),
                description: names(&lines, rate),
            })
        })
        .collect();
    let vat_id = seller
        .all("SpecifiedTaxRegistration")
        .find(|r| r.child("ID").and_then(|id| id.attribute("schemeID")) == Some("VA"))
        .and_then(|r| r.text("ID"));
    Some(Incoming {
        format: Format::Cii,
        supplier: Party {
            name: seller.text("Name"),
            street: seller.text("PostalTradeAddress/LineOne"),
            postcode: seller.text("PostalTradeAddress/PostcodeCode"),
            city: seller.text("PostalTradeAddress/CityName"),
            country: seller.text("PostalTradeAddress/CountryID"),
            vat_id,
            iban: settlement.text(
                "SpecifiedTradeSettlementPaymentMeans/PayeePartyCreditorFinancialAccount/IBANID",
            ),
            email: seller.text("URIUniversalCommunication/URIID"),
        },
        number: root.text("ExchangedDocument/ID"),
        date: cii_date(root.text("ExchangedDocument/IssueDateTime/DateTimeString")),
        due_date: cii_date(
            settlement.text("SpecifiedTradePaymentTerms/DueDateDateTime/DateTimeString"),
        ),
        currency: settlement
            .text("InvoiceCurrencyCode")
            .unwrap_or_else(|| "EUR".to_owned()),
        groups,
    })
}

fn ubl(root: &Node) -> Option<Incoming> {
    let credit = root.name == "CreditNote";
    let sign = if credit { -1 } else { 1 };
    let party = root.at("AccountingSupplierParty/Party")?;
    let line_name = if credit {
        "CreditNoteLine"
    } else {
        "InvoiceLine"
    };
    let lines: Vec<(Decimal, String)> = root
        .all(line_name)
        .filter_map(|line| {
            let rate = line
                .text("Item/ClassifiedTaxCategory/Percent")
                .and_then(|r| r.parse().ok())
                .unwrap_or_default();
            Some((rate, line.text("Item/Name")?))
        })
        .collect();
    let groups = root
        .all("TaxTotal")
        .flat_map(|total| total.all("TaxSubtotal"))
        .filter_map(|sub| {
            let rate: Decimal = sub
                .text("TaxCategory/Percent")
                .and_then(|r| r.parse().ok())
                .unwrap_or_default();
            Some(Group {
                rate: rate.normalize(),
                net: sign * amount(sub.text("TaxableAmount"))?,
                tax: sign * amount(sub.text("TaxAmount")).unwrap_or_default(),
                description: names(&lines, rate),
            })
        })
        .collect();
    let vat_id = party
        .all("PartyTaxScheme")
        .find(|s| s.text("TaxScheme/ID").as_deref() == Some("VAT"))
        .and_then(|s| s.text("CompanyID"));
    Some(Incoming {
        format: Format::Ubl,
        supplier: Party {
            name: party
                .text("PartyName/Name")
                .or_else(|| party.text("PartyLegalEntity/RegistrationName")),
            street: party.text("PostalAddress/StreetName"),
            postcode: party.text("PostalAddress/PostalZone"),
            city: party.text("PostalAddress/CityName"),
            country: party.text("PostalAddress/Country/IdentificationCode"),
            vat_id,
            iban: root.text("PaymentMeans/PayeeFinancialAccount/ID"),
            email: party.text("Contact/ElectronicMail"),
        },
        number: root.text("ID"),
        date: root.text("IssueDate"),
        due_date: root
            .text("DueDate")
            .or_else(|| root.text("PaymentMeans/PaymentDueDate")),
        currency: root
            .text("DocumentCurrencyCode")
            .unwrap_or_else(|| "EUR".to_owned()),
        groups,
    })
}
