# Layout data carries a schema version

The data a layout receives has a schema version, and every layout declares the version it was written for. Adding fields keeps the version; renaming, removing or retyping one raises it. A layout written for another version is refused when uploaded and, after an upgrade, cannot render until it is uploaded again for the current version; built-in layouts always match. Issued documents keep their PDF and the data they were issued with, so they are never rendered again and a schema change cannot alter them.
