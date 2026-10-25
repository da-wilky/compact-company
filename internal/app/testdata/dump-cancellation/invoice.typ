// Renders invoices but fails for a cancellation with the received JSON as the message.
#let data = json(bytes(sys.inputs.data))
#if data.kind == "cancellation" { panic(sys.inputs.data) }
#data.customer.name
