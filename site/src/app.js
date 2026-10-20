// The site's small bits of life. No framework: a handful of listeners, and every animation
// stops when the visitor prefers less motion. Each page has one language; the build made it so.
(() => {
  const root = document.documentElement;
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lang = () => root.dataset.lang;
  const t = (zh, en) => (lang() === "zh" ? zh : en);
  const $$ = (sel) => [...document.querySelectorAll(sel)];

  // Segmented controls share one look: the chosen one is lifted.
  function choose(buttons, on) {
    buttons.forEach((b) => {
      const yes = on(b);
      b.classList.toggle("bg-fg", yes);
      b.classList.toggle("text-bg", yes);
      b.classList.toggle("text-fg/55", !yes);
      b.setAttribute("aria-selected", yes);
    });
  }

  // Swap something on the page without a jolt: cross-fade where the browser
  // can, and keep whatever sits mid-screen exactly where it was.
  function smoothly(change, opts = {}) {
    const anchor = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    const before = anchor ? anchor.getBoundingClientRect().top : 0;
    const apply = () => {
      change();
      if (anchor && anchor.isConnected) scrollBy({ top: anchor.getBoundingClientRect().top - before, behavior: "instant" });
    };
    if (calm || !document.startViewTransition) return apply();
    if (opts.skin) root.classList.add("skin-change");
    const vt = document.startViewTransition(apply);
    vt.finished.finally(() => root.classList.remove("skin-change"));
  }

  // Skin: follows the system until the visitor picks one with the toggle.
  const system = matchMedia("(prefers-color-scheme: light)");
  const themeColor = document.querySelector('meta[name="theme-color"]');
  // Every screenshot exists per language and per skin: name.zh.light.webp.
  // data-skin pins one ("dark" inside the always-dark mockups, "none" when it has no skins).
  function picture(img, l, t) {
    const skin = img.dataset.skin || t;
    return "/assets/" + img.dataset.pic + "." + l + (skin === "none" ? "" : "." + skin) + ".webp";
  }
  function refreshPictures() {
    $$("[data-pic]").forEach((img) => {
      const src = picture(img, lang(), root.dataset.theme);
      if (img.getAttribute("src") !== src) img.src = src;
    });
  }
  function paint(theme) {
    root.dataset.theme = theme;
    if (themeColor) themeColor.content = theme === "light" ? "#f3efe9" : "#0c0b0a";
    refreshPictures();
  }
  system.addEventListener("change", () => {
    let saved = null;
    try { saved = localStorage.getItem("theme"); } catch (e) {}
    if (!saved) smoothly(() => paint(system.matches ? "light" : "dark"));
  });
  const themeBtn = document.getElementById("theme");
  themeBtn.addEventListener("click", () => {
    const next = root.dataset.theme === "light" ? "dark" : "light";
    // Picking what the system already says means "follow the system" again.
    try {
      if (next === (system.matches ? "light" : "dark")) localStorage.removeItem("theme");
      else localStorage.setItem("theme", next);
    } catch (e) {}
    const r = themeBtn.getBoundingClientRect();
    root.style.setProperty("--vx", r.left + r.width / 2 + "px");
    root.style.setProperty("--vy", r.top + r.height / 2 + "px");
    smoothly(() => paint(next), { skin: true });
  });
  paint(root.dataset.theme);

  // Language: each lives at its own URL. Switching keeps your place: the section in view and
  // how far into it you were travel with you, and the page cross-fades instead of blinking.
  const PLACE = "cc-moss:place";
  $$("[data-set-lang]").forEach((a) => a.addEventListener("click", (e) => {
    if (a.dataset.setLang === lang()) { e.preventDefault(); return; }
    try {
      localStorage.setItem("lang", a.dataset.setLang);
      const at = document.elementFromPoint(innerWidth / 2, innerHeight / 3);
      const section = at && at.closest("[id]");
      if (section) sessionStorage.setItem(PLACE, JSON.stringify({ id: section.id, top: section.getBoundingClientRect().top }));
    } catch (err) {}
  }));
  try {
    const place = JSON.parse(sessionStorage.getItem(PLACE) || "null");
    sessionStorage.removeItem(PLACE);
    const section = place && document.getElementById(place.id);
    if (section && !location.hash) scrollTo({ top: section.getBoundingClientRect().top + scrollY - place.top, behavior: "instant" });
  } catch (err) {}
  choose($$(".lang-btn"), (b) => b.dataset.setLang === lang());
  // Fetch the other skin's pictures once the page is idle, so switching never shows a blank.
  addEventListener("load", () => setTimeout(() => {
    $$("[data-pic]").forEach((img) => {
      for (const t of ["light", "dark"]) new Image().src = picture(img, lang(), t);
    });
  }, 1500));

  // Copy buttons.
  $$("[data-copy]").forEach((b) => {
    b.addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(b.dataset.copy); } catch (e) { return; }
      const label = b.querySelector(".copy-label");
      const before = label.innerHTML;
      label.innerHTML = '<span class="text-coral">' + t("已复制", "Copied") + "</span>";
      setTimeout(() => { label.innerHTML = before; }, 1600);
    });
  });

  // Rise into view.
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.12 });
  $$(".reveal").forEach((el) => io.observe(el));

  // Nav: the section you're in is marked.
  const navLinks = $$(".nav-link");
  const navIO = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      navLinks.forEach((a) => {
        const on = a.dataset.nav === e.target.id;
        a.classList.toggle("text-fg", on);
        a.classList.toggle("bg-fg/[.06]", on);
      });
    });
  }, { rootMargin: "-45% 0px -50% 0px" });
  ["top", "tour", "moss", "mo", "terminal", "compare", "faq", "install"].forEach((id) => { const el = document.getElementById(id); if (el) navIO.observe(el); });

  // Changelog: the version you're reading is marked in the jump list.
  const jumps = $$(".cl-jump");
  if (jumps.length) {
    const jumpIO = new IntersectionObserver((entries) => {
      entries.forEach((e) => {
        if (!e.isIntersecting) return;
        const id = e.target.querySelector("h2[id]").id;
        jumps.forEach((a) => a.setAttribute("aria-current", a.dataset.v === id ? "true" : "false"));
        const on = jumps.find((a) => a.dataset.v === id);
        const strip = on && on.closest(".cl-index");
        // On phones the list is a strip of chips: keep the current one in view.
        if (strip && strip.scrollWidth > strip.clientWidth) strip.scrollTo({ left: on.offsetLeft - 20, behavior: calm ? "auto" : "smooth" });
      });
    }, { rootMargin: "-35% 0px -60% 0px" });
    $$(".release").forEach((li) => jumpIO.observe(li));
  }

  // The tour: scrolling picks the step; tabs and arrows scroll to one.
  const steps = $$(".step");
  if (steps.length) {
  const screens = $$(".screen");
  const tabs = $$(".tour-tab");
  const prev = document.querySelector(".tour-prev");
  const next = document.querySelector(".tour-next");
  let current = 0;
  function showStep(i) {
    current = i;
    const id = steps[i].dataset.step;
    steps.forEach((s, j) => s.classList.toggle("on", j === i));
    screens.forEach((s) => s.classList.toggle("on", s.dataset.screen === id));
    choose(tabs, (b) => b.dataset.go === id);
    // Only a strip that overflows (phones) needs to bring its tab into view; a
    // needless smooth scroll here would cancel the page's own (e.g. a #link jump).
    const tab = tabs.find((b) => b.dataset.go === id);
    const strip = tab && tab.parentElement;
    if (strip && strip.scrollWidth > strip.clientWidth) strip.scrollTo({ left: tab.offsetLeft - 40, behavior: calm ? "auto" : "smooth" });
    prev.disabled = i === 0;
    next.disabled = i === steps.length - 1;
  }
  function goTo(i) {
    const step = steps[Math.max(0, Math.min(steps.length - 1, i))];
    // Land the step where the observer reads it: mid-screen on desktop, below the stage on phones.
    const wide = matchMedia("(min-width: 1024px)").matches;
    const box = step.getBoundingClientRect();
    const target = wide ? innerHeight / 2 : innerHeight * 0.78;
    scrollBy({ top: box.top + box.height / 2 - target, behavior: calm ? "auto" : "smooth" });
  }
  tabs.forEach((b) => b.addEventListener("click", () => goTo(steps.findIndex((s) => s.dataset.step === b.dataset.go))));
  prev.addEventListener("click", () => goTo(current - 1));
  next.addEventListener("click", () => goTo(current + 1));
  let stepIO = null;
  function watchSteps() {
    if (stepIO) stepIO.disconnect();
    const wide = matchMedia("(min-width: 1024px)").matches;
    stepIO = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) showStep(steps.indexOf(e.target)); });
    }, { rootMargin: wide ? "-49% 0px -49% 0px" : "-77% 0px -21% 0px" });
    steps.forEach((s) => stepIO.observe(s));
  }
  matchMedia("(min-width: 1024px)").addEventListener("change", watchSteps);
  watchSteps();
  showStep(0);
  }

  // Keycaps: ⌃ then ⌃, and the panel answers.
  const caps = $$("#keys .keycap");
  const summon = document.querySelector("#keys .summon");
  if (!calm && caps.length === 2) {
    const press = () => {
      setTimeout(() => caps[0].classList.add("down"), 0);
      setTimeout(() => caps[0].classList.remove("down"), 150);
      setTimeout(() => caps[1].classList.add("down"), 300);
      setTimeout(() => caps[1].classList.remove("down"), 450);
      setTimeout(() => summon && summon.classList.add("on"), 520);
      setTimeout(() => summon && summon.classList.remove("on"), 2600);
    };
    press();
    setInterval(press, 3400);
  } else if (summon) {
    summon.classList.add("on");
  }

  // "Bring all back": the rows come back one by one.
  const restoreBtn = document.querySelector(".restore-demo");
  const rows = $$("#restore li");
  function restore() {
    if (restoreBtn.disabled) return;
    restoreBtn.disabled = true;
    rows.forEach((row, i) => {
      setTimeout(() => {
        row.querySelector(".dot").style.background = "#ff5a3c";
        const state = row.querySelector(".row-state");
        state.dataset.before = state.dataset.before || state.innerHTML;
        state.innerHTML = '<span class="text-coral">' + t("回来了", "back") + "</span>";
      }, 350 + i * 380);
    });
    setTimeout(() => {
      rows.forEach((row) => {
        row.querySelector(".dot").style.background = "";
        const state = row.querySelector(".row-state");
        state.innerHTML = state.dataset.before;
      });
      restoreBtn.disabled = false;
    }, 4600);
  }
  if (restoreBtn) {
    restoreBtn.addEventListener("click", restore);
    // Show it once on its own when it comes into view.
    const rio = new IntersectionObserver((e) => {
      if (e[0].isIntersecting) { setTimeout(restore, 600); rio.disconnect(); }
    }, { threshold: 0.8 });
    rio.observe(restoreBtn);
  }

  // Mo: three tempers, and a poke.
  const bubble = document.getElementById("mo-bubble");
  const modeCaption = document.getElementById("mode-caption");
  const modes = {
    quiet: { zh: ["只做个表情，你点它才说。", "…"], en: ["Just a look. Click Mo to hear more.", "…"] },
    everyday: { zh: ["先冒个泡，你凑过来再细说。", "有件事～"], en: ["A small hint first. The rest when you come closer.", "Got a sec?"] },
    active: { zh: ["直接把问题和选项递给你，点一下就答。", "接口评审有个问题"], en: ["The whole question, options and all. Click one to answer.", "api-review has a question."] },
  };
  let mode = "everyday";
  const pokes = {
    zh: ["嗨，我是小莫", "别戳啦，痒～", "账单修复断了，点我就能找回", "找回来啦，从刚才的地方接着来", "官网改版正在忙，我盯着呢"],
    en: ["Hi, I'm Moss.", "Hey, that tickles.", "billing-fix stopped. Want it back?", "Back where you left off.", "acme-web is busy. I've got it."],
  };
  let poke = 0;
  const card = document.getElementById("mo-card");
  // Pop something in again, so a repeat still reads as new.
  function pop(el) {
    el.classList.remove("pop");
    void el.offsetWidth;
    el.classList.add("pop");
  }
  // Mo speaks in a bubble; in the active temper it hands over the full message card.
  function say(text) {
    if (!bubble) return;
    if (card) card.classList.add("hidden");
    bubble.classList.remove("hidden");
    bubble.textContent = text;
    pop(bubble);
  }
  function showCard() {
    if (!bubble) return;
    bubble.classList.add("hidden");
    card.classList.remove("hidden");
    pop(card);
  }
  const modeButtons = $$(".mode");
  modeButtons.forEach((b) => b.addEventListener("click", () => { mode = b.dataset.mode; refreshMo(); }));
  const pokeBtn = document.getElementById("mo-poke");
  if (pokeBtn) pokeBtn.addEventListener("click", () => { say(pokes[lang()][poke++ % pokes[lang()].length]); });
  function refreshMo() {
    choose(modeButtons, (b) => b.dataset.mode === mode);
    if (modeCaption) modeCaption.textContent = modes[mode][lang()][0];
    if (mode === "active" && card) showCard();
    else say(modes[mode][lang()][1]);
  }

  // The terminal types `t`, and the picker appears.
  const typed = document.getElementById("typed");
  const picker = document.getElementById("picker");
  if (typed && picker) {
    const run = () => {
      typed.textContent = "";
      picker.classList.add("hidden");
      setTimeout(() => { typed.textContent = "t"; }, 700);
      setTimeout(() => { picker.classList.remove("hidden"); }, 1200);
    };
    const tio = new IntersectionObserver((e) => {
      if (e[0].isIntersecting) { run(); if (!calm) setInterval(run, 6000); tio.disconnect(); }
    });
    tio.observe(typed);
  }

  if (bubble) refreshMo();

  // WebMCP: the same tools as /mcp, offered to an AI agent working in this browser tab.
  const modelContext = document.modelContext || navigator.modelContext;
  if (modelContext) {
    const call = (name, args) =>
      fetch("/mcp", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args || {} } }),
      }).then((r) => r.json()).then((r) => r.result);
    fetch("/mcp", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list" }),
    })
      .then((r) => r.json())
      .then(({ result }) => {
        const tools = result.tools.map((tool) => ({ ...tool, execute: (args) => call(tool.name, args) }));
        if (typeof modelContext.registerTool === "function") return Promise.all(tools.map((tool) => modelContext.registerTool(tool)));
        if (typeof modelContext.provideContext === "function") return modelContext.provideContext({ tools });
      })
      .then(() => dispatchEvent(new Event("cc-moss:webmcp")))
      .catch(() => {});
  }
})();
