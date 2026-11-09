# Tax returns are computed, not transmitted

The app computes VAT return and EÜR figures, each traceable to its postings, and the owner enters them in Mein ELSTER. Direct transmission needs ERiC, a C library only available for x86_64, while the app must run on arm64 (Raspberry Pi 5); FFI, certificates and yearly ERiC updates would also be the largest single maintenance cost for a few minutes of typing per quarter.
