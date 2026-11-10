# One number range per document type, assigned when issued

Each document type has its own number range with a format the owner sets, such as `RE-{yyyy}-{nnn}`; a range whose format contains the year starts again at 1 each year. A draft has no number; the next number is taken in the same transaction that issues the document, so a number is never skipped or used twice and a deleted draft leaves no gap. Changing a format applies from the next number on and never renumbers issued documents. Formats are settings, so config may pin them (0017).
