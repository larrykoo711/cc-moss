// The site's small bits of life. No framework: a handful of listeners, and every animation
// stops when the visitor prefers less motion. Each page has one language; the build made it so.
(() => {
  const root = document.documentElement;
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lang = () => root.dataset.lang;
  const t = (zh, en) => (lang() === "zh" ? zh : en);
  const $$ = (sel) => [...document.querySelectorAll(sel)];

  // One quiet voice for screen readers: what a click just did.
  const live = document.createElement("p");
  live.className = "sr-only";
  live.setAttribute("aria-live", "polite");
  document.body.append(live);
  const announce = (text) => { live.textContent = ""; setTimeout(() => { live.textContent = text; }, 60); };

  // Segmented controls share one look: the chosen one is lifted. Each says so in its own terms:
  // a tab is selected, a link is the current page, a button is pressed.
  function choose(buttons, on) {
    buttons.forEach((b) => {
      const yes = on(b);
      b.classList.toggle("bg-fg", yes);
      b.classList.toggle("text-bg", yes);
      b.classList.toggle("text-muted", !yes);
      if (b.getAttribute("role") === "tab") b.setAttribute("aria-selected", yes);
      else if (b.tagName === "A") { if (yes) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current"); }
      else b.setAttribute("aria-pressed", yes);
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

  // First visit, and the browser reads the other language: offer it once, never move the page.
  const suggest = root.dataset.suggest;
  const otherLink = suggest && document.querySelector('link[rel="alternate"][hreflang="' + suggest + '"]');
  if (otherLink) {
    const keep = (l) => { try { localStorage.setItem("lang", l); } catch (e) {} };
    const hint = document.createElement("div");
    hint.className = "lang-hint off fixed bottom-4 inset-x-4 sm:inset-x-auto sm:left-1/2 sm:-translate-x-1/2 z-40 flex items-center gap-1 rounded-full bg-raise shadow-[inset_0_0_0_1px_var(--hair),0_20px_50px_-20px_var(--drop)] p-1 pl-2 text-sm";
    hint.setAttribute("lang", suggest === "zh" ? "zh-CN" : "en");
    const go = document.createElement("a");
    go.href = new URL(otherLink.href).pathname + location.hash;
    go.className = "flex-1 inline-flex items-center min-h-10 px-3 rounded-full hover:bg-fg/[.06] transition-colors";
    go.textContent = suggest === "zh" ? "这一页有中文版 →" : "Read this page in English →";
    go.addEventListener("click", () => keep(suggest));
    const close = document.createElement("button");
    close.className = "grid place-items-center w-10 h-10 rounded-full text-muted hover:text-fg hover:bg-fg/[.06] cursor-pointer transition-colors";
    close.setAttribute("aria-label", suggest === "zh" ? "不用了" : "No thanks");
    close.innerHTML = '<svg viewBox="0 0 16 16" class="w-4 h-4" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';
    close.addEventListener("click", () => { keep(lang()); hint.classList.add("off"); setTimeout(() => hint.remove(), 500); });
    hint.append(go, close);
    document.body.append(hint);
    setTimeout(() => hint.classList.remove("off"), 900);
  }
  // Fetch the other skin's pictures once the page is idle, so switching never shows a blank.
  addEventListener("load", () => setTimeout(() => {
    $$("[data-pic]").forEach((img) => {
      for (const t of ["light", "dark"]) new Image().src = picture(img, lang(), t);
    });
  }, 1500));

  // Copy buttons. If the browser won't copy, say so, so nobody pastes the wrong thing.
  $$("[data-copy]").forEach((b) => {
    const label = b.querySelector(".copy-label");
    const before = label.innerHTML;
    b.addEventListener("click", async () => {
      let ok = true;
      try { await navigator.clipboard.writeText(b.dataset.copy); } catch (e) { ok = false; }
      const text = ok ? t("已复制", "Copied") : t("复制不了，请手动选中", "Couldn’t copy. Select it by hand");
      label.innerHTML = '<span class="' + (ok ? "text-coral" : "text-fg/80") + '">' + text + "</span>";
      announce(ok ? t("命令已复制", "Command copied") : text);
      setTimeout(() => { label.innerHTML = before; }, ok ? 1600 : 3200);
    });
  });

  // Phones can't install it: hand the link on (the share sheet, or the clipboard).
  $$("[data-share]").forEach((b) => {
    b.addEventListener("click", async () => {
      const url = location.origin + location.pathname + "#install";
      let text;
      try {
        if (navigator.share) { await navigator.share({ title: document.title, url }); return; }
        await navigator.clipboard.writeText(url);
        text = t("链接已复制，发到 Mac 上打开", "Link copied. Open it on your Mac");
      } catch (e) {
        if (e && e.name === "AbortError") return; // closed the share sheet: nothing to say
        text = t("复制不了，网址是 " + url, "Couldn’t copy. The address is " + url);
      }
      const label = b.querySelector(".share-label");
      const before = label.innerHTML;
      label.innerHTML = '<span class="text-coral">' + text + "</span>";
      announce(text);
      setTimeout(() => { label.innerHTML = before; }, 3200);
    });
  });

  // Rise into view.
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.12 });
  $$(".reveal").forEach((el) => io.observe(el));

  // Nav: Install steps back while the hero's own button is on screen, so the hero asks once.
  const heroCta = document.querySelector("[data-hero-cta]");
  const navCta = document.querySelector(".nav-cta");
  if (heroCta && navCta) {
    new IntersectionObserver(([e]) => {
      navCta.classList.toggle("off", e.isIntersecting);
      navCta.inert = e.isIntersecting;
    }).observe(heroCta);
  }

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
  // Every section is watched, so one without a link of its own (the bento, the story) clears the mark.
  ["top", "restore-story", "tour", "moss", "features", "mo", "terminal", "compare", "faq", "install"].forEach((id) => { const el = document.getElementById(id); if (el) navIO.observe(el); });

  // FAQ: opening one answer closes the others (browsers without details[name] get it here),
  // and a link to #faq-N opens that answer.
  const faqs = $$(".faq-item");
  faqs.forEach((d) => d.addEventListener("toggle", () => {
    if (d.open) faqs.forEach((o) => { if (o !== d && o.open) o.open = false; });
  }));
  const openFaq = () => {
    const d = location.hash && document.getElementById(location.hash.slice(1));
    if (d && d.classList.contains("faq-item")) { d.open = true; d.scrollIntoView({ block: "start" }); }
  };
  openFaq();
  addEventListener("hashchange", openFaq);

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
  const restoreLabel = restoreBtn && restoreBtn.querySelector(".restore-label");
  const restoreBefore = restoreLabel && restoreLabel.innerHTML;
  const rows = $$("#restore li");
  function restore() {
    if (restoreBtn.disabled) return;
    restoreBtn.disabled = true;
    // The button says what it's doing, so a click mid-way never feels dead.
    restoreLabel.textContent = t("正在找回…", "Bringing back…");
    setTimeout(() => { restoreLabel.textContent = t("都回来了", "All back"); }, 350 + rows.length * 380);
    rows.forEach((row, i) => {
      setTimeout(() => {
        row.querySelector(".dot").style.background = "var(--color-coral)";
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
      restoreLabel.innerHTML = restoreBefore;
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
    en: ["Hi, I'm Mo.", "Hey, that tickles.", "billing-fix stopped. Want it back?", "Back where you left off.", "acme-web is busy. I've got it."],
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
