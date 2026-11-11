#import "/cc.typ"
#import "/layout.typ": schema, render

#let data = cc.load(json("/data.json"))
#assert(
  schema == data.schema,
  message: "the layout is written for data schema " + str(schema) + ", the app provides schema " + str(data.schema),
)

#set document(title: cc.title(data), author: (data.business.name,).filter(n => n != none))
#set text(lang: data.language)

#for file in json("/attachments.json") {
  pdf.attach("/" + file.name, relationship: "alternative", mime-type: "text/xml", description: file.description)
}

#render(data)
