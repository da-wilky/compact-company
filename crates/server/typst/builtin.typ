// The built-in layout for every kind of document.
#import "/cc.typ"

#let schema = 1

#let muted = luma(95)

#let _info(data) = {
  let rows = ()
  rows.push((cc.t(data, "number"), if data.number != none { data.number } else { cc.t(data, "draft") }))
  if data.issue_date != none { rows.push((cc.t(data, "date"), cc.date(data, data.issue_date))) }
  rows.push((cc.t(data, "customer_number"), data.customer.number))
  if data.service_from != none {
    let from = cc.date(data, data.service_from)
    let to = cc.date(data, data.service_to)
    rows.push((cc.t(data, "service_period"), if from == to { from } else { from + " – " + to }))
  }
  if data.valid_until != none { rows.push((cc.t(data, "valid_until"), cc.date(data, data.valid_until))) }
  if data.due_date != none { rows.push((cc.t(data, "due_date"), cc.date(data, data.due_date))) }
  grid(
    columns: 2,
    column-gutter: 1em,
    row-gutter: 0.45em,
    ..rows.map(((k, v)) => (text(fill: muted, k), v)).flatten(),
  )
}

#let _lines(data) = {
  let priced = data.totals != none
  let head = (cc.t(data, "position"), cc.t(data, "description"), cc.t(data, "quantity"))
  if priced { head += (cc.t(data, "unit_price"), cc.t(data, "amount")) }
  let rows = data.lines.map(line => {
    let row = (
      str(line.position),
      line.description,
      cc.number(data, line.quantity) + " " + cc.unit(data, line.unit),
    )
    if priced {
      row += (
        if line.unit_price != none { cc.money(data, line.unit_price) } else { "–" },
        if line.net != none { cc.money(data, line.net) } else { "–" },
      )
    }
    row
  })
  table(
    columns: if priced { (auto, 1fr, auto, auto, auto) } else { (auto, 1fr, auto) },
    align: (col, _) => if col < 2 { left } else { right },
    stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
    inset: (x: 0.4em, y: 0.55em),
    table.header(..head.map(h => text(weight: "bold", h))),
    ..rows.flatten(),
  )
}

#let _dunning(data) = {
  let d = data.dunning
  table(
    columns: (1fr, auto, auto, auto),
    align: (col, _) => if col == 0 { left } else { right },
    stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
    inset: (x: 0.4em, y: 0.55em),
    table.header(..(cc.t(data, "invoice"), cc.t(data, "invoice_date"), cc.t(data, "due_date"), cc.t(data, "amount_due")).map(h => text(weight: "bold", h))),
    d.invoice_number,
    cc.date(data, d.invoice_date),
    if d.invoice_due_date != none { cc.date(data, d.invoice_due_date) } else { "–" },
    text(weight: "bold", cc.money(data, d.amount_due)),
  )
  if d.pay_until != none {
    v(0.6em)
    par(cc.t(data, "pay_until") + " " + text(weight: "bold", cc.date(data, d.pay_until)) + ".")
  }
}

#let _totals(data) = {
  let rows = ((cc.t(data, "net_total"), cc.money(data, data.totals.net)),)
  if not data.business.small_business {
    for group in data.vat {
      rows.push((cc.t(data, "vat") + " " + cc.percent(data, group.rate), cc.money(data, group.tax)))
    }
  }
  align(right, grid(
    columns: 2,
    column-gutter: 2em,
    row-gutter: 0.55em,
    align: right,
    ..rows.flatten(),
    grid.hline(stroke: 0.6pt),
    text(weight: "bold", cc.t(data, "total")),
    text(weight: "bold", cc.money(data, data.totals.gross)),
  ))
}

#let _timesheet(data) = {
  if data.timesheet == none or data.timesheet.len() == 0 { return }
  pagebreak()
  heading(cc.t(data, "timesheet") + if data.number != none { " – " + data.number })
  v(0.6em)
  table(
    columns: (auto, auto, 1fr, auto),
    align: (col, _) => if col == 3 { right } else { left },
    stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
    inset: (x: 0.4em, y: 0.5em),
    table.header(..(cc.t(data, "date"), cc.t(data, "user"), cc.t(data, "description"), cc.t(data, "hours")).map(h => text(weight: "bold", h))),
    ..data.timesheet.map(e => (
      cc.date(data, e.date),
      e.user,
      e.description,
      cc.number(data, e.hours),
    )).flatten(),
  )
}

#let render(data) = {
  let b = data.business
  set page(
    paper: "a4",
    margin: (top: 2cm, bottom: 3.4cm, left: 2.5cm, right: 2cm),
    footer: context {
      set text(7.5pt, fill: muted)
      line(length: 100%, stroke: 0.3pt + luma(180))
      grid(
        columns: (1fr, auto),
        column-gutter: 1.5em,
        cc.legal(data).join(" · "),
        counter(page).display((n, total) => cc.t(data, "page") + " " + str(n) + " " + cc.t(data, "of") + " " + str(total), both: true),
      )
    },
  )
  set text(font: "Libertinus Serif", size: 10.5pt)
  set par(leading: 0.6em)
  show heading: set text(15pt)

  grid(
    columns: (1fr, auto),
    text(17pt, weight: "bold", b.name),
    align(right, text(8.5pt, fill: muted, (
      cc.address(b).slice(1) + (b.email, b.phone, b.website).filter(x => x != none)
    ).join(linebreak()))),
  )
  v(1.4cm)
  grid(
    columns: (1fr, auto),
    {
      text(7pt, fill: muted, cc.address(b).join(" · "))
      v(0.4em)
      cc.address(data.customer).join(linebreak())
    },
    _info(data),
  )
  v(1.2cm)
  heading(if data.title != none { data.title } else { cc.title(data) })
  if data.title != none { text(fill: muted, cc.title(data)) }
  if data.reference != none { linebreak(); text(fill: muted, data.reference) }
  v(0.6em)
  if data.intro != none { par(data.intro); v(0.4em) }
  if data.kind == "dunning" { _dunning(data) } else { _lines(data) }
  if data.totals != none {
    v(0.4em)
    _totals(data)
  }
  if data.totals != none and cc.vat-note(data) != none {
    v(0.6em)
    cc.vat-note(data)
  }
  if data.pricing == "effort" { v(0.6em); text(fill: muted, cc.t(data, "effort")) }
  if data.outro != none { v(0.8em); par(data.outro) }
  if data.kind == "acceptance_report" {
    v(1.2em)
    par(cc.t(data, "accepted"))
    v(2.4cm)
    grid(
      columns: (1fr, 1fr),
      column-gutter: 2em,
      ..(cc.t(data, "place_date") + ", " + cc.t(data, "customer_signature"), cc.t(data, "place_date") + ", " + cc.t(data, "business_signature"))
        .map(label => { line(length: 100%, stroke: 0.4pt); text(8pt, fill: muted, label) }),
    )
  }
  _timesheet(data)
}
