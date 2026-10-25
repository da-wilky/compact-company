"use strict";

function tickTimers() {
  const now = Date.now() / 1000;
  for (const el of document.querySelectorAll("time.elapsed[data-start]")) {
    const s = Math.max(0, Math.floor(now - Number(el.dataset.start)));
    const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60;
    el.textContent = `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  }
}

function markCurrentNav() {
  for (const a of document.querySelectorAll(".tabs a")) {
    if (location.pathname.startsWith(a.pathname)) a.setAttribute("aria-current", "page");
  }
}

// Go can't enumerate IANA zones, so the list comes from the browser; the server still validates.
function timezoneSelects() {
  if (!Intl.supportedValuesOf) return;
  const zones = Intl.supportedValuesOf("timeZone");
  for (const input of document.querySelectorAll("input[name=timezone]")) {
    const select = document.createElement("select");
    select.name = input.name;
    select.required = input.required;
    if (!input.value) select.add(new Option("", ""));
    for (const zone of zones.includes(input.value) || !input.value ? zones : [input.value, ...zones]) {
      select.add(new Option(zone.replaceAll("_", " "), zone, false, zone === input.value));
    }
    input.replaceWith(select);
  }
}

tickTimers();
setInterval(tickTimers, 1000);
document.addEventListener("htmx:afterSwap", tickTimers);
markCurrentNav();
timezoneSelects();
