# Config may pin business data and defaults

Business data and defaults are edited by the owner in the app, and any key set in config pins its value: the app shows it read-only. A declarative NixOS deployment keeps the business's data and secrets in sops rather than in the database, while an instance without such a config is still set up entirely in the app. Pinning is per key, so a deployment can fix the timezone and leave the rest editable.
