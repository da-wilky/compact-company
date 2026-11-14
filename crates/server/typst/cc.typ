// compact-company's helper library for layouts, imported as `#import "/cc.typ"`.
// A layout defines `schema` (the data schema it is written for) and `render(data)`.

#let _decimals = ("quantity", "unit_price", "net", "vat_rate", "rate", "tax", "gross", "hours", "amount_due",
  "prepaid", "due")
#let _dates = ("issue_date", "due_date", "valid_until", "service_from", "service_to", "date",
  "invoice_date", "invoice_due_date", "pay_until")

#let _date(value) = datetime(
  year: int(value.slice(0, 4)),
  month: int(value.slice(5, 7)),
  day: int(value.slice(8, 10)),
)

#let _convert(key, value) = {
  if type(value) == dictionary {
    value.pairs().map(((k, v)) => (k, _convert(k, v))).to-dict()
  } else if type(value) == array {
    value.map(v => _convert(key, v))
  } else if value == none {
    none
  } else if key in _decimals {
    decimal(value)
  } else if key in _dates {
    _date(value)
  } else {
    value
  }
}

/// The app's data with amounts as decimals and dates as datetimes.
#let load(raw) = _convert("", raw)

#let _labels = (
  quote: ("Angebot", "Quote"),
  order_confirmation: ("Auftragsbestätigung", "Order confirmation"),
  invoice: ("Rechnung", "Invoice"),
  partial: ("Abschlagsrechnung", "Partial invoice"),
  final: ("Schlussrechnung", "Final invoice"),
  less_partial: ("abzüglich Abschlagsrechnung", "less partial invoice"),
  of_date: ("vom", "of"),
  net: ("netto", "net"),
  amount_to_pay: ("Zahlbetrag", "Amount to pay"),
  cancellation: ("Stornorechnung", "Cancellation invoice"),
  delivery_note: ("Lieferschein", "Delivery note"),
  acceptance_report: ("Abnahmeprotokoll", "Acceptance report"),
  dunning: ("Mahnung", "Payment reminder"),
  dunning_1: ("Zahlungserinnerung", "Payment reminder"),
  dunning_2: ("Mahnung", "Second reminder"),
  dunning_3: ("Letzte Mahnung", "Final reminder"),
  invoice_date: ("Rechnungsdatum", "Invoice date"),
  amount_due: ("Offener Betrag", "Amount due"),
  pay_until: ("Bitte überweisen Sie den offenen Betrag bis zum", "Please transfer the amount due by"),
  timesheet: ("Stundennachweis", "Timesheet"),
  number: ("Nummer", "Number"),
  date: ("Datum", "Date"),
  customer_number: ("Kundennummer", "Customer number"),
  position: ("Pos.", "No."),
  description: ("Beschreibung", "Description"),
  quantity: ("Menge", "Quantity"),
  unit_price: ("Einzelpreis", "Unit price"),
  amount: ("Betrag", "Amount"),
  net_total: ("Summe netto", "Net total"),
  vat: ("Umsatzsteuer", "VAT"),
  total: ("Gesamtbetrag", "Total"),
  service_period: ("Leistungszeitraum", "Service period"),
  due_date: ("Zahlbar bis", "Due by"),
  valid_until: ("Gültig bis", "Valid until"),
  draft: ("Entwurf", "Draft"),
  tax_number: ("Steuernummer", "Tax number"),
  vat_id: ("USt-IdNr.", "VAT ID"),
  register: ("Register", "Register"),
  managing_directors: ("Geschäftsführung", "Managing directors"),
  bank: ("Bankverbindung", "Bank details"),
  page: ("Seite", "Page"),
  of: ("von", "of"),
  place_date: ("Ort, Datum", "Place, date"),
  customer_signature: ("Unterschrift Auftraggeber", "Customer's signature"),
  business_signature: ("Unterschrift Auftragnehmer", "Contractor's signature"),
  accepted: (
    "Die oben aufgeführten Leistungen wurden geprüft und abgenommen.",
    "The work listed above has been inspected and is accepted.",
  ),
  small_business: (
    "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.",
    "No VAT is charged under § 19 UStG (small business).",
  ),
  effort: (
    "Abrechnung nach Aufwand: Die Summe ist eine Schätzung, berechnet wird die tatsächlich geleistete Zeit.",
    "Charged by effort: the total is an estimate; the time actually spent is invoiced.",
  ),
  cancels: ("Storniert Rechnung", "Cancels invoice"),
  confirms: ("Zu Angebot", "For quote"),
  user: ("Mitarbeiter", "Staff"),
  hours: ("Stunden", "Hours"),
  hour: ("Std.", "h"),
  day: ("Tag(e)", "day(s)"),
  month: ("Monat(e)", "month(s)"),
  piece: ("Stk.", "pcs"),
  lump_sum: ("pauschal", "lump sum"),
)

/// A label in the document's language, e.g. `t(data, "invoice")`.
#let t(data, key) = {
  let pair = _labels.at(key, default: (key, key))
  if data.language == "de" { pair.at(0) } else { pair.at(1) }
}

#let _group(digits, separator) = {
  let out = ""
  let n = digits.len()
  for (i, c) in digits.clusters().enumerate() {
    if i > 0 and calc.rem(n - i, 3) == 0 { out += separator }
    out += c
  }
  out
}

#let _separators(data) = if data.language == "de" { (".", ",") } else { (",", ".") }

/// An amount with two decimals and grouped thousands, without currency: `1.234,50`.
#let amount(data, value) = {
  let (group, point) = _separators(data)
  let cents = int(calc.round(value * 100))
  let sign = if cents < 0 { "-" } else { "" }
  let cents = str(calc.abs(cents))
  while cents.len() < 3 { cents = "0" + cents }
  let whole = cents.slice(0, cents.len() - 2)
  sign + _group(whole, group) + point + cents.slice(cents.len() - 2)
}

#let _symbols = (EUR: "€", USD: "$", GBP: "£", CHF: "CHF", DKK: "kr.", SEK: "kr", NOK: "kr", PLN: "zł", CZK: "Kč")

/// An amount with the document's currency: `1.234,50 €` or `€1,234.50`.
#let money(data, value) = {
  let symbol = _symbols.at(data.currency, default: data.currency)
  if data.language == "de" or symbol.len() > 2 {
    amount(data, value) + sym.space.nobreak + symbol
  } else {
    symbol + amount(data, value)
  }
}

/// A quantity or rate without trailing zeros: `2,5`.
#let number(data, value) = {
  let (group, point) = _separators(data)
  let text = str(calc.abs(value))
  let (whole, fraction) = if text.contains(".") { text.split(".") } else { (text, "") }
  fraction = fraction.trim("0", at: end)
  let sign = if value < 0 { "-" } else { "" }
  sign + _group(whole, group) + if fraction == "" { "" } else { point + fraction }
}

#let percent(data, value) = number(data, value) + sym.space.nobreak + "%"

#let _months = ("January", "February", "March", "April", "May", "June", "July", "August",
  "September", "October", "November", "December")

/// A date: `31.12.2026` or `31 December 2026`.
#let date(data, value) = {
  if value == none { return none }
  if data.language == "de" {
    value.display("[day].[month].[year]")
  } else {
    str(value.day()) + " " + _months.at(value.month() - 1) + " " + str(value.year())
  }
}

#let unit(data, value) = t(data, value)

/// The document's title: its kind, or the draft marker.
#let title(data) = {
  let kind = if data.kind == "dunning" and data.dunning != none {
    t(data, "dunning_" + str(data.dunning.level))
  } else if data.at("invoice_type", default: none) != none {
    t(data, data.invoice_type)
  } else { t(data, data.kind) }
  if data.number == none { kind + " (" + t(data, "draft") + ")" } else { kind + " " + data.number }
}

/// The business's address block, one item per line.
#let address(party) = (
  (party.name,) + (party.street, ) + (
    (party.postcode, party.city).filter(p => p != none).join(" "),
  ) + if party.country != "DE" { (party.country,) } else { () }
).filter(line => line != none and line != "")

/// The § 19 UStG note for a small business, or none.
#let vat-note(data) = if data.business.small_business { t(data, "small_business") }

/// Legal details for the footer: tax and register numbers, managing directors, bank.
#let legal(data) = {
  let b = data.business
  let lines = ()
  let name = if b.legal_form != none and not b.name.contains(b.legal_form) {
    b.name + " " + b.legal_form
  } else { b.name }
  lines.push(name)
  if b.register_court != none or b.register_number != none {
    lines.push(t(data, "register") + ": " + (b.register_court, b.register_number).filter(x => x != none).join(" "))
  }
  if b.managing_directors != none {
    lines.push(t(data, "managing_directors") + ": " + b.managing_directors)
  }
  if b.vat_id != none { lines.push(t(data, "vat_id") + " " + b.vat_id) }
  if b.tax_number != none { lines.push(t(data, "tax_number") + " " + b.tax_number) }
  if b.iban != none {
    lines.push(
      (b.bank_name, "IBAN " + b.iban, if b.bic != none { "BIC " + b.bic }).filter(x => x != none).join(" · "),
    )
  }
  lines
}
