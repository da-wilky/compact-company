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
  const team = new URLSearchParams(location.search).has("everyone");
  for (const a of document.querySelectorAll(".tabs a")) {
    if (location.pathname.startsWith(a.pathname)) a.setAttribute("aria-current", "page");
    if (team && (a.pathname === "/week" || a.pathname === "/entries")) a.search = "?everyone=1";
  }
}

const iso = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const parseISO = (s) => { const [y, m, d] = s.split("-").map(Number); return new Date(y, m - 1, d); };
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const monday = (d) => addDays(d, -((d.getDay() + 6) % 7));

function isoWeek(d) {
  const thursday = addDays(monday(d), 3);
  const jan4 = new Date(thursday.getFullYear(), 0, 4);
  return 1 + Math.round((monday(thursday) - monday(jan4)) / 6048e5);
}

// Replaces a jump form's native date input with a calendar popover; the native input stays the no-JS fallback.
function datePicker(input) {
  const lang = document.documentElement.lang;
  const strings = document.body.dataset;
  const weekMode = input.name === "week";
  const today = new Date();
  let selected = input.value ? parseISO(input.value) : null;
  let focus = selected || today;

  const trigger = document.createElement("button");
  trigger.type = "button";
  trigger.className = "picker-trigger";
  trigger.setAttribute("aria-haspopup", "dialog");
  trigger.setAttribute("aria-label", input.getAttribute("aria-label"));
  const fmt = new Intl.DateTimeFormat(lang, weekMode ? { day: "numeric", month: "short", year: "numeric" } : { weekday: "short", day: "numeric", month: "short", year: "numeric" });
  trigger.textContent = !selected ? strings.pickDay : weekMode ? fmt.formatRange(monday(selected), addDays(monday(selected), 6)) : fmt.format(selected);

  const pop = document.createElement("div");
  pop.className = "picker";
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", input.getAttribute("aria-label"));
  pop.hidden = true;

  input.type = "hidden";
  input.after(trigger, pop);

  const dayName = new Intl.DateTimeFormat(lang, { weekday: "short" });
  const dayLabel = new Intl.DateTimeFormat(lang, { dateStyle: "full" });
  const monthName = new Intl.DateTimeFormat(lang, { month: "long", year: "numeric" });

  function render() {
    const first = new Date(focus.getFullYear(), focus.getMonth(), 1);
    const start = monday(first);
    let html = `<div class="picker-head"><button type="button" class="picker-step" data-month="-1" aria-label="${strings.prevMonth}">‹</button>
      <span class="picker-title">${monthName.format(first)}</span>
      <button type="button" class="picker-step" data-month="1" aria-label="${strings.nextMonth}">›</button></div>
      <div class="picker-grid${weekMode ? " weeks" : ""}" role="grid"><div class="picker-row" role="row"><span class="picker-wk" role="columnheader">${strings.week}</span>`;
    for (let i = 0; i < 7; i++) html += `<span role="columnheader">${dayName.format(addDays(start, i))}</span>`;
    html += "</div>";
    for (let w = 0; w < 6; w++) {
      const rowStart = addDays(start, w * 7);
      const chosenWeek = weekMode && selected && iso(monday(selected)) === iso(rowStart);
      html += `<div class="picker-row${chosenWeek ? " chosen" : ""}" role="row"><span class="picker-wk">${isoWeek(rowStart)}</span>`;
      for (let i = 0; i < 7; i++) {
        const d = addDays(rowStart, i);
        const classes = [d.getMonth() !== first.getMonth() && "outside", iso(d) === iso(today) && "today", !weekMode && selected && iso(d) === iso(selected) && "chosen"].filter(Boolean).join(" ");
        html += `<button type="button" role="gridcell" class="${classes}" data-day="${iso(d)}" aria-label="${dayLabel.format(d)}" tabindex="${iso(d) === iso(focus) ? 0 : -1}"${classes.includes("chosen") || chosenWeek ? ' aria-selected="true"' : ""}>${d.getDate()}</button>`;
      }
      html += "</div>";
    }
    pop.innerHTML = html + "</div>";
  }

  function open() {
    focus = selected || today;
    render();
    pop.hidden = false;
    trigger.setAttribute("aria-expanded", "true");
    pop.querySelector("[tabindex='0']").focus();
  }

  function close(refocus) {
    pop.hidden = true;
    trigger.setAttribute("aria-expanded", "false");
    if (refocus) trigger.focus();
  }

  function moveFocus(d) {
    focus = d;
    render();
    pop.querySelector("[tabindex='0']").focus();
  }

  trigger.addEventListener("click", () => (pop.hidden ? open() : close(false)));
  pop.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    if (b.dataset.month) moveFocus(new Date(focus.getFullYear(), focus.getMonth() + Number(b.dataset.month), 1));
    if (b.dataset.day) {
      input.value = b.dataset.day;
      input.form.requestSubmit();
    }
  });
  pop.addEventListener("keydown", (e) => {
    const steps = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
    if (e.key === "Escape") close(true);
    else if (!e.target.dataset.day) return;
    else if (steps[e.key]) moveFocus(addDays(focus, steps[e.key]));
    else if (e.key === "PageUp" || e.key === "PageDown") {
      const m = focus.getMonth() + (e.key === "PageUp" ? -1 : 1);
      moveFocus(new Date(focus.getFullYear(), m, Math.min(focus.getDate(), new Date(focus.getFullYear(), m + 1, 0).getDate())));
    }
    else if (e.key === "Home") moveFocus(monday(focus));
    else if (e.key === "End") moveFocus(addDays(monday(focus), 6));
    else return;
    e.preventDefault();
  });
  document.addEventListener("click", (e) => {
    if (!pop.hidden && !pop.contains(e.target) && e.target !== trigger) close(false);
  });
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
document.querySelectorAll(".jump input[type=date]").forEach(datePicker);
timezoneSelects();
