// Default compact-company quote template, contract version 1 (see ../README.md).
#let data = json(bytes(sys.inputs.data))
#assert.eq(data.version, 1, message: "this template supports contract version 1")

#let de = data.language == "de"
#let tr(en, de-text) = if de { de-text } else { en }
#let effort = data.pricing == "effort"

#let date(iso) = {
  let (y, m, d) = iso.split("-").map(int)
  let day = datetime(year: y, month: m, day: d)
  if de { day.display("[day].[month].[year]") } else { day.display("[month repr:long] [day padding:none], [year]") }
}

// A decimal string such as "-1234.5" in the quote language: "−1.234,5" or "−1,234.5".
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

#let per = (
  once: none,
  monthly: tr("per month", "pro Monat"),
  yearly: tr("per year", "pro Jahr"),
)
#let interval-total = (
  once: tr("One-off", "Einmalig"),
  monthly: tr("Per month", "Monatlich"),
  yearly: tr("Per year", "Jährlich"),
)

#let b = data.business
#let sender = (b.company, b.name).filter(s => s != "").first()

#set document(title: if data.number == none { tr("Draft quote", "Angebotsentwurf") } else { data.number })
#set text(font: "Libertinus Serif", size: 10.5pt, lang: data.language)
#set par(justify: false)
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
#text(18pt, weight: "bold", tr("Quote", "Angebot"))
#v(0.4em)

#{
  let rows = (
    (tr("Number", "Angebotsnummer"), if data.number == none { tr("(draft)", "(Entwurf)") } else { data.number }),
    (tr("Date", "Angebotsdatum"), date(data.issue_date)),
    (tr("Customer number", "Kundennummer"), data.customer.number),
    (tr("Valid until", "Gültig bis"), date(data.valid_until)),
  )
  if data.project != none { rows.push((tr("Project", "Projekt"), data.project)) }
  grid(columns: (auto, auto), column-gutter: 1.5em, row-gutter: 0.5em, ..rows.flatten())
}

#v(0.8cm)
#text(12pt, weight: "bold", data.subject)
#v(0.4em)

#tr([Dear Sir or Madam,], [Sehr geehrte Damen und Herren,])

#if effort {
  tr(
    [Thank you for your interest. We offer the following work, invoiced by effort for the time actually spent.],
    [vielen Dank für Ihr Interesse. Wir bieten Ihnen die folgenden Leistungen an, abgerechnet nach Aufwand für die tatsächlich aufgewendete Zeit.],
  )
} else {
  tr(
    [Thank you for your interest. We offer the following work at a fixed price.],
    [vielen Dank für Ihr Interesse. Wir bieten Ihnen die folgenden Leistungen zum Festpreis an.],
  )
}

#if data.lines.len() > 0 {
  v(0.6cm)
  set table(stroke: none, inset: (x: 4pt, y: 5pt))
  let rows = ()
  for (i, l) in data.lines.enumerate() {
    rows += (
      str(i + 1),
      [#l.description #if l.detail != "" [\ #text(9pt, fill: luma(90), l.detail)]],
      decimal(l.quantity),
      l.unit,
      amount(l.unit_price),
      [#amount(l.total) #if per.at(l.interval) != none [\ #text(9pt, fill: luma(90), per.at(l.interval))]],
    )
  }
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
    ..rows,
    table.hline(stroke: 0.4pt),
  )

  let gross-label = if effort { tr("Estimated total", "Voraussichtlich gesamt") } else { tr("Total", "Gesamtbetrag") }
  let header = if data.tax.small_business { ([], [*#gross-label*]) } else {
    ([], [*#tr("Net", "Netto")*], [*#tr("VAT", "USt.") #data.tax.vat_rate %*], [*#gross-label*])
  }
  let rows = ()
  for interval in ("once", "monthly", "yearly") {
    let t = data.totals.at(interval)
    if t == none { continue }
    rows.push(interval-total.at(interval))
    if not data.tax.small_business { rows += (amount(t.net), amount(t.vat)) }
    rows.push(strong(amount(t.gross)))
  }
  align(right, table(
    columns: header.len(),
    align: (left,) + (right,) * (header.len() - 1),
    column-gutter: 1.5em,
    ..header,
    table.hline(stroke: 0.4pt),
    ..rows,
  ))
  if data.tax.small_business {
    tr(
      [No VAT is charged: small business under § 19 UStG.],
      [Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.],
    )
  }
}

#for s in data.sections {
  v(0.5cm)
  if s.heading != "" {
    block(below: 0.6em, text(11.5pt, weight: "bold", s.heading))
  }
  eval(s.body, mode: "markup")
}

#v(0.6cm)
#tr(
  [This quote is valid until #date(data.valid_until).],
  [Dieses Angebot ist gültig bis zum #date(data.valid_until).],
)

#v(0.4cm)
#tr([Kind regards], [Mit freundlichen Grüßen]) \
#v(0.6em)
#b.name

#v(0.8cm)
#block(breakable: false, width: 100%)[
  #text(12pt, weight: "bold", tr("Acceptance", "Auftragserteilung"))
  #v(0.2em)
  #if effort {
    tr(
      [We commission the work described above, invoiced by effort, under the terms of this quote.],
      [Hiermit beauftragen wir die oben beschriebenen Leistungen zur Abrechnung nach Aufwand zu den Bedingungen dieses Angebots.],
    )
  } else {
    tr(
      [We accept this quote and commission the work described above at the fixed price stated.],
      [Hiermit nehmen wir dieses Angebot an und beauftragen die oben beschriebenen Leistungen zum genannten Festpreis.],
    )
  }
  #v(1.2cm)
  #grid(
    columns: (1fr, 1fr),
    column-gutter: 2cm,
    row-gutter: 0.4em,
    line(length: 100%, stroke: 0.4pt), line(length: 100%, stroke: 0.4pt),
    text(9pt, tr("Place, date", "Ort, Datum")), text(9pt, [#tr("Signature", "Unterschrift"), #data.customer.name]),
  )
]
