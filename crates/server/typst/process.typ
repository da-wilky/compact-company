// The process documentation (GoBD Rz. 150–155); the app hands over every text, already in the
// document's language.
#let data = json("/data.json")

#set document(title: data.title + " " + data.business, author: data.business)
#set text(lang: data.language, size: 10pt)
#set page(
  paper: "a4",
  margin: (x: 2.2cm, y: 2.4cm),
  footer: context [
    #set text(size: 8pt, fill: luma(95))
    #data.business · #data.title · #data.labels.version #data.version
    #h(1fr)
    #counter(page).display("1 / 1", both: true)
  ],
)
#set par(justify: true)
#set heading(numbering: "1.1")
#show heading: set block(above: 1.6em, below: 0.8em)

#align(center)[
  #v(4cm)
  #text(size: 22pt, weight: "bold", data.title)
  #v(0.6em)
  #text(size: 14pt, data.business)
  #v(2em)
  #grid(
    columns: 2,
    column-gutter: 1em,
    row-gutter: 0.6em,
    align: (right, left),
    text(fill: luma(95), data.labels.version), str(data.version),
    text(fill: luma(95), data.labels.valid_from), data.valid_from,
    text(fill: luma(95), data.labels.software), data.software,
  )
]

#v(2em)
#text(size: 9pt, fill: luma(60), data.preface)

#if data.changes.len() > 0 [
  #v(1em)
  #text(weight: "bold", data.labels.changes)
  #list(..data.changes)
]

#pagebreak()
#outline()

#for part in data.parts [
  #pagebreak(weak: true)
  = #part.title
  #part.intro

  #for entry in part.entries [
    == #entry.label
    #if entry.text == none {
      text(fill: luma(95), style: "italic", data.labels.unanswered)
    } else {
      for paragraph in entry.text.split("\n").filter(p => p.trim() != "") [
        #paragraph

      ]
    }
  ]
]
