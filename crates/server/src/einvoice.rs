//! EN 16931 invoices in the UN/CEFACT Cross Industry Invoice syntax: embedded in the PDF as
//! ZUGFeRD (Factur-X profile EN 16931), or on their own as XRechnung.

use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use rust_decimal::Decimal;

use crate::{
    api::customers::Language,
    documents::{Data, Deduction, InvoiceType, Kind, Unit},
};

pub const ZUGFERD_NAME: &str = "factur-x.xml";
const EN16931: &str = "urn:cen.eu:en16931:2017";
const XRECHNUNG: &str = "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0";
const PEPPOL_BILLING: &str = "urn:fdc:peppol.eu:2017:poacc:billing:01:1.0";
const SMALL_BUSINESS: &str = "Kleinunternehmer gemäß § 19 UStG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    Zugferd,
    Xrechnung,
}

/// What an invoice still needs to be a valid e-invoice of `flavour`, by field.
pub fn missing(data: &Data, flavour: Flavour) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if flavour == Flavour::Xrechnung {
        let b = &data.business;
        if b.email.is_none() {
            missing.push("business.email");
        }
        if b.phone.is_none() {
            missing.push("business.phone");
        }
        if data.customer.email.is_none() {
            missing.push("customer.email");
        }
    }
    missing
}

struct El {
    name: &'static str,
    attrs: Vec<(&'static str, String)>,
    text: Option<String>,
    children: Vec<El>,
}

fn el(name: &'static str) -> El {
    El {
        name,
        attrs: Vec::new(),
        text: None,
        children: Vec::new(),
    }
}

fn text(name: &'static str, value: impl ToString) -> El {
    El {
        text: Some(value.to_string()),
        ..el(name)
    }
}

impl El {
    fn attr(mut self, name: &'static str, value: &str) -> Self {
        self.attrs.push((name, value.to_owned()));
        self
    }

    fn child(mut self, child: El) -> Self {
        self.children.push(child);
        self
    }

    fn opt(mut self, child: Option<El>) -> Self {
        self.children.extend(child);
        self
    }

    fn all(mut self, children: impl IntoIterator<Item = El>) -> Self {
        self.children.extend(children);
        self
    }

    fn write(&self, writer: &mut Writer<Vec<u8>>) -> std::io::Result<()> {
        let mut start = BytesStart::new(self.name);
        for (name, value) in &self.attrs {
            start.push_attribute((*name, value.as_str()));
        }
        writer.write_event(Event::Start(start))?;
        if let Some(text) = &self.text {
            writer.write_event(Event::Text(BytesText::new(text)))?;
        }
        for child in &self.children {
            child.write(writer)?;
        }
        writer.write_event(Event::End(BytesEnd::new(self.name)))
    }
}

fn date(name: &'static str, iso: &str) -> El {
    el(name).child(text("udt:DateTimeString", iso.replace('-', "")).attr("format", "102"))
}

fn amount(value: Decimal) -> String {
    let value = value.round_dp(2);
    // a reversed zero would print as -0.00
    if value.is_zero() {
        Decimal::new(0, 2)
    } else {
        value
    }
    .to_string()
}

fn unit_code(unit: Unit) -> &'static str {
    match unit {
        Unit::Hour => "HUR",
        Unit::Day => "DAY",
        Unit::Month => "MON",
        Unit::Piece => "C62",
        Unit::LumpSum => "LS",
    }
}

/// The VAT category of a rate: exempt for a small business, zero rated at 0 %, else standard.
fn category(data: &Data, rate: Decimal) -> &'static str {
    if data.business.small_business {
        "E"
    } else if rate.is_zero() {
        "Z"
    } else {
        "S"
    }
}

fn address(
    street: Option<&String>,
    postcode: Option<&String>,
    city: Option<&String>,
    country: &str,
) -> El {
    let mut lines = street
        .map(|s| s.lines().map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    lines.truncate(3);
    let mut address =
        el("ram:PostalTradeAddress").opt(postcode.map(|p| text("ram:PostcodeCode", p)));
    for (name, line) in ["ram:LineOne", "ram:LineTwo", "ram:LineThree"]
        .into_iter()
        .zip(lines)
    {
        address = address.child(text(name, line));
    }
    address
        .opt(city.map(|c| text("ram:CityName", c)))
        .child(text("ram:CountryID", country))
}

/// A deducted partial invoice with its net amount and VAT per rate, as § 14 Abs. 5 UStG asks.
fn deduction_note(data: &Data, d: &Deduction) -> String {
    let currency = &data.currency;
    let de = data.language == Language::De;
    let amounts = d
        .vat
        .iter()
        .map(|g| {
            let rate = g.rate.normalize();
            if de {
                format!(
                    "netto {} {currency} zu {rate} %, USt {} {currency}",
                    amount(g.net),
                    amount(g.tax)
                )
            } else {
                format!(
                    "net {} {currency} at {rate} %, VAT {} {currency}",
                    amount(g.net),
                    amount(g.tax)
                )
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    if de {
        format!(
            "Abzüglich Abschlagsrechnung {} vom {}: {amounts}",
            d.number, d.issue_date
        )
    } else {
        format!(
            "Less partial invoice {} of {}: {amounts}",
            d.number, d.issue_date
        )
    }
}

/// The invoice or cancellation invoice as CII XML. A cancellation invoice is a credit note
/// (381) with its amounts positive; a partial invoice is 326, and a final invoice carries its
/// deductions as the paid amount (BT-113), a note each and its latest as preceding invoice (BG-3).
pub fn cii(data: &Data, flavour: Flavour) -> Vec<u8> {
    let credit = data.kind == Kind::Cancellation;
    let type_code = match (credit, data.invoice_type) {
        (true, _) => "381",
        (false, Some(InvoiceType::Partial)) => "326",
        (false, _) => "380",
    };
    let sign = |value: Decimal| if credit { -value } else { value };
    let b = &data.business;
    let c = &data.customer;
    let currency = data.currency.as_str();
    let xrechnung = flavour == Flavour::Xrechnung;

    let context = el("rsm:ExchangedDocumentContext")
        .opt(xrechnung.then(|| {
            el("ram:BusinessProcessSpecifiedDocumentContextParameter")
                .child(text("ram:ID", PEPPOL_BILLING))
        }))
        .child(
            el("ram:GuidelineSpecifiedDocumentContextParameter")
                .child(text("ram:ID", if xrechnung { XRECHNUNG } else { EN16931 })),
        );

    let document =
        el("rsm:ExchangedDocument")
            .child(text("ram:ID", data.number.as_deref().unwrap_or_default()))
            .child(text("ram:TypeCode", type_code))
            .child(date(
                "ram:IssueDateTime",
                data.issue_date.as_deref().unwrap_or_default(),
            ))
            .opt(
                b.small_business
                    .then(|| el("ram:IncludedNote").child(text("ram:Content", SMALL_BUSINESS))),
            )
            .all(data.deductions.iter().map(|d| {
                el("ram:IncludedNote").child(text("ram:Content", deduction_note(data, d)))
            }));

    let lines = data.lines.iter().map(|line| {
        let rate = line.vat_rate.unwrap_or_default();
        el("ram:IncludedSupplyChainTradeLineItem")
            .child(
                el("ram:AssociatedDocumentLineDocument").child(text("ram:LineID", line.position)),
            )
            .child(el("ram:SpecifiedTradeProduct").child(text("ram:Name", &line.description)))
            .child(el("ram:SpecifiedLineTradeAgreement").child(
                el("ram:NetPriceProductTradePrice").child(text(
                    "ram:ChargeAmount",
                    amount(line.unit_price.unwrap_or_default()),
                )),
            ))
            .child(
                el("ram:SpecifiedLineTradeDelivery").child(
                    text("ram:BilledQuantity", sign(line.quantity).normalize())
                        .attr("unitCode", unit_code(line.unit)),
                ),
            )
            .child(
                el("ram:SpecifiedLineTradeSettlement")
                    .child(
                        el("ram:ApplicableTradeTax")
                            .child(text("ram:TypeCode", "VAT"))
                            .child(text("ram:CategoryCode", category(data, rate)))
                            .child(text("ram:RateApplicablePercent", rate.normalize())),
                    )
                    .child(
                        el("ram:SpecifiedTradeSettlementLineMonetarySummation").child(text(
                            "ram:LineTotalAmount",
                            amount(sign(line.net.unwrap_or_default())),
                        )),
                    ),
            )
    });

    let contact = (b.email.is_some() || b.phone.is_some()).then(|| {
        el("ram:DefinedTradeContact")
            .child(text(
                "ram:PersonName",
                b.managing_directors
                    .as_ref()
                    .or(b.name.as_ref())
                    .cloned()
                    .unwrap_or_default(),
            ))
            .opt(b.phone.as_ref().map(|p| {
                el("ram:TelephoneUniversalCommunication").child(text("ram:CompleteNumber", p))
            }))
            .opt(
                b.email
                    .as_ref()
                    .map(|e| el("ram:EmailURIUniversalCommunication").child(text("ram:URIID", e))),
            )
    });
    let seller = el("ram:SellerTradeParty")
        .child(text("ram:Name", b.name.as_deref().unwrap_or_default()))
        .opt(
            b.register_number
                .as_ref()
                .map(|r| el("ram:SpecifiedLegalOrganization").child(text("ram:ID", r))),
        )
        .opt(contact)
        .child(address(
            b.street.as_ref(),
            b.postcode.as_ref(),
            b.city.as_ref(),
            &b.country,
        ))
        .opt(b.email.as_ref().map(|e| {
            el("ram:URIUniversalCommunication").child(text("ram:URIID", e).attr("schemeID", "EM"))
        }))
        .opt(b.tax_number.as_ref().map(|t| {
            el("ram:SpecifiedTaxRegistration").child(text("ram:ID", t).attr("schemeID", "FC"))
        }))
        .opt(b.vat_id.as_ref().map(|v| {
            el("ram:SpecifiedTaxRegistration").child(text("ram:ID", v).attr("schemeID", "VA"))
        }));
    let buyer = el("ram:BuyerTradeParty")
        .child(text("ram:Name", &c.name))
        .child(address(
            c.street.as_ref(),
            c.postcode.as_ref(),
            c.city.as_ref(),
            &c.country,
        ))
        .opt(c.email.as_ref().map(|e| {
            el("ram:URIUniversalCommunication").child(text("ram:URIID", e).attr("schemeID", "EM"))
        }))
        .opt(c.vat_id.as_ref().map(|v| {
            el("ram:SpecifiedTaxRegistration").child(text("ram:ID", v).attr("schemeID", "VA"))
        }));
    let agreement = el("ram:ApplicableHeaderTradeAgreement")
        .opt(
            c.leitweg_id
                .clone()
                .or_else(|| xrechnung.then(|| c.number.clone()))
                .map(|r| text("ram:BuyerReference", r)),
        )
        .child(seller)
        .child(buyer);

    let payment = match &b.iban {
        Some(iban) => el("ram:SpecifiedTradeSettlementPaymentMeans")
            .child(text("ram:TypeCode", "58"))
            .child(el("ram:PayeePartyCreditorFinancialAccount").child(text("ram:IBANID", iban)))
            .opt(b.bic.as_ref().map(|bic| {
                el("ram:PayeeSpecifiedCreditorFinancialInstitution").child(text("ram:BICID", bic))
            })),
        None => el("ram:SpecifiedTradeSettlementPaymentMeans").child(text("ram:TypeCode", "1")),
    };
    let taxes = data.vat.iter().map(|group| {
        let category = category(data, group.rate);
        el("ram:ApplicableTradeTax")
            .child(text("ram:CalculatedAmount", amount(sign(group.tax))))
            .child(text("ram:TypeCode", "VAT"))
            .opt((category == "E").then(|| text("ram:ExemptionReason", SMALL_BUSINESS)))
            .child(text("ram:BasisAmount", amount(sign(group.net))))
            .child(text("ram:CategoryCode", category))
            .child(text("ram:RateApplicablePercent", group.rate.normalize()))
    });
    let period = data
        .service_from
        .as_ref()
        .zip(data.service_to.as_ref())
        .map(|(from, to)| {
            el("ram:BillingSpecifiedPeriod")
                .child(date("ram:StartDateTime", from))
                .child(date("ram:EndDateTime", to))
        });
    let terms = data
        .due_date
        .as_ref()
        .map(|due| el("ram:SpecifiedTradePaymentTerms").child(date("ram:DueDateDateTime", due)));
    let totals = data.totals.clone().unwrap_or(crate::documents::Totals {
        net: Decimal::ZERO,
        tax: Decimal::ZERO,
        gross: Decimal::ZERO,
        prepaid: None,
        due: None,
    });
    let summation = el("ram:SpecifiedTradeSettlementHeaderMonetarySummation")
        .child(text("ram:LineTotalAmount", amount(sign(totals.net))))
        .child(text("ram:TaxBasisTotalAmount", amount(sign(totals.net))))
        .child(text("ram:TaxTotalAmount", amount(sign(totals.tax))).attr("currencyID", currency))
        .child(text("ram:GrandTotalAmount", amount(sign(totals.gross))))
        .opt(
            totals
                .prepaid
                .map(|p| text("ram:TotalPrepaidAmount", amount(sign(p)))),
        )
        .child(text("ram:DuePayableAmount", amount(sign(totals.payable()))));
    let referenced = |number: &str, issue_date: &str| {
        el("ram:InvoiceReferencedDocument")
            .child(text("ram:IssuerAssignedID", number))
            .child(el("ram:FormattedIssueDateTime").child(
                text("qdt:DateTimeString", issue_date.replace('-', "")).attr("format", "102"),
            ))
    };
    // CII takes one preceding invoice reference; a final invoice names its latest deduction and
    // lists every one in a note
    let preceding = data
        .preceding
        .as_ref()
        .map(|p| referenced(&p.number, &p.issue_date))
        .or_else(|| {
            data.deductions
                .last()
                .map(|d| referenced(&d.number, &d.issue_date))
        });
    let settlement = el("ram:ApplicableHeaderTradeSettlement")
        .child(text("ram:InvoiceCurrencyCode", currency))
        .child(payment)
        .all(taxes)
        .opt(period)
        .opt(terms)
        .child(summation)
        .opt(preceding);

    let invoice = el("rsm:CrossIndustryInvoice")
        .attr(
            "xmlns:rsm",
            "urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100",
        )
        .attr(
            "xmlns:ram",
            "urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100",
        )
        .attr(
            "xmlns:qdt",
            "urn:un:unece:uncefact:data:standard:QualifiedDataType:100",
        )
        .attr(
            "xmlns:udt",
            "urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100",
        )
        .child(context)
        .child(document)
        .child(
            el("rsm:SupplyChainTradeTransaction")
                .all(lines)
                .child(agreement)
                .child(el("ram:ApplicableHeaderTradeDelivery"))
                .child(settlement),
        );

    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .and_then(|()| invoice.write(&mut writer))
        .expect("writing to memory does not fail");
    writer.into_inner()
}

/// The Factur-X extension schema in the PDF's XMP metadata, which ZUGFeRD readers look for.
const XMP: &str = r#"<rdf:Description rdf:about="" xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
<fx:DocumentType>INVOICE</fx:DocumentType>
<fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
<fx:Version>1.0</fx:Version>
<fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>
</rdf:Description>
<rdf:Description rdf:about="" xmlns:pdfaExtension="http://www.aiim.org/pdfa/ns/extension/" xmlns:pdfaSchema="http://www.aiim.org/pdfa/ns/schema#" xmlns:pdfaProperty="http://www.aiim.org/pdfa/ns/property#">
<pdfaExtension:schemas><rdf:Bag><rdf:li rdf:parseType="Resource">
<pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema>
<pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI>
<pdfaSchema:prefix>fx</pdfaSchema:prefix>
<pdfaSchema:property><rdf:Seq>
<rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentFileName</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The name of the embedded XML document</pdfaProperty:description></rdf:li>
<rdf:li rdf:parseType="Resource"><pdfaProperty:name>DocumentType</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The type of the hybrid document</pdfaProperty:description></rdf:li>
<rdf:li rdf:parseType="Resource"><pdfaProperty:name>Version</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The version of the XML schema</pdfaProperty:description></rdf:li>
<rdf:li rdf:parseType="Resource"><pdfaProperty:name>ConformanceLevel</pdfaProperty:name><pdfaProperty:valueType>Text</pdfaProperty:valueType><pdfaProperty:category>external</pdfaProperty:category><pdfaProperty:description>The conformance level of the embedded data</pdfaProperty:description></rdf:li>
</rdf:Seq></pdfaSchema:property>
</rdf:li></rdf:Bag></pdfaExtension:schemas>
</rdf:Description>
"#;

/// Adds the Factur-X metadata to a PDF/A-3 that has `factur-x.xml` attached.
pub fn mark_zugferd(pdf: &[u8]) -> Result<Vec<u8>, lopdf::Error> {
    let mut document = lopdf::Document::load_mem(pdf)?;
    let metadata = document.catalog()?.get(b"Metadata")?.as_reference()?;
    let stream = document.get_object_mut(metadata)?.as_stream_mut()?;
    let xmp = String::from_utf8_lossy(&stream.get_plain_content()?).into_owned();
    let Some(end) = xmp.rfind("</rdf:RDF>") else {
        return Err(lopdf::Error::DictKey("rdf:RDF".to_owned()));
    };
    let marked = format!("{}{XMP}{}", &xmp[..end], &xmp[end..]);
    stream.set_plain_content(marked.into_bytes());
    let mut out = Vec::new();
    document.save_to(&mut out)?;
    Ok(out)
}
