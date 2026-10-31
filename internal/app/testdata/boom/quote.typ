// Fails for a quote with a line described "boom", so tests can make one send fail and the next succeed.
#let data = json(bytes(sys.inputs.data))
#if data.lines.any(l => l.description == "boom") { panic("boom line") }
#data.customer.name
