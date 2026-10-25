# Invoice templates

compact-company renders invoices with [Typst](https://typst.app). A template is a directory with an `invoice.typ` entry point; point `invoice.template_dir` at it, or leave it unset to use the bundled `default/` (English and German).

```toml
[invoice]
template_dir = "/etc/compact-company/invoice-template"
font_paths = ["/etc/compact-company/fonts"]   # optional
```

## Invocation

```
typst compile --root <template_dir> [--font-path <p> …] --input data=<json> <template_dir>/invoice.typ <out.pdf>
```

The template reads its data with:

```typst
#let data = json(bytes(sys.inputs.data))
```

Everything about the invoice is in that JSON; a template needs no other source. Files outside `template_dir` can't be read. Fonts come from `font_paths`, the system and Typst's embedded fonts.

A failing compile (a `panic`, a missing font, a syntax error) is shown to the user as Typst reported it, and nothing is issued.

## Contract, version 1

```json
{
  "version": 1,
  "kind": "invoice",
  "draft": false,
  "number": "2026-001",
  "issue_date": "2026-10-27",
  "due_date": "2026-11-10",
  "service_period": { "from": "2026-09-01", "to": "2026-09-30" },
  "language": "de",
  "currency": "EUR",
  "business": {
    "name": "Samuel Wilk",
    "company": "swilk",
    "address": ["Hauptstraße 1", "12345 Berlin"],
    "email": "mail@swilk.example",
    "web": "swilk.example",
    "phone": "030 123",
    "tax_number": "12/345/67890",
    "bank": { "name": "GLS Bank", "iban": "DE02430609671234567890", "bic": "GENODEM1GLS" }
  },
  "tax": { "small_business": false, "vat_rate": 19 },
  "customer": { "number": "K-001", "name": "Genossenschaft Aufwind eG", "address": ["Windweg 3", "54321 Hamburg"] },
  "lines": [
    {
      "description": "Website relaunch",
      "detail": "",
      "quantity": "2.5",
      "unit": "h",
      "unit_price": "85.00",
      "total": "212.50",
      "entries": [
        { "date": "2026-09-02", "user": "Samuel Wilk", "seconds": 9000, "description": "Kickoff and wireframes" }
      ]
    }
  ],
  "totals": { "net": "212.50", "vat": "40.38", "gross": "252.88" },
  "cancels": null,
  "reason": ""
}
```

| Field | Meaning |
|---|---|
| `version` | Contract version; `1`. A template should refuse versions it doesn't know. |
| `kind` | `invoice` or `cancellation`. |
| `draft` | `true` for a preview: show a draft watermark. |
| `number` | Invoice number; `null` while `draft`. |
| `issue_date`, `due_date` | Issue date (today for a preview) and issue date plus the payment days. |
| `service_period` | When the work was done. |
| `language` | The customer's invoice language: `en` or `de`. |
| `currency` | ISO 4217 code of every amount. |
| `business` | The sender. Empty strings for unset optional fields; `name`, `address` and `tax_number` are always set. |
| `tax` | `small_business: true` means no VAT under § 19 UStG and `vat_rate` 0; otherwise `vat_rate` is the integer percent applied. |
| `customer` | The recipient. |
| `lines` | In order. `quantity × unit_price = total`, rounded half up to the currency's minor unit. Cancellation lines carry negated quantities and totals. |
| `lines[].entries` | The time entries billed by a line, oldest first; empty for free lines and cancellations. `seconds` is the tracked duration before rounding. |
| `totals` | `net` is the sum of line totals, `vat` is rounded once on `net`, `gross = net + vat`. |
| `cancels` | For a cancellation invoice the cancelled invoice's `number` and `issue_date`; otherwise `null`. |
| `reason` | Cancellation reason; may be empty. |

- Dates are ISO `YYYY-MM-DD`, in the customer's timezone.
- Money and quantities are decimal strings with `.` as separator and an optional leading `-` (`"1083.75"`, `"-12.75"`, `"1.5"`); amounts carry the currency's minor-unit digits, quantities up to 3 decimals without trailing zeros. Formatting them is the template's job.
- An issued invoice stores exactly this JSON as its snapshot, so its PDF can be rendered again from it.
