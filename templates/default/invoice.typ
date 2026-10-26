// Default compact-company invoice template, contract version 1 (see ../README.md).
#let data = json(bytes(sys.inputs.data))
#assert.eq(data.version, 1, message: "this template supports contract version 1")

#let de = data.language == "de"
#let tr(en, de-text) = if de { de-text } else { en }
#let cancellation = data.kind == "cancellation"

#let date(iso) = {
  let (y, m, d) = iso.split("-").map(int)
  let day = datetime(year: y, month: m, day: d)
  if de { day.display("[day].[month].[year]") } else { day.display("[month repr:long] [day padding:none], [year]") }
}

// A decimal string such as "-1234.5" in the invoice language: "−1.234,5" or "−1,234.5".
#let decimal(s) = {
  let negative = s.starts-with("-")
  let (whole, ..frac) = s.trim("-", at: start).split(".")
  let groups = ()
  let i = calc.rem(whole.len(), 3)
  if i > 0 { groups.push(whole.slice(0, i)) }
  while i < whole.len() {
    groups.push(whole.slice(i, i + 3))
    i += 3
  }
  let out = groups.join(if de { "." } else { "," })
  if frac.len() > 0 { out += (if de { "," } else { "." }) + frac.first() }
  if negative { "−" + out } else { out }
}

#let amount(s) = decimal(s) + "\u{a0}" + data.currency

#let duration(seconds) = {
  let minutes = calc.quo(seconds + 30, 60)
  let m = calc.rem(minutes, 60)
  str(calc.quo(minutes, 60)) + ":" + (if m < 10 { "0" }) + str(m)
}

#let b = data.business
#let sender = (b.company, b.name).filter(s => s != "").first()

#set document(title: if data.number == none { tr("Draft invoice", "Rechnungsentwurf") } else { data.number })
#set text(font: "Libertinus Serif", size: 10.5pt, lang: data.language)
#set page(
  paper: "a4",
  margin: (top: 2.5cm, bottom: 3cm, x: 2.2cm),
  background: if data.draft {
    place(center + horizon, rotate(-45deg, text(96pt, weight: "bold", fill: luma(228), tr("DRAFT", "ENTWURF"))))
  },
  footer: {
    set text(8pt, fill: luma(90))
    line(length: 100%, stroke: 0.4pt + luma(170))
    grid(
      columns: (1fr, 1fr, 1fr),
      gutter: 1em,
      [#sender \ #b.address.join(", ")],
      [#tr("Tax number", "Steuernummer"): #b.tax_number \ #(b.email, b.web, b.phone).filter(s => s != "").join(" · ")],
      if b.bank.iban != "" [#b.bank.name \ IBAN #b.bank.iban #if b.bank.bic != "" [\ BIC #b.bank.bic]],
    )
  },
)

#grid(
  columns: (1fr, auto),
  [
    #v(2.2cm)
    #text(7.5pt, fill: luma(90), underline(((sender,) + b.address).join(" · "))) \
    #v(0.3em)
    #data.customer.name \
    #data.customer.address.join(linebreak())
  ],
  align(right)[
    #text(14pt, weight: "bold", sender) \
    #if b.company != "" and b.name != "" [#b.name \ ]
    #b.address.join(linebreak())
    #v(0.6em)
    #set text(9pt)
    #(b.email, b.web, b.phone).filter(s => s != "").join(linebreak())
  ],
)

#v(1.2cm)
#text(18pt, weight: "bold", if cancellation { tr("Cancellation invoice", "Stornorechnung") } else { tr("Invoice", "Rechnung") })
#v(0.4em)

#{
  let rows = (
    (tr("Number", "Rechnungsnummer"), if data.number == none { tr("(draft)", "(Entwurf)") } else { data.number }),
    (tr("Date", "Rechnungsdatum"), date(data.issue_date)),
    (tr("Customer number", "Kundennummer"), data.customer.number),
    (tr("Service period", "Leistungszeitraum"), date(data.service_period.from) + " – " + date(data.service_period.to)),
  )
  if not cancellation { rows.push((tr("Due date", "Fällig am"), date(data.due_date))) }
  grid(columns: (auto, auto), column-gutter: 1.5em, row-gutter: 0.5em, ..rows.flatten())
}

#if cancellation [
  #v(0.6em)
  #tr(
    [This invoice cancels invoice #data.cancels.number of #date(data.cancels.issue_date).],
    [Diese Rechnung storniert die Rechnung #data.cancels.number vom #date(data.cancels.issue_date).],
  )
  #if data.reason != "" [ #tr("Reason", "Grund"): #data.reason]
]

#v(0.8cm)
#{
  set table(stroke: none, inset: (x: 4pt, y: 5pt))
  table(
    columns: (auto, 1fr, auto, auto, auto, auto),
    align: (left, left, right, left, right, right),
    table.hline(stroke: 0.6pt),
    table.header(
      [*#tr("Pos.", "Pos.")*],
      [*#tr("Description", "Beschreibung")*],
      [*#tr("Qty", "Menge")*],
      [*#tr("Unit", "Einheit")*],
      [*#tr("Unit price", "Einzelpreis")*],
      [*#tr("Total", "Gesamt")*],
    ),
    table.hline(stroke: 0.4pt),
    ..data
      .lines
      .enumerate()
      .map(((i, l)) => (
        str(i + 1),
        [#l.description #if l.detail != "" [\ #text(9pt, fill: luma(90), l.detail)]],
        decimal(l.quantity),
        l.unit,
        amount(l.unit_price),
        amount(l.total),
      ))
      .flatten(),
    table.hline(stroke: 0.4pt),
  )
}

#align(right, grid(
  columns: (auto, auto),
  column-gutter: 2em,
  row-gutter: 0.6em,
  ..if data.tax.small_business {
    ([*#tr("Total", "Gesamtbetrag")*], [*#amount(data.totals.gross)*])
  } else {
    (
      tr("Net", "Nettobetrag"),
      amount(data.totals.net),
      tr("VAT", "Umsatzsteuer") + " " + str(data.tax.vat_rate) + " %",
      amount(data.totals.vat),
      [*#tr("Total", "Gesamtbetrag")*],
      [*#amount(data.totals.gross)*],
    )
  },
))

#v(0.8cm)
#if data.tax.small_business [
  #tr(
    [No VAT is charged: small business under § 19 UStG.],
    [Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.],
  )
]

#if not cancellation [
  #tr(
    [Please transfer #amount(data.totals.gross) by #date(data.due_date)],
    [Bitte überweisen Sie #amount(data.totals.gross) bis zum #date(data.due_date)],
  )#if b.bank.iban != "" [ #tr("to", "auf das Konto") IBAN #b.bank.iban#if b.bank.bic != "" [ (BIC #b.bank.bic)]].
  #if data.number != none [#tr("Reference", "Verwendungszweck"): #data.number]
]

#let entries = data.lines.enumerate().map(((i, l)) => l.entries.map(e => (i + 1, e))).flatten().chunks(2)
#if entries.len() > 0 {
  pagebreak()
  text(14pt, weight: "bold", tr("Time entries", "Zeitnachweis"))
  v(0.4em)
  set table(stroke: none, inset: (x: 4pt, y: 4pt))
  set text(9.5pt)
  table(
    columns: (auto, auto, auto, 1fr, auto),
    align: (left, left, left, left, right),
    table.header(
      [*#tr("Pos.", "Pos.")*],
      [*#tr("Date", "Datum")*],
      [*#tr("Person", "Person")*],
      [*#tr("Description", "Beschreibung")*],
      [*#tr("Hours", "Stunden")*],
    ),
    table.hline(stroke: 0.4pt),
    ..entries.map(((pos, e)) => (str(pos), date(e.date), e.user, e.description, duration(e.seconds))).flatten(),
  )
}
